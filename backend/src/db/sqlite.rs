//! SQLite 后端：连接池创建与方言级初始化。

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use super::AppDb;

impl AppDb for Sqlite {
    type Query<'q> =
        sqlx::query::Query<'q, Self, <Self as sqlx::database::HasArguments<'q>>::Arguments>;

    fn make_query<'q>(sql: &'q str) -> Self::Query<'q> {
        sqlx::query(sql)
    }

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
        // 每连接 PRAGMA 必须走 ConnectOptions：busy_timeout/synchronous/foreign_keys 是
        // 连接级设置，对池（max=10）借用单连接执行 PRAGMA 只会影响那一个连接，
        // 其余连接会丢失 foreign_keys 与 busy_timeout（journal_mode=WAL 是文件级不受影响）。
        let options = url
            .parse::<SqliteConnectOptions>()
            .map_err(|e| {
                tracing::error!("Invalid SQLite URL: {}", e);
                sqlx::Error::Configuration(Box::new(e))
            })?
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);

        tracing::debug!("Creating database pool with max_connections=10, acquire_timeout=5s");
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await
            .map_err(|e| {
                tracing::error!("Database connection failed: {}", e);
                e
            })?;

        Ok(pool)
    }
}
