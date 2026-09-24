//! PostgreSQL 后端：连接池与插入 id 语义。
//!
//! URL 示例：`postgres://user:pass@localhost:5432/stellar`

use std::time::Duration;

use sqlx::{
    Connection, Pool, Postgres,
    postgres::{PgConnection, PgPoolOptions},
};

use super::{AppDb, query::PostgresQuery};

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/postgres");

const LEGACY_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0xa5, 0xde, 0x51, 0x7b, 0xa4, 0x72, 0x86, 0x4b, 0x15, 0x0f, 0x4f, 0x13, 0x13, 0xed, 0x24, 0xf5,
    0x78, 0xaa, 0x7b, 0x6c, 0x4b, 0x31, 0x1f, 0x25, 0xf3, 0x55, 0x3c, 0x1c, 0x62, 0x7f, 0x61, 0x22,
    0x88, 0x89, 0x2b, 0x97, 0xd2, 0x75, 0xce, 0x64, 0xb0, 0x69, 0x4b, 0xf3, 0xaa, 0xc1, 0x31, 0xb1,
];

impl AppDb for Postgres {
    type Query<'q> = PostgresQuery<'q>;

    fn make_query<'q>(sql: &'q str) -> Self::Query<'q> {
        PostgresQuery::new(sql)
    }

    fn migrations() -> &'static sqlx::migrate::Migrator {
        &MIGRATIONS
    }

    fn initial_schema_compatibility_checksums() -> &'static [&'static [u8]] {
        &[LEGACY_INITIAL_SCHEMA_CHECKSUM]
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
