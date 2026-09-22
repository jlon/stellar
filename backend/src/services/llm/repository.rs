//! LLM Repository - Database operations for LLM service

use crate::db::AppDb;
use crate::db::dialect::RowsAffected;
use crate::db::query as db_query;
use sqlx::Pool;
use stellar_macros::app_impl;
use uuid::Uuid;

use super::UpdateProviderRequest;
use super::credentials::LlmCredentialCipher;
use super::models::*;

/// Repository for LLM database operations
/// Some methods are reserved for future use (admin UI, cache management, usage stats)
pub struct LLMRepository<DB: AppDb> {
    pool: Pool<DB>,
    credential_cipher: LlmCredentialCipher,
}

#[allow(dead_code)]
#[app_impl]
impl<DB: AppDb> LLMRepository<DB> {
    pub fn with_encryption_key(pool: Pool<DB>, encryption_key: &str) -> Self {
        Self { pool, credential_cipher: LlmCredentialCipher::new(encryption_key) }
    }

    #[cfg(test)]
    pub fn new(pool: Pool<DB>) -> Self {
        Self::with_encryption_key(pool, "stellar-llm-provider-test-key")
    }

    /// Get reference to pool (for testing)
    #[cfg(test)]
    pub fn pool(&self) -> &Pool<DB> {
        &self.pool
    }

    /// Get the currently active provider
    pub async fn get_active_provider(&self) -> Result<Option<LLMProvider>, LLMError> {
        db_query::query_as::<_, LLMProvider>(
            r#"SELECT * FROM llm_providers 
               WHERE is_active = TRUE AND enabled = TRUE 
               LIMIT 1"#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(LLMError::from)
    }

    /// Resolve the active provider into request-only credentials. Legacy plaintext
    /// rows are upgraded in place once a server-side encryption key is configured.
    pub async fn get_active_provider_for_use(
        &self,
    ) -> Result<Option<ResolvedLLMProvider>, LLMError> {
        let Some(provider) = self.get_active_provider().await? else {
            return Ok(None);
        };
        self.resolve_provider_for_use(provider).await.map(Some)
    }

    /// List all providers
    pub async fn list_providers(&self) -> Result<Vec<LLMProvider>, LLMError> {
        db_query::query_as::<_, LLMProvider>(
            "SELECT * FROM llm_providers ORDER BY priority ASC, name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(LLMError::from)
    }

    /// Encrypt every pre-v1 credential before serving requests. This keeps
    /// startup fail-closed when a legacy plaintext row exists without a key.
    pub async fn migrate_legacy_credentials(&self) -> Result<usize, LLMError> {
        let providers = self.list_providers().await?;
        let mut migrated = 0;

        for provider in providers {
            let Some(stored) = provider.api_key_encrypted.as_deref() else {
                continue;
            };
            if LlmCredentialCipher::is_encrypted(stored) {
                continue;
            }

            let encrypted = self.credential_cipher.encrypt(stored)?;
            db_query::query(
                "UPDATE llm_providers SET api_key_encrypted = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(encrypted)
            .bind(provider.id)
            .execute(&self.pool)
            .await?;
            migrated += 1;
        }

        Ok(migrated)
    }

    /// Activate a provider (deactivates all others)
    pub async fn activate_provider(&self, provider_id: i64) -> Result<(), LLMError> {
        let mut tx = self.pool.begin().await?;

        db_query::query("UPDATE llm_providers SET is_active = FALSE")
            .execute(&mut *tx)
            .await?;

        let result = db_query::query(
            "UPDATE llm_providers SET is_active = TRUE WHERE id = ? AND enabled = TRUE",
        )
        .bind(provider_id)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(LLMError::ProviderNotFound(provider_id.to_string()));
        }

        tx.commit().await?;
        Ok(())
    }

    /// Get provider by ID
    pub async fn get_provider(&self, id: i64) -> Result<Option<LLMProvider>, LLMError> {
        db_query::query_as::<_, LLMProvider>("SELECT * FROM llm_providers WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(LLMError::from)
    }

    /// Resolve one provider for a server-side test or model invocation.
    pub async fn get_provider_for_use(
        &self,
        id: i64,
    ) -> Result<Option<ResolvedLLMProvider>, LLMError> {
        let Some(provider) = self.get_provider(id).await? else {
            return Ok(None);
        };
        self.resolve_provider_for_use(provider).await.map(Some)
    }

    /// Create a new provider
    pub async fn create_provider(
        &self,
        req: CreateProviderRequest,
    ) -> Result<LLMProvider, LLMError> {
        if req.api_key.trim().is_empty() {
            return Err(LLMError::ApiError("API key is required".to_string()));
        }
        let api_key_encrypted = Some(self.credential_cipher.encrypt(&req.api_key)?);

        let id = db_query::query(
            r#"INSERT INTO llm_providers 
               (name, display_name, api_base, model_name, api_key_encrypted, 
                max_tokens, temperature, timeout_seconds, enabled, is_active, priority)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, TRUE, FALSE, ?)"#,
        )
        .bind(&req.name)
        .bind(&req.display_name)
        .bind(&req.api_base)
        .bind(&req.model_name)
        .bind(&api_key_encrypted)
        .bind(req.max_tokens)
        .bind(req.temperature)
        .bind(req.timeout_seconds)
        .bind(req.priority)
        .insert_id(&self.pool)
        .await?;

        db_query::query_as::<_, LLMProvider>("SELECT * FROM llm_providers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(LLMError::from)
    }

    /// Update provider
    pub async fn update_provider(
        &self,
        id: i64,
        req: UpdateProviderRequest,
    ) -> Result<LLMProvider, LLMError> {
        if req
            .api_key
            .as_deref()
            .is_some_and(|key| key.trim().is_empty())
        {
            return Err(LLMError::ApiError("API key cannot be empty".to_string()));
        }
        let mut sql = String::from("UPDATE llm_providers SET updated_at = CURRENT_TIMESTAMP");
        if req.display_name.is_some() {
            sql.push_str(", display_name = ?");
        }
        if req.api_base.is_some() {
            sql.push_str(", api_base = ?");
        }
        if req.model_name.is_some() {
            sql.push_str(", model_name = ?");
        }
        if req.api_key.is_some() {
            sql.push_str(", api_key_encrypted = ?");
        }
        if req.max_tokens.is_some() {
            sql.push_str(", max_tokens = ?");
        }
        if req.temperature.is_some() {
            sql.push_str(", temperature = ?");
        }
        if req.timeout_seconds.is_some() {
            sql.push_str(", timeout_seconds = ?");
        }
        if req.priority.is_some() {
            sql.push_str(", priority = ?");
        }
        if req.enabled.is_some() {
            sql.push_str(", enabled = ?");
        }
        if req.enabled == Some(false) {
            sql.push_str(", is_active = FALSE");
        }

        sql.push_str(" WHERE id = ?");
        let mut query = db_query::query(&sql);

        if let Some(v) = &req.display_name {
            query = query.bind(v);
        }
        if let Some(v) = &req.api_base {
            query = query.bind(v);
        }
        if let Some(v) = &req.model_name {
            query = query.bind(v);
        }
        if let Some(v) = &req.api_key {
            query = query.bind(self.credential_cipher.encrypt(v)?);
        }
        if let Some(v) = &req.max_tokens {
            query = query.bind(v);
        }
        if let Some(v) = &req.temperature {
            query = query.bind(v);
        }
        if let Some(v) = &req.timeout_seconds {
            query = query.bind(v);
        }
        if let Some(v) = &req.priority {
            query = query.bind(v);
        }
        if let Some(v) = &req.enabled {
            query = query.bind(v);
        }
        query = query.bind(id);

        let result = query.execute(&self.pool).await?;

        if result.rows_affected() == 0 {
            return Err(LLMError::ProviderNotFound(id.to_string()));
        }

        db_query::query_as::<_, LLMProvider>("SELECT * FROM llm_providers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(LLMError::from)
    }

    /// Delete provider
    pub async fn delete_provider(&self, id: i64) -> Result<(), LLMError> {
        let provider = self.get_provider(id).await?;
        match provider {
            None => return Err(LLMError::ProviderNotFound(id.to_string())),
            Some(p) if p.is_active => {
                return Err(LLMError::ApiError(
                    "Cannot delete active provider. Deactivate it first.".to_string(),
                ));
            },
            _ => {},
        }

        db_query::query("DELETE FROM llm_usage_stats WHERE provider_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        db_query::query(
            "UPDATE llm_analysis_sessions SET provider_id = NULL WHERE provider_id = ?",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        let result = db_query::query("DELETE FROM llm_providers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(LLMError::ProviderNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Deactivate a provider
    pub async fn deactivate_provider(&self, id: i64) -> Result<(), LLMError> {
        let result = db_query::query("UPDATE llm_providers SET is_active = FALSE WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(LLMError::ProviderNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Set provider enabled status
    pub async fn set_provider_enabled(
        &self,
        id: i64,
        enabled: bool,
    ) -> Result<LLMProvider, LLMError> {
        let result = db_query::query(
            "UPDATE llm_providers SET enabled = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(enabled)
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(LLMError::ProviderNotFound(id.to_string()));
        }

        if !enabled {
            db_query::query("UPDATE llm_providers SET is_active = FALSE WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }

        db_query::query_as::<_, LLMProvider>("SELECT * FROM llm_providers WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(LLMError::from)
    }

    async fn resolve_provider_for_use(
        &self,
        mut provider: LLMProvider,
    ) -> Result<ResolvedLLMProvider, LLMError> {
        let stored = provider
            .api_key_encrypted
            .clone()
            .ok_or_else(|| LLMError::ApiError("API key not configured".to_string()))?;

        let api_key = if LlmCredentialCipher::is_encrypted(&stored) {
            self.credential_cipher.decrypt(&stored)?
        } else {
            // Records created before credential encryption are upgraded only after
            // a server key has been supplied; without it, fail closed.
            let encrypted = self.credential_cipher.encrypt(&stored)?;
            db_query::query(
                "UPDATE llm_providers SET api_key_encrypted = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(&encrypted)
            .bind(provider.id)
            .execute(&self.pool)
            .await?;
            provider.api_key_encrypted = Some(encrypted);
            stored
        };

        Ok(ResolvedLLMProvider::new(provider, api_key))
    }

    /// Create a new analysis session
    pub async fn create_session(
        &self,
        query_id: &str,
        provider_id: i64,
        cluster_id: Option<i64>,
        scenario: LLMScenario,
    ) -> Result<String, LLMError> {
        let session_id = Uuid::new_v4().to_string();

        db_query::query(
            r#"INSERT INTO llm_analysis_sessions 
               (id, provider_id, scenario, query_id, cluster_id, status)
               VALUES (?, ?, ?, ?, ?, 'pending')"#,
        )
        .bind(&session_id)
        .bind(provider_id)
        .bind(scenario.as_str())
        .bind(query_id)
        .bind(cluster_id)
        .execute(&self.pool)
        .await?;

        Ok(session_id)
    }

    /// Update session status
    pub async fn update_session_status(
        &self,
        session_id: &str,
        status: SessionStatus,
    ) -> Result<(), LLMError> {
        db_query::query("UPDATE llm_analysis_sessions SET status = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Complete a session with metrics
    pub async fn complete_session(
        &self,
        session_id: &str,
        status: SessionStatus,
        input_tokens: i32,
        output_tokens: i32,
        latency_ms: i32,
        error_message: Option<&str>,
    ) -> Result<(), LLMError> {
        db_query::query(
            r#"UPDATE llm_analysis_sessions SET
               status = ?, completed_at = CURRENT_TIMESTAMP,
               input_tokens = ?, output_tokens = ?, latency_ms = ?,
               error_message = ?
               WHERE id = ?"#,
        )
        .bind(status.as_str())
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(latency_ms)
        .bind(error_message)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get session by ID
    pub async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<LLMAnalysisSession>, LLMError> {
        db_query::query_as::<_, LLMAnalysisSession>(
            "SELECT * FROM llm_analysis_sessions WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(LLMError::from)
    }

    /// Save request for debugging
    pub async fn save_request(
        &self,
        session_id: &str,
        request_json: &str,
        sql_hash: &str,
        profile_hash: &str,
    ) -> Result<i64, LLMError> {
        let id = db_query::query(
            r#"INSERT INTO llm_analysis_requests 
               (session_id, request_json, sql_hash, profile_hash)
               VALUES (?, ?, ?, ?)"#,
        )
        .bind(session_id)
        .bind(request_json)
        .bind(sql_hash)
        .bind(profile_hash)
        .insert_id(&self.pool)
        .await?;

        Ok(id)
    }

    /// Save analysis result
    pub async fn save_result(
        &self,
        session_id: &str,
        response_json: &str,
        confidence: Option<f64>,
    ) -> Result<i64, LLMError> {
        let parsed: serde_json::Value = serde_json::from_str(response_json)?;

        let root_causes = parsed
            .get("root_causes")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());
        let causal_chains = parsed
            .get("causal_chains")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());
        let recommendations = parsed
            .get("recommendations")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());
        let summary = parsed
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let hidden_issues = parsed
            .get("hidden_issues")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "[]".to_string());

        let root_cause_count = parsed
            .get("root_causes")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as i32);
        let recommendation_count = parsed
            .get("recommendations")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as i32);

        let id = db_query::query(
            r#"INSERT INTO llm_analysis_results 
               (session_id, root_causes_json, causal_chains_json, recommendations_json,
                summary, hidden_issues_json, confidence_avg, root_cause_count, recommendation_count)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(session_id)
        .bind(&root_causes)
        .bind(&causal_chains)
        .bind(&recommendations)
        .bind(&summary)
        .bind(&hidden_issues)
        .bind(confidence)
        .bind(root_cause_count)
        .bind(recommendation_count)
        .insert_id(&self.pool)
        .await?;

        Ok(id)
    }

    /// Get result by session ID
    pub async fn get_result_by_session(
        &self,
        session_id: &str,
    ) -> Result<Option<LLMAnalysisResult>, LLMError> {
        db_query::query_as::<_, LLMAnalysisResult>(
            "SELECT * FROM llm_analysis_results WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(LLMError::from)
    }

    /// Get cached response
    pub async fn get_cached_response(&self, cache_key: &str) -> Result<Option<String>, LLMError> {
        let result = db_query::query_scalar::<_, String>(
            r#"SELECT response_json FROM llm_cache 
               WHERE cache_key = ? AND expires_at > CURRENT_TIMESTAMP"#,
        )
        .bind(cache_key)
        .fetch_optional(&self.pool)
        .await?;

        if result.is_some() {
            db_query::query(
                r#"UPDATE llm_cache SET 
                   hit_count = hit_count + 1, 
                   last_accessed_at = CURRENT_TIMESTAMP
                   WHERE cache_key = ?"#,
            )
            .bind(cache_key)
            .execute(&self.pool)
            .await?;
        }

        Ok(result)
    }

    /// Cache response
    pub async fn cache_response(
        &self,
        cache_key: &str,
        scenario: LLMScenario,
        request_hash: &str,
        response_json: &str,
        ttl_hours: i64,
    ) -> Result<(), LLMError> {
        let expires_at = chrono::Utc::now() + chrono::Duration::hours(ttl_hours);
        let replace_sql = DB::replace_sql(
            "llm_cache",
            &["cache_key", "scenario", "request_hash", "response_json", "expires_at"],
            &["cache_key"],
        );
        db_query::query(&replace_sql)
            .bind(cache_key)
            .bind(scenario.as_str())
            .bind(request_hash)
            .bind(response_json)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Clean expired cache entries
    pub async fn clean_expired_cache(&self) -> Result<u64, LLMError> {
        let result = db_query::query("DELETE FROM llm_cache WHERE expires_at <= CURRENT_TIMESTAMP")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Record usage for statistics
    pub async fn record_usage(
        &self,
        provider_id: i64,
        input_tokens: i32,
        output_tokens: i32,
        success: bool,
        latency_ms: i32,
        cache_hit: bool,
    ) -> Result<(), LLMError> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

        db_query::query(
            &format!(
                r#"INSERT INTO llm_usage_stats 
               (date, provider_id, total_requests, successful_requests, failed_requests,
                total_input_tokens, total_output_tokens, avg_latency_ms, cache_hits)
               VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?)
               {}"#,
                DB::upsert_suffix(
                    &["date", "provider_id"],
                    &[],
                    &[
                        "total_requests = total_requests + 1",
                        "successful_requests = successful_requests + excluded.successful_requests",
                        "failed_requests = failed_requests + excluded.failed_requests",
                        "total_input_tokens = total_input_tokens + excluded.total_input_tokens",
                        "total_output_tokens = total_output_tokens + excluded.total_output_tokens",
                        "avg_latency_ms = (avg_latency_ms * total_requests + excluded.avg_latency_ms) / (total_requests + 1)",
                        "cache_hits = cache_hits + excluded.cache_hits",
                    ]
                )
            ),
        )
        .bind(&today)
        .bind(provider_id)
        .bind(if success { 1 } else { 0 })
        .bind(if success { 0 } else { 1 })
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(latency_ms as f64)
        .bind(if cache_hit { 1 } else { 0 })
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get usage stats for date range
    pub async fn get_usage_stats(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<LLMUsageStats>, LLMError> {
        db_query::query_as::<_, LLMUsageStats>(
            r#"SELECT * FROM llm_usage_stats 
               WHERE date >= ? AND date <= ?
               ORDER BY date DESC"#,
        )
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await
        .map_err(LLMError::from)
    }
}
