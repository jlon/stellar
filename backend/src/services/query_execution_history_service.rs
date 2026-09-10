use crate::db::AppDb;
use crate::db::dialect::RowsAffected;
use crate::db::query as db_query;
use chrono::{DateTime, Utc};
use sqlx::Pool;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use stellar_macros::app_impl;

use crate::models::QueryExecutionHistory;
use crate::utils::ApiError;

const MAX_HISTORY_PER_USER_CLUSTER: i64 = 100;

pub struct ExecutionRecord<'a> {
    pub user_id: i64,
    pub cluster_id: i64,
    pub catalog: Option<&'a str>,
    pub database_name: Option<&'a str>,
    pub sql_statement: &'a str,
    pub execution_time_ms: Option<i64>,
    pub row_count: Option<i64>,
    pub success: bool,
    pub error_message: Option<&'a str>,
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    id: i64,
    user_id: i64,
    cluster_id: i64,
    catalog: Option<String>,
    database_name: Option<String>,
    sql_statement: String,
    execution_time_ms: Option<i64>,
    row_count: Option<i64>,
    success: bool,
    error_message: Option<String>,
    created_at: DateTime<Utc>,
}

pub struct QueryExecutionHistoryService<DB: AppDb> {
    pool: Pool<DB>,
}

#[app_impl]
impl<DB: AppDb> QueryExecutionHistoryService<DB> {
    pub fn new(pool: Pool<DB>) -> Self {
        Self { pool }
    }

    fn compute_sql_hash(sql: &str) -> String {
        let normalized = sql.trim().to_lowercase();
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub async fn record_execution(&self, record: ExecutionRecord<'_>) -> Result<i64, ApiError> {
        let sql_hash = Self::compute_sql_hash(record.sql_statement);

        let existing: Option<(i64,)> = db_query::query_as(
            r#"
            SELECT id FROM query_execution_history
            WHERE user_id = ? AND cluster_id = ? AND sql_hash = ?
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(record.user_id)
        .bind(record.cluster_id)
        .bind(&sql_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            ApiError::internal_error(format!("Failed to check existing history: {}", e))
        })?;

        if let Some((existing_id,)) = existing {
            db_query::query(
                r#"
                UPDATE query_execution_history
                SET execution_time_ms = ?, row_count = ?, success = ?, error_message = ?, created_at = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
            )
            .bind(record.execution_time_ms)
            .bind(record.row_count)
            .bind(record.success)
            .bind(record.error_message)
            .bind(existing_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ApiError::internal_error(format!("Failed to update history: {}", e)))?;

            return Ok(existing_id);
        }

        let new_id = db_query::query(
            r#"
            INSERT INTO query_execution_history 
            (user_id, cluster_id, catalog, database_name, sql_statement, sql_hash, execution_time_ms, row_count, success, error_message)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(record.user_id)
        .bind(record.cluster_id)
        .bind(record.catalog)
        .bind(record.database_name)
        .bind(record.sql_statement)
        .bind(&sql_hash)
        .bind(record.execution_time_ms)
        .bind(record.row_count)
        .bind(record.success)
        .bind(record.error_message)
        .insert_id(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to insert history: {}", e)))?;

        self.cleanup_old_records(record.user_id, record.cluster_id)
            .await?;

        Ok(new_id)
    }

    async fn cleanup_old_records(&self, user_id: i64, cluster_id: i64) -> Result<(), ApiError> {
        db_query::query(
            r#"
            DELETE FROM query_execution_history
            WHERE user_id = ? AND cluster_id = ? AND id NOT IN (
                SELECT id FROM query_execution_history
                WHERE user_id = ? AND cluster_id = ?
                ORDER BY created_at DESC
                LIMIT ?
            )
            "#,
        )
        .bind(user_id)
        .bind(cluster_id)
        .bind(user_id)
        .bind(cluster_id)
        .bind(MAX_HISTORY_PER_USER_CLUSTER)
        .execute(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to cleanup old records: {}", e)))?;

        Ok(())
    }

    pub async fn list_history(
        &self,
        user_id: i64,
        cluster_id: i64,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<QueryExecutionHistory>, i64), ApiError> {
        let total: (i64,) = db_query::query_as(
            "SELECT COUNT(*) FROM query_execution_history WHERE user_id = ? AND cluster_id = ?",
        )
        .bind(user_id)
        .bind(cluster_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to count history: {}", e)))?;

        let rows: Vec<HistoryRow> = db_query::query_as(
            r#"
            SELECT id, user_id, cluster_id, catalog, database_name, sql_statement, 
                   execution_time_ms, row_count, success, error_message, created_at
            FROM query_execution_history
            WHERE user_id = ? AND cluster_id = ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(user_id)
        .bind(cluster_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to fetch history: {}", e)))?;

        let items: Vec<QueryExecutionHistory> = rows
            .into_iter()
            .map(|row| QueryExecutionHistory {
                id: row.id,
                user_id: row.user_id,
                cluster_id: row.cluster_id,
                catalog: row.catalog,
                database_name: row.database_name,
                sql_statement: row.sql_statement,
                execution_time_ms: row.execution_time_ms,
                row_count: row.row_count,
                success: row.success,
                error_message: row.error_message,
                created_at: row.created_at.to_rfc3339(),
            })
            .collect();

        Ok((items, total.0))
    }

    pub async fn delete_history(&self, user_id: i64, history_id: i64) -> Result<bool, ApiError> {
        let result =
            db_query::query("DELETE FROM query_execution_history WHERE id = ? AND user_id = ?")
                .bind(history_id)
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    ApiError::internal_error(format!("Failed to delete history: {}", e))
                })?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn clear_history(&self, user_id: i64, cluster_id: i64) -> Result<i64, ApiError> {
        let result = db_query::query(
            "DELETE FROM query_execution_history WHERE user_id = ? AND cluster_id = ?",
        )
        .bind(user_id)
        .bind(cluster_id)
        .execute(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to clear history: {}", e)))?;

        Ok(result.rows_affected() as i64)
    }
}
