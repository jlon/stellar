//! SQLite 后端：连接池创建与方言级初始化。

use std::path::Path;
use std::time::Duration;

use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};

use super::AppDb;

impl AppDb for Sqlite {
    fn migrations_dir() -> &'static str {
        "migrations/sqlite"
    }

    async fn connect(url: &str) -> sqlx::Result<Pool<Sqlite>> {
        let db_path = url.trim_start_matches("sqlite://");

        if let Some(dir) = Path::new(db_path).parent()
            && !dir.exists()
        {
            tracing::debug!("Creating database directory: {:?}", dir);
            std::fs::create_dir_all(dir).map_err(sqlx::Error::Io)?;
        }

        if !Path::new(db_path).exists() {
            tracing::debug!("Creating database file: {}", db_path);
            std::fs::File::create(db_path).map_err(sqlx::Error::Io)?;
        }

        tracing::debug!("Creating database pool with max_connections=10, acquire_timeout=5s");
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(|e| {
                tracing::error!("Database connection failed: {}", e);
                e
            })?;

        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout = 5000")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous = NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;

        Ok(pool)
    }
}
