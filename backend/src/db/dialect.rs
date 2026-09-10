//! SQL 方言差异抽象。
//!
//! 所有"SQLite 与 MySQL 语法不同"的知识都收口在本模块：
//! - `LastInsertId`: 从执行结果读取自增主键
//! - `SqlDialect`: 生成方言化的 upsert / ignore / replace SQL 片段
//!
//! 均以关联函数形式静态分发（`DB::upsert_suffix(...)`），零运行时开销，
//! 业务代码只需一个 `DB: AppDb` 约束即可获得全部能力。

use sqlx::{Database, MySql, Sqlite, mysql::MySqlQueryResult, sqlite::SqliteQueryResult};

/// 从 INSERT 执行结果中读取自增主键 id。
///
/// SQLite 对应 `last_insert_rowid()`，MySQL 对应 `last_insert_id` 字段。
pub trait LastInsertId {
    fn last_insert_id(&self) -> i64;
}

impl LastInsertId for SqliteQueryResult {
    fn last_insert_id(&self) -> i64 {
        self.last_insert_rowid()
    }
}

impl LastInsertId for MySqlQueryResult {
    fn last_insert_id(&self) -> i64 {
        self.last_insert_id() as i64
    }
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
    fn insert_ignore() -> &'static str;

    /// INSERT 替换语义语句的关键字前缀（不含 INTO）。
    fn insert_replace() -> &'static str;
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

    fn insert_ignore() -> &'static str {
        "INSERT OR IGNORE"
    }

    fn insert_replace() -> &'static str {
        "INSERT OR REPLACE"
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

    fn insert_ignore() -> &'static str {
        "INSERT IGNORE"
    }

    fn insert_replace() -> &'static str {
        "REPLACE"
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
    fn insert_prefixes_match_dialect() {
        assert_eq!(<Sqlite as SqlDialect>::insert_ignore(), "INSERT OR IGNORE");
        assert_eq!(<Sqlite as SqlDialect>::insert_replace(), "INSERT OR REPLACE");
        assert_eq!(<MySql as SqlDialect>::insert_ignore(), "INSERT IGNORE");
        assert_eq!(<MySql as SqlDialect>::insert_replace(), "REPLACE");
    }
}
