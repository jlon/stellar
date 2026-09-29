//! MySQL 后端：连接池创建。
//!
//! 字符集与连接参数建议通过 URL 配置，例如：
//! `mysql://user:pass@localhost:3306/stellar?charset=utf8mb4`

use std::time::Duration;

use sqlx::Connection;
use sqlx::mysql::{MySqlConnection, MySqlPoolOptions};
use sqlx::{MySql, Pool};

use super::AppDb;

static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/mysql");

const LEGACY_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x4d, 0x77, 0x46, 0x6a, 0x71, 0x93, 0xb4, 0x4b, 0x49, 0xe8, 0x46, 0xad, 0x7d, 0x07, 0xba, 0xf4,
    0x1f, 0x66, 0x44, 0x06, 0x44, 0xdb, 0xe5, 0x65, 0xdf, 0x3d, 0xfe, 0x89, 0x1b, 0x06, 0x77, 0xe7,
    0x93, 0xa7, 0xae, 0xaf, 0x2d, 0xa8, 0x63, 0xe2, 0x4c, 0xee, 0xbf, 0x2e, 0x21, 0x1c, 0x47, 0x4a,
];
const PREVIOUS_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x36, 0x73, 0x72, 0x53, 0xb0, 0x07, 0xe7, 0x7b, 0xb7, 0x1b, 0x55, 0x98, 0x78, 0x83, 0xf7, 0x8a,
    0x55, 0xb7, 0xdd, 0x78, 0x1d, 0x0b, 0xdf, 0xac, 0x1c, 0x23, 0x23, 0x27, 0xc5, 0x07, 0x52, 0x5e,
    0x96, 0x4e, 0x0c, 0x83, 0x30, 0xf6, 0x71, 0x26, 0xbb, 0x5e, 0x80, 0x91, 0x2a, 0x2d, 0x6c, 0x85,
];
const PRE_CONSOLIDATION_INITIAL_SCHEMA_CHECKSUM: &[u8] = &[
    0x4d, 0x7a, 0xe7, 0xf6, 0xfe, 0xf5, 0x0e, 0xc1, 0xc2, 0xe9, 0x90, 0xf1, 0xb2, 0x76, 0xc1, 0x57,
    0xdc, 0x45, 0xea, 0x1a, 0x00, 0x55, 0x80, 0x5e, 0x81, 0x88, 0x0c, 0x6b, 0xd1, 0x52, 0x80, 0x34,
    0xe2, 0x3e, 0xf4, 0x3d, 0x3d, 0xd2, 0xd3, 0xfc, 0x7a, 0xe1, 0x18, 0x7f, 0x4f, 0x74, 0xa3, 0x49,
];

impl AppDb for MySql {
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

    async fn connect(url: &str) -> sqlx::Result<Pool<MySql>> {
        // 先用单连接探测：Pool::connect 会把底层错误包装为 PoolTimedOut（重试后），
        // 导致无法识别具体原因（如 database 不存在）；直连的错误原样透出。
        let probe = MySqlConnection::connect(url).await;
        if let Err(e) = probe {
            // 与 SQLite（建文件即可用）不同，MySQL 的 database 本身需要预先创建；
            // 表结构会由启动时的 migrations 自动建立。给忘记建库的用户明确指引。
            if let sqlx::Error::Database(db_err) = &e
                // Box<dyn DatabaseError> 无法 downcast（trait 非 Any），trait code()
                // 返回 SQLState（42000）而非 MySQL 内部码 1049；message() 的
                // "Unknown database" 是 MySQL 文档化固定文案，据此识别 1049。
                && db_err.message().starts_with("Unknown database")
            {
                let db_name = url
                    .rsplit_once('/')
                    .map(|(_, p)| p.split('?').next().unwrap_or(p))
                    .unwrap_or("");
                return Err(sqlx::Error::Configuration(
                    format!(
                        "MySQL database '{}' does not exist (error 1049). Create it first, e.g. \
                         CREATE DATABASE {} CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci; \
                         table schema is created automatically by migrations on startup.",
                        db_name, db_name,
                    )
                    .into(),
                ));
            }
            tracing::error!("Database connection failed: {}", e);
            return Err(e);
        }

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
