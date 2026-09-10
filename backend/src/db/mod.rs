//! 数据库后端抽象核心。
//!
//! 设计目标：高内聚低耦合。
//! - **高内聚**：所有数据库方言知识（连接初始化、迁移目录、SQL 方言）集中在 `db` 模块，
//!   由 [`AppDb`] 一个 trait 收口，新增后端（如 PostgreSQL）只需新增一个 impl。
//! - **低耦合**：业务代码（services/handlers）只依赖 `DB: AppDb` 泛型约束与
//!   `sqlx::Pool<DB>`，不直接感知具体后端类型；同一份 SQL 字符串代码可在
//!   SQLite / MySQL 上编译运行（两者占位符同为 `?`）。
//!
//! 运行时选择：由连接串协议决定（`sqlite://` / `mysql://`），见 [`DatabaseKind`]。

pub mod dialect;

mod mysql;
mod sqlite;

use std::future::Future;
use std::path::Path;

use sqlx::{Database, Pool};

pub use dialect::{LastInsertId, RowsAffected, SqlDialect};

/// 能力 bound 清单：泛型数据库后端上下文的完整能力集合。
///
/// sqlx 的多后端抽象存在若干只能按具体后端满足的 bound（`&mut Connection: Executor`、
/// `Arguments: IntoArguments`、`ColumnIndex`、各值类型的 Encode/Decode/Type）。
/// 这些 bound 不随 `T: Database` 自动获得，必须显式约束。
///
/// 本宏是这些 bound 的**单一事实源**：
/// - 业务层泛型 impl 块 / 函数：`where app_db_where!(DB)`（impl 块级声明一次，
///   其全部方法共享，无需逐方法重复）
///
/// 注意：where 子句位置不能直接调用宏，因此 `AppDb` trait 定义处维护了一份
/// 展开后的同内容清单（见下方 where 子句）。**修改本宏时同步修改 trait 定义。**
///
/// 新增后端时只需提供 sqlx 的具体能力实现，bound 清单不变。
#[macro_export]
macro_rules! app_db_where {
    ($DB:ident) => {
        $DB: $crate::db::AppDb,
        // 连接可作为 Executor（&Pool<DB> / &mut Transaction 执行查询的前提）
        for<'c> &'c mut <$DB as ::sqlx::Database>::Connection:
            ::sqlx::Executor<'c, Database = $DB>,
        // 参数类型可用于 query()/query_with()
        for<'q> <$DB as ::sqlx::HasArguments<'q>>::Arguments:
            ::sqlx::IntoArguments<'q, $DB> + Default,
        // 列索引能力（row.get("col") / row.get(0)）
        ::std::primitive::usize: ::sqlx::ColumnIndex<<$DB as ::sqlx::Database>::Row>,
        for<'a> &'a ::std::primitive::str: ::sqlx::ColumnIndex<<$DB as ::sqlx::Database>::Row>,
        // 业务代码使用的值类型编解码全集
        for<'q> i64: ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> i32: ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> f64: ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> bool: ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> ::std::string::String:
            ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> &'q ::std::primitive::str: ::sqlx::Encode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> ::chrono::DateTime<::chrono::Utc>:
            ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> ::chrono::NaiveDateTime:
            ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
        for<'q> ::chrono::NaiveDate:
            ::sqlx::Encode<'q, $DB> + ::sqlx::Decode<'q, $DB> + ::sqlx::Type<$DB>,
    };
}

/// 业务代码统一使用的数据库后端约束。
///
/// 能力打包：
/// - `SqlDialect`: 方言 SQL 片段生成（upsert / ignore / replace）
/// - `QueryResult: LastInsertId`: 读取自增主键
/// - `Connection: Migrate`: 支持 sqlx 迁移
/// - `AppDb::connect`: 创建并初始化连接池
/// - `AppDb::migrations_dir`: 该后端的迁移脚本目录
///
/// where 子句把 sqlx 的各类能力 bound（Executor/IntoArguments/编解码/列索引）
/// 集中声明一次，业务代码只需一个 `DB: AppDb`；两个后端的具体实现均满足。
pub trait AppDb:
    Database<Connection: sqlx::migrate::Migrate, QueryResult: LastInsertId + RowsAffected>
    + SqlDialect
    + Send
    + Sync
    + 'static
    + Sized
{
    // 能力 bound（Executor/IntoArguments/ColumnIndex/值类型编解码）不在此处声明：
    // Rust 的 bound 不跨签名隐式传播，声明在 trait 定义处反而会让 `T: AppDb`
    // 成为不可用的 bound。能力集合由 app_impl! 宏与 #[app_db] 宏在使用点展开，
    // 两者同源（宏内部清单）。

    /// 建立连接池并完成方言相关初始化（SQLite 建库文件 + PRAGMA 调优；MySQL 直连）。
    fn connect(url: &str) -> impl Future<Output = sqlx::Result<Pool<Self>>> + Send;

    /// 该后端对应的迁移脚本目录（migrations/<后端>/）。
    fn migrations_dir() -> &'static str;
}

/// 支持的数据库后端，由连接串协议在启动时决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseKind {
    Sqlite,
    MySql,
}

impl DatabaseKind {
    /// 从连接串协议识别数据库后端。
    pub fn from_url(url: &str) -> anyhow::Result<Self> {
        match url.split_once("://") {
            Some(("sqlite", _)) => Ok(Self::Sqlite),
            Some(("mysql", _)) => Ok(Self::MySql),
            _ => anyhow::bail!("不支持的数据库 URL 协议: {}（支持 sqlite:// 或 mysql://）", url),
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
    for<'q> <DB as sqlx::database::HasArguments<'q>>::Arguments:
        sqlx::IntoArguments<'q, DB> + Default,
    usize: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'a> &'a str: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> i32: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> f64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> bool: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> &'q str: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
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

    let migrations_dir = find_migrations_dir(DB::migrations_dir());
    tracing::info!("Using migrations from: {}", migrations_dir);

    tracing::debug!("Running database migrations...");
    sqlx::migrate::Migrator::new(Path::new(&migrations_dir))
        .await
        .map_err(|e| {
            tracing::error!("Migration setup failed: {}", e);
            e
        })?
        .run(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Migration execution failed: {}", e);
            e
        })?;

    tracing::info!("Database pool created and migrations applied successfully");
    Ok(pool)
}

/// 在常见工作目录下探测迁移脚本目录（兼容从仓库根 / backend / dist 等目录启动）。
fn find_migrations_dir(default_path: &str) -> String {
    let possible_paths =
        [format!("./{default_path}"), format!("../{default_path}"), default_path.to_string()];

    for path in &possible_paths {
        if Path::new(path).exists() {
            tracing::debug!("Found migrations directory at: {}", path);
            return path.to_string();
        }
    }

    tracing::warn!("No migrations directory found, using default: {default_path}");
    default_path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{mysql::MySql, sqlite::Sqlite};

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
    }

    #[test]
    fn from_url_rejects_unknown_scheme() {
        assert!(DatabaseKind::from_url("postgres://localhost/db").is_err());
        assert!(DatabaseKind::from_url("garbage").is_err());
    }

    #[test]
    fn migrations_dir_follows_backend() {
        assert_eq!(<Sqlite as AppDb>::migrations_dir(), "migrations/sqlite");
        assert_eq!(<MySql as AppDb>::migrations_dir(), "migrations/mysql");
    }
}
