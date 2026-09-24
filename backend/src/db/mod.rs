//! 数据库后端抽象核心。
//!
//! 设计目标：高内聚低耦合。
//! - **高内聚**：所有数据库方言知识（连接初始化、迁移目录、SQL 方言）集中在 `db` 模块，
//!   由 [`AppDb`] 一个 trait 收口，新增后端（如 PostgreSQL）只需新增一个 impl。
//! - **低耦合**：业务代码（services/handlers）只依赖 `DB: AppDb` 泛型约束与
//!   `sqlx::Pool<DB>`，不直接感知具体后端类型；同一份 SQL 字符串代码可在
//!   SQLite / MySQL / PostgreSQL 上编译运行（PostgreSQL 占位符由 `AppQuery` 转换）。
//!
//! 运行时选择：由连接串协议决定（`sqlite://` / `mysql://` / `postgres://`），见 [`DatabaseKind`]。
//! 迁移脚本在编译期通过 `sqlx::migrate!` 按方言嵌入二进制（每后端一个静态
//! [`Migrator`]），运行时按 [`AppDb::migrations`] 选择执行，无需外置迁移目录。

pub mod bootstrap;
pub mod dialect;
pub mod query;

mod mysql;
mod postgres;
mod sqlite;

use std::future::Future;

use sqlx::{Database, Pool, migrate::Migrate};

pub use dialect::{RowsAffected, SqlDialect};
pub use query::{query, query_as, query_scalar};

/// 业务代码统一使用的数据库后端约束。
///
/// 能力打包：
/// - `SqlDialect`: 方言 SQL 片段生成（upsert / ignore / replace）
/// - `Connection: Migrate`: 支持 sqlx 迁移
/// - `AppDb::connect`: 创建并初始化连接池
/// - `AppDb::Query`: 生成后端原生参数占位符的动态查询
/// - `AppDb::migrations`: 编译期嵌入的后端迁移集
///
/// where 子句把 sqlx 的各类能力 bound（Executor/IntoArguments/编解码/列索引）
/// 集中声明一次，业务代码只需一个 `DB: AppDb`；两个后端的具体实现均满足。
pub trait AppDb:
    Database<Connection: sqlx::migrate::Migrate, QueryResult: RowsAffected>
    + SqlDialect
    + Send
    + Sync
    + 'static
    + Sized
{
    // 能力 bound（Executor/IntoArguments/ColumnIndex/值类型编解码）不在此处声明：
    // Rust 的 bound 不跨签名隐式传播，声明在 trait 定义处反而会让 `T: AppDb`
    // 成为不可用的 bound。能力集合由 app_impl! 宏与 #[app_db] 宏在使用点展开，
    // 三者同源（宏内部清单）。

    /// 建立连接池并完成方言相关初始化（SQLite 建库文件 + PRAGMA 调优；MySQL 直连）。
    fn connect(url: &str) -> impl Future<Output = sqlx::Result<Pool<Self>>> + Send;

    /// 当前后端的动态查询实现。
    type Query<'q>: query::AppQuery<'q, Self> + Send + 'q;

    /// 由原始 SQL 创建该后端的动态查询。
    fn make_query<'q>(sql: &'q str) -> Self::Query<'q>;

    /// 该后端编译期嵌入的迁移集（`sqlx::migrate!("./migrations/<backend>")`）。
    fn migrations() -> &'static sqlx::migrate::Migrator;

    /// 已发布初始 schema 的校验和白名单。
    ///
    /// Stellar 将版本 0 的 schema 收敛为单一文件；已知旧版数据库在启动时仅允许
    /// 将这里列出的 checksum 升级为当前 checksum，其他不匹配仍由 SQLx 拒绝。
    fn initial_schema_compatibility_checksums() -> &'static [&'static [u8]] {
        &[]
    }
}

/// 支持的数据库后端，由连接串协议在启动时决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseKind {
    Sqlite,
    MySql,
    Postgres,
}

impl DatabaseKind {
    /// 从连接串协议识别数据库后端。
    ///
    /// `mariadb://` 与 `mysql://` 同归 MySql 后端；`postgresql://` 是 PostgreSQL
    /// URL 的常见别名。
    pub fn from_url(url: &str) -> anyhow::Result<Self> {
        match url.split_once("://") {
            Some(("sqlite", _)) => Ok(Self::Sqlite),
            Some(("mysql", _)) | Some(("mariadb", _)) => Ok(Self::MySql),
            Some(("postgres", _)) | Some(("postgresql", _)) => Ok(Self::Postgres),
            _ => anyhow::bail!(
                "不支持的数据库 URL 协议: {}（支持 sqlite://、mysql:// 或 postgres://）",
                url
            ),
        }
    }
}

/// 创建连接池并执行该后端的数据库迁移。
///
/// 这是业务侧创建数据库访问的唯一入口：
/// ```ignore
/// let pool = db::create_pool::<Sqlite>(&config.database.url).await?;
/// ```
pub async fn create_pool<DB: AppDb>(url: &str) -> anyhow::Result<Pool<DB>>
where
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
    for<'q> <DB as sqlx::Database>::Arguments<'q>: sqlx::IntoArguments<'q, DB> + Default,
    usize: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'a> &'a str: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> i32: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> f64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> bool: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> &'q str: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Vec<u8>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> chrono::DateTime<chrono::Utc>:
        sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> chrono::NaiveDateTime: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> chrono::NaiveDate: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
{
    tracing::info!("Initializing database connection: {}", url);

    let pool = DB::connect(url).await.map_err(|e| {
        tracing::error!("Database connection failed: {}", e);
        e
    })?;

    reconcile_initial_schema_checksum::<DB>(&pool).await?;
    tracing::debug!("Running database migrations...");
    DB::migrations().run(&pool).await.map_err(|e| {
        let hint = migration_hint(&e);
        match hint {
            Some(ref hint) => tracing::error!("Migration execution failed: {}（{}）", e, hint),
            None => tracing::error!("Migration execution failed: {}", e),
        }
        anyhow::anyhow!("{}{}", e, hint.unwrap_or_default())
    })?;

    tracing::info!("Database pool created and migrations applied successfully");
    Ok(pool)
}

/// 仅为版本 0 的已知收敛前 checksum 更新迁移记录。
///
/// 版本 0 的 DDL 被有意收敛为单文件，不能通过新增迁移修复已存在数据库的权限种子。
/// 这里严格按每方言白名单匹配历史 checksum；未知变更继续由 SQLx 的
/// `VersionMismatch` 拦截，避免静默接受任意 schema 漂移。
async fn reconcile_initial_schema_checksum<DB: AppDb>(pool: &Pool<DB>) -> anyhow::Result<()>
where
    for<'c> &'c mut <DB as Database>::Connection: sqlx::Executor<'c, Database = DB>,
    for<'q> Vec<u8>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
{
    let Some(initial_migration) = DB::migrations()
        .iter()
        .find(|migration| migration.version == 0)
    else {
        return Ok(());
    };

    // SQLx creates this table lazily inside `Migrator::run`; create it first so fresh
    // databases still take the normal migration path after the no-op update below.
    let mut connection = pool.acquire().await?;
    connection.ensure_migrations_table().await?;
    for legacy_checksum in DB::initial_schema_compatibility_checksums() {
        let result = query::<DB>(
            "UPDATE _sqlx_migrations SET checksum = ? \
             WHERE version = 0 AND checksum = ?",
        )
        .bind(initial_migration.checksum.to_vec())
        .bind(legacy_checksum.to_vec())
        .execute(&mut *connection)
        .await?;
        if result.rows_affected() > 0 {
            tracing::warn!(
                "Reconciled known version 0 schema checksum before running consolidated migrations"
            );
        }
    }
    Ok(())
}

/// 迁移失败时给出可操作的提示。
///
/// 库中的迁移记录必须与本二进制内置的迁移集一致；不一致通常意味着
/// 数据库与二进制版本不匹配。
fn migration_hint(error: &sqlx::migrate::MigrateError) -> Option<String> {
    use sqlx::migrate::MigrateError;
    match error {
        MigrateError::VersionMissing(version) | MigrateError::VersionMismatch(version) => {
            Some(format!(
                "；数据库中存在本二进制不再内置的迁移记录 version={version}，\
                 通常由旧版本数据库搭配新版本二进制导致：请使用与建库时一致的版本启动，\
                 或在备份后重建数据库。"
            ))
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{mysql::MySql, postgres::Postgres, sqlite::Sqlite};

    #[test]
    fn from_url_detects_kind_by_scheme() {
        assert_eq!(
            DatabaseKind::from_url("sqlite://data/stellar.db").unwrap(),
            DatabaseKind::Sqlite
        );
        assert_eq!(
            DatabaseKind::from_url("mysql://user:pass@localhost:3306/stellar").unwrap(),
            DatabaseKind::MySql
        );
        assert_eq!(
            DatabaseKind::from_url("postgresql://user:pass@localhost:5432/stellar").unwrap(),
            DatabaseKind::Postgres
        );
    }

    #[test]
    fn from_url_rejects_unknown_scheme() {
        assert!(DatabaseKind::from_url("garbage").is_err());
    }

    #[test]
    fn migration_hint_explains_consolidated_ddl() {
        use sqlx::migrate::MigrateError;
        let missing = migration_hint(&MigrateError::VersionMissing(20260917000001));
        assert!(missing.is_some_and(|hint| hint.contains("version=20260917000001")));
        let mismatch = migration_hint(&MigrateError::VersionMismatch(0));
        assert!(mismatch.is_some_and(|hint| hint.contains("version=0")));
        assert!(migration_hint(&MigrateError::Dirty(0)).is_none());
    }

    #[test]
    fn migrations_embedded_per_backend() {
        let sqlite = <Sqlite as AppDb>::migrations();
        let mysql = <MySql as AppDb>::migrations();
        let postgres = <Postgres as AppDb>::migrations();
        // 三方言目录保持同步：迁移数量一致（编译期嵌入，数量变化即编译失败）。
        assert!(!sqlite.migrations.is_empty());
        assert_eq!(sqlite.migrations.len(), mysql.migrations.len());
        assert_eq!(mysql.migrations.len(), postgres.migrations.len());
    }
}
