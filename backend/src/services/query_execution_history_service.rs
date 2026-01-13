use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use sqlx::SqlitePool;

use crate::models::QueryExecutionHistory;
use crate::utils::ApiError;

const MAX_HISTORY_PER_USER_CLUSTER: i64 = 100;

pub struct QueryExecutionHistoryService {
    pool: SqlitePool,
}

impl QueryExecutionHistoryService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn compute_sql_hash(sql: &str) -> String {
        let normalized = sql.trim().to_lowercase();
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub async fn record_execution(
        &self,
        user_id: i64,
        cluster_id: i64,
        catalog: Option<&str>,
        database_name: Option<&str>,
        sql_statement: &str,
        execution_time_ms: Option<i64>,
        row_count: Option<i64>,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<i64, ApiError> {
        let sql_hash = Self::compute_sql_hash(sql_statement);

        let existing: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT id FROM query_execution_history
            WHERE user_id = ? AND cluster_id = ? AND sql_hash = ?
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(cluster_id)
        .bind(&sql_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to check existing history: {}", e)))?;

        if let Some((existing_id,)) = existing {
            sqlx::query(
                r#"
                UPDATE query_execution_history
                SET execution_time_ms = ?, row_count = ?, success = ?, error_message = ?, created_at = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
            )
            .bind(execution_time_ms)
            .bind(row_count)
            .bind(success)
            .bind(error_message)
            .bind(existing_id)
            .execute(&self.pool)
            .await
            .map_err(|e| ApiError::internal_error(format!("Failed to update history: {}", e)))?;

            return Ok(existing_id);
        }

        let result = sqlx::query(
            r#"
            INSERT INTO query_execution_history 
            (user_id, cluster_id, catalog, database_name, sql_statement, sql_hash, execution_time_ms, row_count, success, error_message)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(cluster_id)
        .bind(catalog)
        .bind(database_name)
        .bind(sql_statement)
        .bind(&sql_hash)
        .bind(execution_time_ms)
        .bind(row_count)
        .bind(success)
        .bind(error_message)
        .execute(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to insert history: {}", e)))?;

        let new_id = result.last_insert_rowid();

        self.cleanup_old_records(user_id, cluster_id).await?;

        Ok(new_id)
    }

    async fn cleanup_old_records(&self, user_id: i64, cluster_id: i64) -> Result<(), ApiError> {
        sqlx::query(
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
        let total: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM query_execution_history WHERE user_id = ? AND cluster_id = ?",
        )
        .bind(user_id)
        .bind(cluster_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to count history: {}", e)))?;

        let rows: Vec<(i64, i64, i64, Option<String>, Option<String>, String, Option<i64>, Option<i64>, bool, Option<String>, String)> = sqlx::query_as(
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
                id: row.0,
                user_id: row.1,
                cluster_id: row.2,
                catalog: row.3,
                database_name: row.4,
                sql_statement: row.5,
                execution_time_ms: row.6,
                row_count: row.7,
                success: row.8,
                error_message: row.9,
                created_at: row.10,
            })
            .collect();

        Ok((items, total.0))
    }

    pub async fn delete_history(&self, user_id: i64, history_id: i64) -> Result<bool, ApiError> {
        let result = sqlx::query(
            "DELETE FROM query_execution_history WHERE id = ? AND user_id = ?",
        )
        .bind(history_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| ApiError::internal_error(format!("Failed to delete history: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn clear_history(&self, user_id: i64, cluster_id: i64) -> Result<i64, ApiError> {
        let result = sqlx::query(
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
