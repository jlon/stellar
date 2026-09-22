//! SQL 方言差异抽象。
//!
//! 所有"SQLite 与 MySQL 语法不同"的知识都收口在本模块：
//! - `SqlDialect`: 生成方言化的 upsert / ignore / replace SQL 片段
//!
//! 均以关联函数形式静态分发（`DB::upsert_suffix(...)`），零运行时开销，
//! 业务代码只需一个 `DB: AppDb` 约束即可获得全部能力。

use sqlx::{
    Database, MySql, Postgres, Sqlite, mysql::MySqlQueryResult, postgres::PgQueryResult,
    sqlite::SqliteQueryResult,
};

/// 为字符串承载的强枚举生成 sqlx 的泛型 `Type/Decode/Encode` impl。
///
/// sqlx 的 `#[derive(sqlx::Type)]` 对强枚举只生成 per-backend impl，其 MySQL
/// compatible() 仅认 `ENUM` 类型标记，与 VARCHAR/TEXT 列不兼容（实测报
/// `mismatched types: as SQL type ENUM is not compatible with SQL type VARCHAR`）。
/// 本宏将枚举与 `String` 同构（编码用 Display，解码用宽松解析），兼容任意
/// 字符串列；两个后端均适用，替代 derive。
///
/// `$parse`: `fn(&str) -> $ty`，解析数据库中的字符串值（宽松解析，未知值
/// 回退默认变体；与 serde 严格语义的差异是可接受的 DB 层容错）。
#[macro_export]
macro_rules! impl_string_backed_db_type {
    ($ty:ty, $parse:expr) => {
        impl<DB: ::sqlx::Database> ::sqlx::Type<DB> for $ty
        where
            String: ::sqlx::Type<DB>,
        {
            fn type_info() -> DB::TypeInfo {
                <String as ::sqlx::Type<DB>>::type_info()
            }

            fn compatible(ty: &DB::TypeInfo) -> bool {
                <String as ::sqlx::Type<DB>>::compatible(ty)
            }
        }

        impl<'r, DB: ::sqlx::Database> ::sqlx::Decode<'r, DB> for $ty
        where
            String: ::sqlx::Decode<'r, DB>,
        {
            fn decode(
                value: <DB as ::sqlx::Database>::ValueRef<'r>,
            ) -> ::std::result::Result<
                Self,
                ::std::boxed::Box<dyn ::std::error::Error + Send + Sync>,
            > {
                let s = <String as ::sqlx::Decode<'r, DB>>::decode(value)?;
                let parse: fn(&str) -> Self = $parse;
                Ok(parse(&s))
            }
        }

        impl<'q, DB: ::sqlx::Database> ::sqlx::Encode<'q, DB> for $ty
        where
            String: ::sqlx::Encode<'q, DB>,
        {
            fn encode_by_ref(
                &self,
                buf: &mut <DB as ::sqlx::Database>::ArgumentBuffer<'q>,
            ) -> ::std::result::Result<
                ::sqlx::encode::IsNull,
                ::std::boxed::Box<dyn ::std::error::Error + Send + Sync>,
            > {
                self.to_string().encode_by_ref(buf)
            }
        }
    };
}

/// 从执行结果读取受影响行数。
///
/// 两种后端都提供 `rows_affected()` 但无共享 trait，泛型上下文需要本抽象。
pub trait RowsAffected {
    fn rows_affected(&self) -> u64;
}

impl RowsAffected for SqliteQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }
}

impl RowsAffected for MySqlQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }
}

impl RowsAffected for PgQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }
}

/// SQL 方言片段生成。
///
/// 调用点通过 `DB::xxx(...)` 静态分发，SQL 主体保留在业务代码中可读，
/// 仅方言相关的"尾巴/前缀"由方言实现生成。
pub trait SqlDialect: Database {
    /// 生成 INSERT 语句的冲突处理（upsert）后缀。
    ///
    /// - `conflict_keys`: 冲突键列（SQLite 的 `ON CONFLICT(...)` 目标；MySQL 忽略）
    /// - `set_cols`: 生成 `col = excluded.col`（SQLite）/ `col = VALUES(col)`（MySQL）
    /// - `extra_set`: 原样拼接的额外 SET 片段（如 `updated_at = CURRENT_TIMESTAMP`；
    ///   其中的 `excluded.col` 引用会被方言适配）
    fn upsert_suffix(conflict_keys: &[&str], set_cols: &[&str], extra_set: &[&str]) -> String;

    /// 将 SQL 片段中的 `excluded.col` 引用适配为方言形态（MySQL 转为 `VALUES(col)`）。
    fn adapt_excluded(sql_fragment: &str) -> String;

    /// INSERT 忽略冲突语句的关键字前缀（不含 INTO）。
    fn insert_ignore_prefix() -> &'static str;

    /// INSERT 忽略冲突语句的末尾片段。
    ///
    /// SQLite/MySQL 的冲突忽略修饰符在 INSERT 前缀，PostgreSQL 使用
    /// `ON CONFLICT DO NOTHING` 后缀。
    fn insert_ignore_suffix() -> &'static str;

    /// 生成按冲突键替换整行的 INSERT 语句。
    ///
    /// SQLite 使用 `INSERT OR REPLACE`，MySQL 使用 `REPLACE`，PostgreSQL 用冲突
    /// 更新且保留原行 id / created_at，避免 REPLACE 的 delete-then-insert 副作用。
    fn replace_sql(table: &str, columns: &[&str], conflict_keys: &[&str]) -> String;

    /// 生成字符串聚合表达式。
    fn string_aggregate(expression: &str, separator: &str) -> String;
}

fn parameter_markers(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

impl SqlDialect for Sqlite {
    fn upsert_suffix(conflict_keys: &[&str], set_cols: &[&str], extra_set: &[&str]) -> String {
        let mut items: Vec<String> = set_cols
            .iter()
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();
        items.extend(extra_set.iter().map(|s| s.to_string()));
        format!("ON CONFLICT({}) DO UPDATE SET {}", conflict_keys.join(", "), items.join(", "))
    }

    fn adapt_excluded(sql_fragment: &str) -> String {
        // SQLite 原生支持 excluded.col
        sql_fragment.to_string()
    }

    fn insert_ignore_prefix() -> &'static str {
        "INSERT OR IGNORE"
    }

    fn insert_ignore_suffix() -> &'static str {
        ""
    }

    fn replace_sql(table: &str, columns: &[&str], _conflict_keys: &[&str]) -> String {
        format!(
            "INSERT OR REPLACE INTO {table} ({}) VALUES ({})",
            columns.join(", "),
            parameter_markers(columns.len()),
        )
    }

    fn string_aggregate(expression: &str, separator: &str) -> String {
        format!("GROUP_CONCAT({expression}, {separator})")
    }
}

impl SqlDialect for MySql {
    fn upsert_suffix(_conflict_keys: &[&str], set_cols: &[&str], extra_set: &[&str]) -> String {
        let mut items: Vec<String> = set_cols
            .iter()
            .map(|c| format!("{c} = VALUES({c})"))
            .collect();
        items.extend(extra_set.iter().map(|s| Self::adapt_excluded(s)));
        format!("ON DUPLICATE KEY UPDATE {}", items.join(", "))
    }

    fn adapt_excluded(sql_fragment: &str) -> String {
        // ponytail: 正则替换 excluded.col → VALUES(col)；覆盖现有 SQL 中的简单引用形态
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(r"\bexcluded\.(\w+)\b").unwrap());
        re.replace_all(sql_fragment, "VALUES($1)").into_owned()
    }

    fn insert_ignore_prefix() -> &'static str {
        "INSERT IGNORE"
    }

    fn insert_ignore_suffix() -> &'static str {
        ""
    }

    fn replace_sql(table: &str, columns: &[&str], _conflict_keys: &[&str]) -> String {
        format!(
            "REPLACE INTO {table} ({}) VALUES ({})",
            columns.join(", "),
            parameter_markers(columns.len()),
        )
    }

    fn string_aggregate(expression: &str, separator: &str) -> String {
        format!("GROUP_CONCAT({expression} SEPARATOR {separator})")
    }
}

impl SqlDialect for Postgres {
    fn upsert_suffix(conflict_keys: &[&str], set_cols: &[&str], extra_set: &[&str]) -> String {
        let mut items: Vec<String> = set_cols
            .iter()
            .map(|c| format!("{c} = excluded.{c}"))
            .collect();
        items.extend(extra_set.iter().map(|s| s.to_string()));
        format!("ON CONFLICT({}) DO UPDATE SET {}", conflict_keys.join(", "), items.join(", "))
    }

    fn adapt_excluded(sql_fragment: &str) -> String {
        sql_fragment.to_string()
    }

    fn insert_ignore_prefix() -> &'static str {
        "INSERT"
    }

    fn insert_ignore_suffix() -> &'static str {
        "ON CONFLICT DO NOTHING"
    }

    fn replace_sql(table: &str, columns: &[&str], conflict_keys: &[&str]) -> String {
        let updates = columns
            .iter()
            .filter(|column| !conflict_keys.contains(column))
            .map(|column| format!("{column} = excluded.{column}"))
            .collect::<Vec<_>>();
        format!(
            "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT({}) DO UPDATE SET {}",
            columns.join(", "),
            parameter_markers(columns.len()),
            conflict_keys.join(", "),
            updates.join(", "),
        )
    }

    fn string_aggregate(expression: &str, separator: &str) -> String {
        format!("STRING_AGG({expression}, {separator})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_upsert_suffix_generates_excluded_form() {
        let sql = <Sqlite as SqlDialect>::upsert_suffix(
            &["cluster_id"],
            &["cpu_usage", "memory_usage"],
            &["updated_at = CURRENT_TIMESTAMP"],
        );
        assert_eq!(
            sql,
            "ON CONFLICT(cluster_id) DO UPDATE SET cpu_usage = excluded.cpu_usage, \
             memory_usage = excluded.memory_usage, updated_at = CURRENT_TIMESTAMP"
        );
    }

    #[test]
    fn mysql_upsert_suffix_generates_values_form() {
        let sql = <MySql as SqlDialect>::upsert_suffix(
            &["cluster_id", "snapshot_date"],
            &["avg_qps"],
            &[],
        );
        assert_eq!(sql, "ON DUPLICATE KEY UPDATE avg_qps = VALUES(avg_qps)");
    }

    #[test]
    fn postgres_upsert_suffix_uses_excluded_form() {
        assert_eq!(
            <Postgres as SqlDialect>::upsert_suffix(&["cluster_id"], &["qps"], &[]),
            "ON CONFLICT(cluster_id) DO UPDATE SET qps = excluded.qps"
        );
    }

    #[test]
    fn insert_ignore_fragments_match_dialect() {
        assert_eq!(<Sqlite as SqlDialect>::insert_ignore_prefix(), "INSERT OR IGNORE");
        assert_eq!(<MySql as SqlDialect>::insert_ignore_prefix(), "INSERT IGNORE");
        assert_eq!(<Postgres as SqlDialect>::insert_ignore_prefix(), "INSERT");
        assert_eq!(<Postgres as SqlDialect>::insert_ignore_suffix(), "ON CONFLICT DO NOTHING");
    }

    #[test]
    fn replace_sql_follows_dialect() {
        let columns = &["cache_key", "scenario", "response_json"];
        let keys = &["cache_key"];
        assert_eq!(
            <Sqlite as SqlDialect>::replace_sql("llm_cache", columns, keys),
            "INSERT OR REPLACE INTO llm_cache (cache_key, scenario, response_json) VALUES (?, ?, ?)"
        );
        assert_eq!(
            <MySql as SqlDialect>::replace_sql("llm_cache", columns, keys),
            "REPLACE INTO llm_cache (cache_key, scenario, response_json) VALUES (?, ?, ?)"
        );
        assert_eq!(
            <Postgres as SqlDialect>::replace_sql("llm_cache", columns, keys),
            "INSERT INTO llm_cache (cache_key, scenario, response_json) VALUES (?, ?, ?) \
             ON CONFLICT(cache_key) DO UPDATE SET scenario = excluded.scenario, \
             response_json = excluded.response_json"
        );
    }

    #[test]
    fn string_aggregate_follows_dialect() {
        assert_eq!(
            <Sqlite as SqlDialect>::string_aggregate("r.code", "','"),
            "GROUP_CONCAT(r.code, ',')"
        );
        assert_eq!(
            <MySql as SqlDialect>::string_aggregate("r.code", "','"),
            "GROUP_CONCAT(r.code SEPARATOR ',')"
        );
        assert_eq!(
            <Postgres as SqlDialect>::string_aggregate("r.code", "','"),
            "STRING_AGG(r.code, ',')"
        );
    }
}
