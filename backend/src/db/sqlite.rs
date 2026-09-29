//! SQLite 后端：连接池创建与方言级初始化。

use std::path::Path;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use super::AppDb;

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/sqlite");

const LEGACY_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x01, 0x18, 0x87, 0x9c, 0x52, 0x23, 0x0a, 0x56, 0xb6, 0xd5, 0x9c, 0x5b, 0xb5, 0xce, 0x33, 0x4e,
    0x2c, 0x2c, 0xa8, 0x73, 0x62, 0xca, 0xd7, 0x2d, 0x83, 0x77, 0x3e, 0x36, 0x7f, 0xb9, 0x8a, 0x92,
    0x1f, 0x2b, 0x82, 0x80, 0xd0, 0xdb, 0xcf, 0x13, 0xce, 0xbb, 0xff, 0x8b, 0x31, 0x04, 0x87, 0x93,
];
const PREVIOUS_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x75, 0x2c, 0xd0, 0x4c, 0xcd, 0x4b, 0xb8, 0xff, 0x20, 0xc9, 0xbf, 0x08, 0x2b, 0x03, 0xd9, 0x92,
    0x20, 0xb1, 0x7e, 0x22, 0x97, 0x60, 0xac, 0xa3, 0xb3, 0xf6, 0x91, 0xa3, 0x5f, 0x5c, 0x21, 0xa9,
    0x26, 0x53, 0x3a, 0x36, 0x94, 0xe0, 0x05, 0x4d, 0x3a, 0x1e, 0x0f, 0x04, 0xdb, 0x05, 0x7c, 0xc4,
];
const PRE_CONSOLIDATION_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x90, 0x14, 0x79, 0xb2, 0x64, 0x3f, 0x8d, 0x49, 0x25, 0x2d, 0xf3, 0xb3, 0xb4, 0x3f, 0xc4, 0x85,
    0xec, 0x87, 0xdf, 0xd9, 0x60, 0xd1, 0x75, 0x3d, 0x96, 0xce, 0x9c, 0xd2, 0xe6, 0xf8, 0x86, 0xd1,
    0x14, 0x8c, 0x3d, 0x06, 0xaa, 0x52, 0x98, 0xeb, 0xab, 0xe2, 0xac, 0x95, 0x2a, 0x3d, 0x3b, 0xde,
];

impl AppDb for Sqlite {
    type Query<'q> = sqlx::query::Query<'q, Self, <Self as sqlx::Database>::Arguments<'q>>;

    fn make_query<'q>(sql: &'q str) -> Self::Query<'q> {
        sqlx::query(sql)
    }

    fn migrations() -> &'static sqlx::migrate::Migrator {
        &MIGRATIONS
    }

    fn initial_schema_compatibility_checksums() -> &'static [&'static [u8]] {
        &[
            LEGACY_INITIAL_SCHEMA_CHECKSUM,
            PREVIOUS_INITIAL_SCHEMA_CHECKSUM,
            PRE_CONSOLIDATION_INITIAL_SCHEMA_CHECKSUM,
        ]
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
