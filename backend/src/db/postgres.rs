//! PostgreSQL 后端：连接池与插入 id 语义。
//!
//! URL 示例：`postgres://user:pass@localhost:5432/stellar`

use std::time::Duration;

use sqlx::{
    Connection, Pool, Postgres,
    postgres::{PgConnection, PgPoolOptions},
};

use super::{AppDb, query::PostgresQuery};

impl AppDb for Postgres {
    type Query<'q> = PostgresQuery<'q>;

    fn make_query<'q>(sql: &'q str) -> Self::Query<'q> {
        PostgresQuery::new(sql)
    }

    fn migrations_dir() -> &'static str {
        "migrations/postgres"
    }

    async fn connect(url: &str) -> sqlx::Result<Pool<Postgres>> {
        // 与 MySQL 相同：先用直连探测。Pool::connect 的重试可能只留下
        // PoolTimedOut，掩盖 PostgreSQL 的具体连接错误。
        if let Err(error) = PgConnection::connect(url).await {
            if let sqlx::Error::Database(db_error) = &error
                && db_error.code().as_deref() == Some("3D000")
            {
                let database_name = url
                    .rsplit_once('/')
                    .map(|(_, path)| path.split('?').next().unwrap_or(path))
                    .unwrap_or("");
                return Err(sqlx::Error::Configuration(
                    format!(
                        "PostgreSQL database '{}' does not exist (SQLSTATE 3D000). Create it first, e.g. \
                         CREATE DATABASE {}; table schema is created automatically by migrations on startup.",
                        database_name, database_name,
                    )
                    .into(),
                ));
            }
            tracing::error!("Database connection failed: {}", error);
            return Err(error);
        }

        tracing::debug!(
            "Creating PostgreSQL database pool with max_connections=10, acquire_timeout=5s"
        );
        PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(|error| {
                tracing::error!("Database connection failed: {}", error);
                error
            })
    }
}
