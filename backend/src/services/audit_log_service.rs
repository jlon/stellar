// Audit Log Service
// Purpose: Query and analyze StarRocks audit logs for access patterns and slow queries
// Design Ref: AUDIT_LOG_FEATURES.md

#![allow(dead_code)]

use crate::config::AuditLogConfig;
use crate::models::Cluster;
use crate::services::cluster_timeout;
use crate::services::{MySQLClient, MySQLPoolManager};
use crate::utils::ApiResult;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utoipa::ToSchema;

static TABLE_REF_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:from|join|into)\s+((?:`?[A-Za-z_][\w$]*`?\.){0,2}`?[A-Za-z_][\w$]*`?)")
        .unwrap_or_else(|_| Regex::new(r"from\s+(\w+)").expect("fallback table regex"))
});

fn row_field<'a>(idx: &HashMap<String, usize>, row: &'a [String], name: &str) -> &'a str {
    idx.get(name)
        .and_then(|&i| row.get(i))
        .map(String::as_str)
        .unwrap_or("")
}

fn is_ignored_table(database: &str, table: &str) -> bool {
    let db = database.to_ascii_lowercase();
    let tbl = table.to_ascii_lowercase();
    matches!(
        db.as_str(),
        "information_schema"
            | "_statistics_"
            | "__internal_schema"
            | "mysql"
            | "starrocks_audit_db__"
    ) || db.ends_with(".information_schema")
        || matches!(
            tbl.as_str(),
            "starrocks_audit_tbl__"
                | "audit_log"
                | "select"
                | "where"
                | "dual"
                | "unnest"
                | "values"
        )
}

pub fn audit_query_user_message(err: &crate::utils::ApiError, audit_table: &str) -> String {
    let raw = err.to_string();
    let msg = raw.to_ascii_lowercase();
    if is_missing_audit_relation(&msg) {
        format!("集群未配置审计日志表 {audit_table}")
    } else if is_audit_permission_denied(&msg) {
        format!("监控账号无权读取审计日志表 {audit_table}")
    } else if msg.contains("timeout") || msg.contains("timed out") {
        "查询审计日志超时".to_string()
    } else if msg.contains("connect") || msg.contains("connection") {
        "无法连接集群查询审计日志".to_string()
    } else {
        "查询审计日志失败".to_string()
    }
}

fn is_missing_audit_relation(msg: &str) -> bool {
    msg.contains("unknown table")
        || msg.contains("unknown database")
        || msg.contains("doesn't exist")
        || msg.contains("does not exist")
        || msg.contains("no matching table")
        || msg.contains("no such table")
        || msg.contains("1146")
        || msg.contains("1049")
}

fn is_audit_permission_denied(msg: &str) -> bool {
    msg.contains("access denied") || msg.contains("1142") || msg.contains("1227")
}

fn extract_table_refs(stmt: &str, fallback_db: &str, catalog: &str) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut tables = Vec::new();
    for caps in TABLE_REF_RE.captures_iter(stmt) {
        let Some(raw) = caps.get(1) else {
            continue;
        };
        let cleaned = raw.as_str().replace('`', "");
        if cleaned.is_empty() || cleaned.contains('(') {
            continue;
        }
        let parts: Vec<&str> = cleaned.split('.').filter(|part| !part.is_empty()).collect();
        let (database, table) = match parts.as_slice() {
            [catalog_name, db_name, table_name] => {
                (format!("{catalog_name}.{db_name}"), (*table_name).to_string())
            },
            [db_name, table_name] => {
                if !catalog.is_empty() && catalog != "default_catalog" {
                    (format!("{catalog}.{db_name}"), (*table_name).to_string())
                } else {
                    ((*db_name).to_string(), (*table_name).to_string())
                }
            },
            [table_name] => {
                let database = if fallback_db.is_empty() {
                    String::new()
                } else if !catalog.is_empty() && catalog != "default_catalog" {
                    format!("{catalog}.{fallback_db}")
                } else {
                    fallback_db.to_string()
                };
                (database, (*table_name).to_string())
            },
            _ => continue,
        };
        if is_ignored_table(&database, &table) {
            continue;
        }
        if seen.insert((database.clone(), table.clone())) {
            tables.push((database, table));
        }
    }
    tables
}

/// Top table by access count (from audit logs)
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TopTableByAccess {
    pub database: String,
    pub table: String,
    pub access_count: i64,
    pub last_access: Option<String>,
    pub unique_users: i32,
}

/// Slow query information
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct SlowQuery {
    pub query_id: String,
    pub user: String,
    pub database: String,
    pub duration_ms: i64,
    pub scan_rows: Option<i64>,
    pub scan_bytes: Option<i64>,
    pub return_rows: Option<i64>,
    pub cpu_cost_ms: Option<i64>,
    pub mem_cost_bytes: Option<i64>,
    pub timestamp: String,
    pub state: String,
    pub query_preview: String, // First 200 characters
}

pub struct AuditLogService {
    mysql_pool_manager: Arc<MySQLPoolManager>,
    audit_config: AuditLogConfig,
}

impl AuditLogService {
    pub fn new(mysql_pool_manager: Arc<MySQLPoolManager>, audit_config: AuditLogConfig) -> Self {
        Self { mysql_pool_manager, audit_config }
    }

    /// Get audit table name and field mappings based on cluster type
    pub fn audit_table_name(&self, cluster: &Cluster) -> String {
        self.get_audit_config(cluster).0
    }

    fn get_audit_config(
        &self,
        cluster: &Cluster,
    ) -> (String, &'static str, &'static str, &'static str, &'static str) {
        use crate::models::cluster::ClusterType;

        match cluster.cluster_type {
            ClusterType::StarRocks => (
                self.audit_config.full_table_name(),
                "timestamp",
                "queryTime",
                "isQuery",
                "queryType",
            ),
            ClusterType::Doris => (
                "__internal_schema.audit_log".to_string(),
                "time",
                "query_time",
                "is_query",
                "stmt_type",
            ),
        }
    }

    /// Get top tables by access count
    ///
    /// This queries the audit log to find the most frequently accessed tables.
    ///
    /// # Arguments
    /// * `cluster` - The StarRocks cluster
    /// * `hours` - Time window in hours (default: 24)
    /// * `limit` - Maximum number of results (default: 20)
    pub async fn get_top_tables_by_access(
        &self,
        cluster: &Cluster,
        hours: i32,
        limit: usize,
    ) -> ApiResult<Vec<TopTableByAccess>> {
        let pool = self.mysql_pool_manager.get_pool(cluster).await?;
        let mysql_client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
        let (audit_table, time_field, _query_time_field, is_query_field, _stmt_type_field) =
            self.get_audit_config(cluster);
        let audit_table_filter = &self.audit_config.table;
        let hours = hours.max(1);
        let limit = limit.max(1);

        let query = format!(
            r#"
            SELECT
                COALESCE(`catalog`, '') as catalog,
                COALESCE(`db`, '') as db_name,
                `stmt`,
                `user`,
                `{time_field}` as last_access
            FROM {audit_table}
            WHERE `{time_field}` >= DATE_SUB(NOW(), INTERVAL {hours} HOUR)
              AND {is_query_field} = 1
              AND `state` = 'EOF'
              AND LOWER(`stmt`) LIKE '% from %'
              AND LOWER(`stmt`) NOT LIKE '%{audit_table_filter}%'
            "#
        );

        tracing::debug!("Querying top tables by access: hours={}, limit={}", hours, limit);

        let (columns, rows) = mysql_client.query_raw(&query).await?;
        let mut col_idx = HashMap::new();
        for (i, col) in columns.iter().enumerate() {
            col_idx.insert(col.to_ascii_lowercase(), i);
        }

        // (digest, sample stmt) -> (count, first seen time, user set)
        type AggregatedRows = HashMap<(String, String), (i64, Option<String>, HashSet<String>)>;
        let mut aggregated: AggregatedRows = HashMap::new();
        for row in rows {
            let stmt = row_field(&col_idx, &row, "stmt");
            if stmt.is_empty() {
                continue;
            }
            let catalog = row_field(&col_idx, &row, "catalog");
            let db_name = row_field(&col_idx, &row, "db_name");
            let user = row_field(&col_idx, &row, "user");
            let last_access = row_field(&col_idx, &row, "last_access");
            for (database, table) in extract_table_refs(stmt, db_name, catalog) {
                let entry = aggregated
                    .entry((database, table))
                    .or_insert_with(|| (0, None, HashSet::new()));
                entry.0 += 1;
                if !last_access.is_empty()
                    && entry
                        .1
                        .as_deref()
                        .map(|prev| last_access > prev)
                        .unwrap_or(true)
                {
                    entry.1 = Some(last_access.to_string());
                }
                if !user.is_empty() {
                    entry.2.insert(user.to_string());
                }
            }
        }

        let mut tables: Vec<TopTableByAccess> = aggregated
            .into_iter()
            .map(|((database, table), (access_count, last_access, users))| TopTableByAccess {
                database,
                table,
                access_count,
                last_access,
                unique_users: users.len() as i32,
            })
            .collect();
        tables.sort_by(|a, b| b.access_count.cmp(&a.access_count));
        tables.truncate(limit);

        tracing::info!("Found {} top tables by access ({}h window)", tables.len(), hours);

        Ok(tables)
    }

    /// Get slow queries
    ///
    /// This queries the audit log to find slow-running queries.
    ///
    /// # Arguments
    /// * `cluster` - The StarRocks cluster
    /// * `hours` - Time window in hours (default: 24)
    /// * `min_duration_ms` - Minimum query duration in milliseconds (default: 1000)
    /// * `limit` - Maximum number of results (default: 20)
    ///
    /// 取某次查询的完整 SQL 与所属库。
    pub async fn get_query_sql(
        &self,
        cluster: &Cluster,
        query_id: &str,
    ) -> ApiResult<(String, String)> {
        use crate::models::cluster::ClusterType;
        use crate::utils::ApiError;
        if query_id.is_empty() || !query_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err(ApiError::invalid_data("query_id 格式非法"));
        }
        let (audit_table, time_field, _, _, _) = self.get_audit_config(cluster);
        let id_col = match cluster.cluster_type {
            ClusterType::StarRocks => "queryId",
            ClusterType::Doris => "query_id",
        };
        let pool = self.mysql_pool_manager.get_pool(cluster).await?;
        let mysql_client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
        let sql = format!(
            "SELECT `stmt`, COALESCE(`db`, '') AS `database` FROM {audit_table} \
             WHERE `{id_col}` = '{query_id}' ORDER BY `{time_field}` DESC LIMIT 1",
        );
        let (columns, rows) = mysql_client.query_raw(&sql).await?;
        let pos = |name: &str| columns.iter().position(|c| c == name);
        let stmt = pos("stmt")
            .and_then(|i| rows.first()?.get(i))
            .cloned()
            .unwrap_or_default();
        let db = pos("database")
            .and_then(|i| rows.first()?.get(i))
            .cloned()
            .unwrap_or_default();
        if stmt.trim().is_empty() {
            return Err(ApiError::not_found(format!(
                "审计日志中找不到该查询的 SQL（query_id={}，可能已超出保留期）",
                query_id
            )));
        }
        Ok((stmt, db))
    }

    pub async fn get_slow_queries(
        &self,
        cluster: &Cluster,
        hours: i32,
        min_duration_ms: i64,
        limit: usize,
    ) -> ApiResult<Vec<SlowQuery>> {
        let pool = self.mysql_pool_manager.get_pool(cluster).await?;
        let mysql_client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
        let (audit_table, time_field, query_time_field, is_query_field, _stmt_type_field) =
            self.get_audit_config(cluster);

        use crate::models::cluster::ClusterType;

        let query = match cluster.cluster_type {
            ClusterType::StarRocks => format!(
                r#"
            SELECT 
                queryId as query_id,
                `user`,
                COALESCE(`db`, '') as `database`,
                `queryTime` as duration_ms,
                `scanRows` as scan_rows,
                `scanBytes` as scan_bytes,
                `returnRows` as return_rows,
                `cpuCostNs` / 1000000 as cpu_cost_ms,
                `memCostBytes` as mem_cost_bytes,
                `timestamp`,
                `state`,
                LEFT(`stmt`, 200) as query_preview
            FROM {audit_table}
                WHERE `{time_field}` >= DATE_SUB(NOW(), INTERVAL {hours} HOUR)
                    AND `{query_time_field}` >= {min_duration_ms}
                    AND {is_query_field} = 1
                    AND `state` = 'EOF'
                ORDER BY `{query_time_field}` DESC
                LIMIT {limit}
                "#,
            ),
            ClusterType::Doris => format!(
                r#"
                SELECT 
                    query_id,
                    `user`,
                    COALESCE(`db`, '') as `database`,
                    `query_time` as duration_ms,
                    `scan_rows`,
                    `scan_bytes`,
                    `return_rows`,
                    `cpu_time_ms` as cpu_cost_ms,
                    `peak_memory_bytes` as mem_cost_bytes,
                    `time` as timestamp,
                    `state`,
                    LEFT(`stmt`, 200) as query_preview
                FROM {audit_table}
                WHERE `{time_field}` >= DATE_SUB(NOW(), INTERVAL {hours} HOUR)
                    AND `{query_time_field}` >= {min_duration_ms}
                    AND {is_query_field} = 1
                AND `state` = 'EOF'
                ORDER BY `{query_time_field}` DESC
            LIMIT {limit}
            "#,
            ),
        };

        tracing::debug!(
            "Querying slow queries: hours={}, min_duration={}ms, limit={}",
            hours,
            min_duration_ms,
            limit
        );

        let (columns, rows) = mysql_client.query_raw(&query).await?;

        let mut col_idx = std::collections::HashMap::new();
        for (i, col) in columns.iter().enumerate() {
            col_idx.insert(col.clone(), i);
        }

        let mut slow_queries = Vec::new();
        for row in rows {
            if let (Some(query_id), Some(user), Some(database), Some(duration_ms_str)) = (
                col_idx.get("query_id").and_then(|&i| row.get(i)),
                col_idx.get("user").and_then(|&i| row.get(i)),
                col_idx.get("database").and_then(|&i| row.get(i)),
                col_idx.get("duration_ms").and_then(|&i| row.get(i)),
            ) {
                let duration_ms = duration_ms_str.parse::<i64>().unwrap_or(0);

                let scan_rows = col_idx
                    .get("scan_rows")
                    .and_then(|&i| row.get(i))
                    .and_then(|s| s.parse::<i64>().ok());

                let scan_bytes = col_idx
                    .get("scan_bytes")
                    .and_then(|&i| row.get(i))
                    .and_then(|s| s.parse::<i64>().ok());

                let return_rows = col_idx
                    .get("return_rows")
                    .and_then(|&i| row.get(i))
                    .and_then(|s| s.parse::<i64>().ok());

                let cpu_cost_ms = col_idx
                    .get("cpu_cost_ms")
                    .and_then(|&i| row.get(i))
                    .and_then(|s| s.parse::<i64>().ok());

                let mem_cost_bytes = col_idx
                    .get("mem_cost_bytes")
                    .and_then(|&i| row.get(i))
                    .and_then(|s| s.parse::<i64>().ok());

                let timestamp = col_idx
                    .get("timestamp")
                    .and_then(|&i| row.get(i))
                    .cloned()
                    .unwrap_or_default();

                let state = col_idx
                    .get("state")
                    .and_then(|&i| row.get(i))
                    .cloned()
                    .unwrap_or_else(|| "UNKNOWN".to_string());

                let query_preview = col_idx
                    .get("query_preview")
                    .and_then(|&i| row.get(i))
                    .cloned()
                    .unwrap_or_default();

                slow_queries.push(SlowQuery {
                    query_id: query_id.to_string(),
                    user: user.to_string(),
                    database: database.to_string(),
                    duration_ms,
                    scan_rows,
                    scan_bytes,
                    return_rows,
                    cpu_cost_ms,
                    mem_cost_bytes,
                    timestamp,
                    state,
                    query_preview,
                });
            }
        }

        tracing::info!(
            "Found {} slow queries (>{}ms, {}h window)",
            slow_queries.len(),
            min_duration_ms,
            hours
        );

        Ok(slow_queries)
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_table_refs, is_ignored_table};

    #[test]
    fn extract_two_part_table() {
        let tables = extract_table_refs(
            "select * from ieg_dwd.st_game_detail_assess order by update_time desc limit 1000",
            "",
            "default_catalog",
        );
        assert_eq!(tables, vec![("ieg_dwd".to_string(), "st_game_detail_assess".to_string())]);
    }

    #[test]
    fn extract_quoted_three_part_table() {
        let tables = extract_table_refs(
            "select * from `iceberg`.`ad_gl`.`dwd_ads_dw_all_cnvt_bjht_rt_inc_h` where dayno >= 20260907",
            "",
            "hive",
        );
        assert_eq!(
            tables,
            vec![("iceberg.ad_gl".to_string(), "dwd_ads_dw_all_cnvt_bjht_rt_inc_h".to_string())]
        );
    }

    #[test]
    fn extract_quoted_db_table_and_join() {
        let tables = extract_table_refs(
            "SELECT COUNT(*) FROM `ib_nebula`.`dwd_meituan_launch_from_all_version_inc_d` JOIN other_db.dim_user u",
            "ib_nebula",
            "default_catalog",
        );
        assert_eq!(
            tables,
            vec![
                ("ib_nebula".to_string(), "dwd_meituan_launch_from_all_version_inc_d".to_string()),
                ("other_db".to_string(), "dim_user".to_string()),
            ]
        );
    }

    #[test]
    fn extract_skips_heartbeat_and_information_schema() {
        assert!(extract_table_refs("select @@version_comment limit 1", "", "").is_empty());
        assert!(extract_table_refs("select * from information_schema.tables", "", "").is_empty());
        assert!(is_ignored_table("information_schema", "tables"));
    }

    #[test]
    fn extract_uses_fallback_db_for_single_name() {
        let tables = extract_table_refs("select * from fact_orders where dt=1", "sales", "");
        assert_eq!(tables, vec![("sales".to_string(), "fact_orders".to_string())]);
    }

    #[test]
    fn audit_error_missing_table_is_explicit() {
        let err = crate::utils::ApiError::internal_error(
            "SQL execution failed: Getting analyzing error. Detail message: Unknown table 'starrocks_audit_db__.definitely_missing_audit_tbl'.",
        );
        assert_eq!(
            super::audit_query_user_message(&err, "starrocks_audit_db__.starrocks_audit_tbl__"),
            "集群未配置审计日志表 starrocks_audit_db__.starrocks_audit_tbl__"
        );
    }

    #[test]
    fn audit_error_unknown_database_is_explicit() {
        let err = crate::utils::ApiError::internal_error(
            "Getting analyzing error. Detail message: Unknown database 'starrocks_audit_db__'",
        );
        assert_eq!(
            super::audit_query_user_message(&err, "starrocks_audit_db__.starrocks_audit_tbl__"),
            "集群未配置审计日志表 starrocks_audit_db__.starrocks_audit_tbl__"
        );
    }

    #[test]
    fn audit_error_denied_and_timeout() {
        let denied = crate::utils::ApiError::internal_error(
            "Access denied; you need the SELECT privilege(s)",
        );
        assert_eq!(super::audit_query_user_message(&denied, "t"), "监控账号无权读取审计日志表 t");
        let timeout = crate::utils::ApiError::cluster_connection_failed("connection timed out");
        assert_eq!(super::audit_query_user_message(&timeout, "t"), "查询审计日志超时");
    }
}
