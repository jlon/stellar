use crate::utils::error::ApiError;
use mysql_async::{Conn, Pool, prelude::Queryable};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct MySQLClient {
    pool: Arc<Pool>,
    /// 单条语句上限（None = 不限制，仅交互式会话使用）。
    /// 列表类读取必须设上限，否则一次 FE 抖动就把所有列表页变成无限转圈。
    query_timeout: Option<Duration>,
}

/// MySQLSession wraps a single database connection for executing multiple operations
/// with persistent context (catalog, database)
pub struct MySQLSession {
    conn: Conn,
    query_timeout: Option<Duration>,
}

impl MySQLClient {
    pub fn from_pool(pool: Pool) -> Self {
        Self { pool: Arc::new(pool), query_timeout: None }
    }

    /// 设置单条语句超时（列表类读取必设；交互式执行保持 None）。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = Some(timeout);
        self
    }

    async fn bounded<T, F: std::future::Future<Output = Result<T, ApiError>>>(
        &self,
        what: &str,
        fut: F,
    ) -> Result<T, ApiError> {
        match self.query_timeout {
            Some(d) => tokio::time::timeout(d, fut).await.map_err(|_| {
                ApiError::cluster_connection_failed(format!(
                    "集群查询超时（{}，{}s）：FE 可能过载，请稍后重试",
                    what,
                    d.as_secs()
                ))
            })?,
            None => fut.await,
        }
    }

    /// Create a new session with a dedicated connection from the pool
    /// Use this when you need to execute multiple operations on the same connection
    /// with persistent context (e.g., USE DATABASE)
    pub async fn create_session(&self) -> Result<MySQLSession, ApiError> {
        let conn = self.pool.get_conn().await.map_err(|e| {
            tracing::error!("Failed to get connection from pool: {}", e);
            ApiError::cluster_connection_failed(format!("Failed to get connection: {}", e))
        })?;
        Ok(MySQLSession { conn, query_timeout: self.query_timeout })
    }

    /// Execute a query and return results as (column_names, rows)
    pub async fn query_raw(&self, sql: &str) -> Result<(Vec<String>, Vec<Vec<String>>), ApiError> {
        let sql = sql.to_string();
        self.bounded("SQL 查询", async move {
            let mut conn = self.pool.get_conn().await.map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                ApiError::cluster_connection_failed(format!("Failed to get connection: {}", e))
            })?;

            let rows: Vec<mysql_async::Row> = conn.query(sql).await.map_err(|e| {
                tracing::error!("MySQL query execution failed: {}", e);
                ApiError::internal_error(format!("SQL execution failed: {}", e))
            })?;

            tracing::debug!("Query returned {} rows", rows.len());

            drop(conn);

            Ok(process_query_result(rows))
        })
        .await
    }

    /// Execute a query and return results as Vec<serde_json::Value> (JSON objects)
    /// Each row is a JSON object with column names as keys
    pub async fn query(&self, sql: &str) -> Result<Vec<serde_json::Value>, ApiError> {
        let (column_names, rows) = self.query_raw(sql).await?;

        let mut result = Vec::new();
        for row in rows {
            let mut obj = serde_json::Map::new();
            for (i, col_name) in column_names.iter().enumerate() {
                if let Some(value) = row.get(i) {
                    obj.insert(col_name.clone(), serde_json::Value::String(value.clone()));
                }
            }
            result.push(serde_json::Value::Object(obj));
        }

        Ok(result)
    }

    pub async fn execute(&self, sql: &str) -> Result<u64, ApiError> {
        let sql = sql.to_string();
        self.bounded("SQL 执行", async move {
            let mut conn = self.pool.get_conn().await.map_err(|e| {
                tracing::error!("Failed to get connection for execute: {}", e);
                ApiError::cluster_connection_failed(format!("Failed to get connection: {}", e))
            })?;

            let result: Vec<mysql_async::Row> = conn.query(sql).await.map_err(|e| {
                tracing::error!("MySQL execute failed: {}", e);
                ApiError::cluster_connection_failed(format!("Query failed: {}", e))
            })?;

            drop(conn);

            Ok(result.len() as u64)
        })
        .await
    }
}

impl MySQLSession {
    /// Set catalog context on this session's connection
    ///
    /// # Syntax Differences
    /// - StarRocks: `SET CATALOG catalog_name`
    /// - Doris: `SWITCH catalog_name`
    pub async fn use_catalog(
        &mut self,
        catalog: &str,
        cluster_type: &crate::models::cluster::ClusterType,
    ) -> Result<(), ApiError> {
        use crate::models::cluster::ClusterType;

        if catalog.is_empty() || catalog == "default_catalog" {
            return Ok(());
        }

        let quoted_catalog = quote_identifier(catalog)?;
        let switch_sql = match cluster_type {
            ClusterType::StarRocks => format!("SET CATALOG {quoted_catalog}"),
            ClusterType::Doris => format!("SWITCH {quoted_catalog}"),
        };

        self.execute_statement(&switch_sql, "切换 Catalog").await?;

        tracing::debug!("Successfully switched to catalog: {}", catalog);
        Ok(())
    }

    /// Set database context on this session's connection
    pub async fn use_database(&mut self, database: &str) -> Result<(), ApiError> {
        if database.is_empty() {
            return Ok(());
        }

        let use_db_sql = format!("USE {}", quote_identifier(database)?);
        self.execute_statement(&use_db_sql, "切换数据库").await?;
        Ok(())
    }

    pub async fn execute(
        &mut self,
        sql: &str,
    ) -> Result<(Vec<String>, Vec<Vec<String>>, u128), ApiError> {
        let start = std::time::Instant::now();
        let rows = self.query_rows(sql).await?;
        let (columns, data_rows) = process_query_result(rows);
        let execution_time_ms = start.elapsed().as_millis();
        tracing::debug!("SQL: '{}' -> {} rows in {}ms", sql, data_rows.len(), execution_time_ms);
        Ok((columns, data_rows, execution_time_ms))
    }

    pub async fn query_with_params<P>(
        &mut self,
        sql: &str,
        params: P,
    ) -> Result<(Vec<String>, Vec<Vec<String>>), ApiError>
    where
        P: Into<mysql_async::Params>,
    {
        let query = self.conn.exec(sql, params.into());
        let rows = match self.query_timeout {
            Some(timeout) => tokio::time::timeout(timeout, query)
                .await
                .map_err(|_| query_timeout_error("SQL 查询", timeout))?,
            None => query.await,
        }
        .map_err(|error| {
            tracing::error!("MySQL query execution failed: {}", error);
            ApiError::internal_error(format!("SQL execution failed: {error}"))
        })?;

        Ok(process_query_result(rows))
    }

    async fn query_rows(&mut self, sql: &str) -> Result<Vec<mysql_async::Row>, ApiError> {
        let query = self.conn.query(sql);
        let rows = match self.query_timeout {
            Some(timeout) => tokio::time::timeout(timeout, query)
                .await
                .map_err(|_| query_timeout_error("SQL 查询", timeout))?,
            None => query.await,
        }
        .map_err(|error| {
            tracing::error!("MySQL query execution failed: {}", error);
            ApiError::internal_error(format!("SQL execution failed: {error}"))
        })?;
        Ok(rows)
    }

    async fn execute_statement(&mut self, sql: &str, operation: &str) -> Result<(), ApiError> {
        self.query_rows(sql).await.map(|_| ()).map_err(|error| {
            tracing::warn!("{} failed: {}", operation, error);
            error
        })
    }
}

fn quote_identifier(value: &str) -> Result<String, ApiError> {
    if value.is_empty()
        || value.len() > 256
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::invalid_data("Invalid SQL identifier"));
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}

fn query_timeout_error(operation: &str, timeout: Duration) -> ApiError {
    ApiError::cluster_connection_failed(format!(
        "集群查询超时（{}，{}s）：FE 可能过载，请稍后重试",
        operation,
        timeout.as_secs()
    ))
}

fn process_query_result(rows: Vec<mysql_async::Row>) -> (Vec<String>, Vec<Vec<String>>) {
    if rows.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let col_count = rows[0].columns_ref().len();
    let row_count = rows.len();

    let mut columns = Vec::with_capacity(col_count);
    let mut result_rows = Vec::with_capacity(row_count);

    for col in rows[0].columns_ref().iter() {
        columns.push(col.name_str().to_string());
    }

    if row_count > 100 && col_count > 5 {
        process_query_result_batch(rows, &mut result_rows);
    } else {
        for row in rows.iter() {
            let mut row_data = Vec::with_capacity(col_count);
            for col_idx in 0..col_count {
                row_data.push(value_to_string_optimized(&row[col_idx]));
            }
            result_rows.push(row_data);
        }
    }

    (columns, result_rows)
}

// Batch processing for large datasets - processes multiple values at once
fn process_query_result_batch(rows: Vec<mysql_async::Row>, result_rows: &mut Vec<Vec<String>>) {
    for row in rows.iter() {
        let col_count = row.columns_ref().len();
        let mut row_data = Vec::with_capacity(col_count);

        for col_idx in 0..col_count {
            row_data.push(value_to_string_optimized(&row[col_idx]));
        }

        result_rows.push(row_data);
    }
}

// Optimized value conversion with minimal allocations
fn value_to_string_optimized(value: &mysql_async::Value) -> String {
    match value {
        mysql_async::Value::NULL => "NULL".to_string(),
        mysql_async::Value::Bytes(bytes) => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => String::from_utf8_lossy(bytes).to_string(),
        },
        mysql_async::Value::Int(i) => {
            let mut s = String::with_capacity(12);
            use std::fmt::Write;
            let _ = write!(s, "{}", i);
            s
        },
        mysql_async::Value::UInt(u) => {
            let mut s = String::with_capacity(12);
            use std::fmt::Write;
            let _ = write!(s, "{}", u);
            s
        },
        mysql_async::Value::Float(f) => {
            let mut s = String::with_capacity(16);
            use std::fmt::Write;
            let _ = write!(s, "{}", f);
            s
        },
        mysql_async::Value::Double(d) => {
            let mut s = String::with_capacity(24);
            use std::fmt::Write;
            let _ = write!(s, "{}", d);
            s
        },
        mysql_async::Value::Date(year, month, day, hour, minute, second, _micro) => {
            let mut s = String::with_capacity(19);
            s.push_str(&format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, minute, second
            ));
            s
        },
        mysql_async::Value::Time(_neg, days, hours, minutes, seconds, _micro) => {
            let total_hours = days * 24 + (*hours as u32);
            format!("{}:{:02}:{:02}", total_hours, minutes, seconds)
        },
    }
}
