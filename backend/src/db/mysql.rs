//! MySQL 后端：连接池创建。
//!
//! 字符集与连接参数建议通过 URL 配置，例如：
//! `mysql://user:pass@localhost:3306/stellar?charset=utf8mb4`

use std::time::Duration;

use sqlx::{MySql, Pool, mysql::MySqlPoolOptions};

use super::AppDb;

impl AppDb for MySql {
    fn migrations_dir() -> &'static str {
        "migrations/mysql"
    }

    async fn connect(url: &str) -> sqlx::Result<Pool<MySql>> {
        tracing::debug!("Creating database pool with max_connections=10, acquire_timeout=5s");
        let pool = MySqlPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(|e| {
                tracing::error!("Database connection failed: {}", e);
                e
            })?;

        Ok(pool)
    }
}
