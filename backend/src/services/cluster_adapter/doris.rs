// Doris Adapter
// Purpose: Implement ClusterAdapter trait for Apache Doris clusters
// Reference: https://doris.apache.org/zh-CN/docs/4.x/gettingStarted/quick-start

use super::ClusterAdapter;
use crate::models::{
    Backend, Cluster, ClusterType, DependencyEvidence, DependencyObject, DependencySource,
    Frontend, MaterializedView, MaterializedViewDependencies, MaterializedViewDependency,
    MaterializedViewKind, MaterializedViewRef, MaterializedViewState, Query,
    RefreshMaterializedViewRequest, RefreshMode, RefreshSchedule, RelationKind, RuntimeInfo,
};
use crate::services::{MySQLClient, MySQLPoolManager};
use crate::utils::{ApiError, ApiResult};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub struct DorisAdapter {
    pub http_client: Client,
    pub cluster: Cluster,
    mysql_pool_manager: Arc<MySQLPoolManager>,
}

impl DorisAdapter {
    pub fn new(cluster: Cluster, mysql_pool_manager: Arc<MySQLPoolManager>) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(cluster.connection_timeout as u64))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!(
                    "Failed to build HTTP client for Doris cluster {}: {}",
                    cluster.name,
                    e
                );
                Client::default()
            });

        Self { http_client, cluster, mysql_pool_manager }
    }

    async fn mysql_client(&self) -> ApiResult<MySQLClient> {
        let pool = self.mysql_pool_manager.get_pool(&self.cluster).await?;
        Ok(MySQLClient::from_pool(pool).with_timeout(std::time::Duration::from_secs(
            self.cluster.connection_timeout.max(1) as u64,
        )))
    }

    /// 折中实现：聚合所有数据库的 Load 错误信息
    /// 替代 StarRocks 的 SHOW PROC '/load_error_hub'
    async fn get_load_errors_compromise(&self) -> ApiResult<Vec<Value>> {
        use serde_json::json;

        let mysql_client = self.mysql_client().await?;

        let system_dbs = [
            "information_schema",
            "_statistics_",
            "starrocks_audit_db__",
            "__internal_schema",
            "sys",
            "mysql",
        ];
        let (_, db_rows) = mysql_client.query_raw("SHOW DATABASES").await?;

        let mut all_errors = Vec::new();

        for db_row in db_rows {
            if let Some(db_name) = db_row.first() {
                let db_name_lower = db_name.to_lowercase();
                if system_dbs.contains(&db_name_lower.as_str()) {
                    continue;
                }

                let sql = format!("SHOW LOAD FROM `{}` WHERE State = 'CANCELLED'", db_name);
                match mysql_client.query_raw(&sql).await {
                    Ok((columns, rows)) => {
                        for row in rows {
                            let mut error_obj = serde_json::Map::new();
                            error_obj.insert("Database".to_string(), json!(db_name));

                            for (i, col) in columns.iter().enumerate() {
                                if let Some(value) = row.get(i) {
                                    let field_name = match col.as_str() {
                                        "JobId" => "JobId",
                                        "Label" => "Label",
                                        "State" => "State",
                                        "Progress" => "Progress",
                                        "Type" => "Type",
                                        "Priority" => "Priority",
                                        "ScanRows" => "ScanRows",
                                        "ScanBytes" => "ScanBytes",
                                        "LoadRows" => "LoadRows",
                                        "LoadBytes" => "LoadBytes",
                                        "EtlInfo" => "EtlInfo",
                                        "TaskInfo" => "TaskInfo",
                                        "ErrorMsg" => "ErrorMsg",
                                        "CreateTime" => "CreateTime",
                                        "EtlStartTime" => "EtlStartTime",
                                        "EtlFinishTime" => "EtlFinishTime",
                                        "LoadStartTime" => "LoadStartTime",
                                        "LoadFinishTime" => "LoadFinishTime",
                                        "URL" => "URL",
                                        "JobDetails" => "JobDetails",
                                        _ => col.as_str(),
                                    };
                                    error_obj.insert(field_name.to_string(), json!(value));
                                }
                            }

                            all_errors.push(serde_json::Value::Object(error_obj));
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            "[Doris] Failed to query load errors from database {}: {:?}",
                            db_name,
                            e
                        );
                    },
                }
            }
        }

        tracing::info!("[Doris] Aggregated {} load errors from all databases", all_errors.len());
        Ok(all_errors)
    }

    fn is_user_database(database: &str) -> bool {
        !database.starts_with("__") && !matches!(database, "information_schema" | "mysql" | "sys")
    }

    fn row_string(row: &Value, key: &str) -> Option<String> {
        row.get(key).and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        })
    }

    async fn list_async_materialized_views(
        &self,
        mysql_client: &MySQLClient,
        database: &str,
    ) -> ApiResult<Vec<MaterializedView>> {
        MaterializedViewRef::validate_identifier("database", database)?;
        let sql = format!(
            "SELECT Id AS id, Name AS name, State AS state, RefreshState AS refresh_state, \
             RefreshInfo AS refresh_info, QuerySql AS definition, MvPartitionInfo AS partition_info \
             FROM mv_infos(\"database\" = \"{database}\")"
        );
        let rows = mysql_client.query(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let name = Self::row_string(&row, "name")?;
                Some(MaterializedView {
                    id: Self::row_string(&row, "id")
                        .map(|id| format!("async:{database}:{id}"))
                        .unwrap_or_else(|| format!("async:{database}:{name}")),
                    name,
                    database_name: database.to_string(),
                    kind: MaterializedViewKind::Async,
                    refresh_type: Self::row_string(&row, "refresh_info")
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| "ASYNC".to_string()),
                    is_active: !Self::row_string(&row, "state")
                        .is_some_and(|state| state.eq_ignore_ascii_case("PAUSED")),
                    partition_type: Self::row_string(&row, "partition_info"),
                    task_id: None,
                    task_name: None,
                    last_refresh_start_time: None,
                    last_refresh_finished_time: None,
                    last_refresh_duration: None,
                    last_refresh_state: Self::row_string(&row, "refresh_state"),
                    rows: None,
                    definition: Self::row_string(&row, "definition").unwrap_or_default(),
                })
            })
            .collect())
    }

    /// Doris exposes rollup history directly. One query per database avoids a
    /// table scan, `DESC ... ALL` fan-out, and the former `COUNT(*)` scans.
    async fn list_rollups(
        &self,
        mysql_client: &MySQLClient,
        database: &str,
    ) -> ApiResult<Vec<MaterializedView>> {
        MaterializedViewRef::validate_identifier("database", database)?;
        let rows = mysql_client
            .query(&format!("SHOW ALTER TABLE ROLLUP FROM `{database}`"))
            .await?;
        let rollups: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                if !Self::is_finished_rollup_state(Self::row_string(&row, "State").as_deref()) {
                    return None;
                }
                let name = Self::row_string(&row, "RollupIndexName")?;
                let table = Self::row_string(&row, "TableName")?;
                Some(MaterializedView {
                    id: format!("rollup:{database}:{table}:{name}"),
                    name,
                    database_name: database.to_string(),
                    kind: MaterializedViewKind::Rollup,
                    refresh_type: "ROLLUP".to_string(),
                    is_active: true,
                    partition_type: None,
                    task_id: None,
                    task_name: None,
                    last_refresh_start_time: Self::row_string(&row, "CreateTime"),
                    last_refresh_finished_time: Self::row_string(&row, "FinishedTime"),
                    last_refresh_duration: None,
                    last_refresh_state: Some("FINISHED".to_string()),
                    rows: None,
                    definition: format!(
                        "-- ROLLUP materialized view on table `{database}`.`{table}`"
                    ),
                })
            })
            .collect();
        let candidate_count = rollups.len();
        let unique_rollups = Self::unique_rollups(rollups);
        if unique_rollups.len() != candidate_count {
            tracing::warn!(
                database,
                skipped = candidate_count - unique_rollups.len(),
                "Skipping ambiguous Doris ROLLUP names from alter-job metadata"
            );
        }
        Ok(unique_rollups)
    }

    pub(crate) fn is_finished_rollup_state(state: Option<&str>) -> bool {
        state.is_some_and(|state| state.eq_ignore_ascii_case("FINISHED"))
    }

    pub(crate) fn unique_rollups(rollups: Vec<MaterializedView>) -> Vec<MaterializedView> {
        let mut counts = HashMap::<String, usize>::new();
        for rollup in &rollups {
            *counts.entry(rollup.name.clone()).or_default() += 1;
        }
        rollups
            .into_iter()
            .filter(|rollup| counts.get(&rollup.name) == Some(&1))
            .collect()
    }

    async fn find_rollup_parent(
        &self,
        mysql_client: &MySQLClient,
        reference: &MaterializedViewRef,
    ) -> ApiResult<String> {
        let materialized_view = self
            .list_rollups(mysql_client, &reference.database)
            .await?
            .into_iter()
            .find(|materialized_view| materialized_view.name == reference.name)
            .ok_or_else(|| ApiError::not_found(reference.display_name()))?;
        Self::rollup_parent_from_definition(&materialized_view.definition, reference)
    }

    fn rollup_parent_from_definition(
        definition: &str,
        reference: &MaterializedViewRef,
    ) -> ApiResult<String> {
        let parent = definition
            .strip_prefix("-- ROLLUP materialized view on table `")
            .and_then(|value| value.rsplit_once("`.`"))
            .map(|(_, table)| table.trim_end_matches('`').to_string())
            .ok_or_else(|| ApiError::not_found(reference.display_name()))?;
        MaterializedViewRef::validate_identifier("ROLLUP parent", &parent)?;
        Ok(parent)
    }

    async fn ensure_current_rollup(
        &self,
        mysql_client: &MySQLClient,
        reference: &MaterializedViewRef,
        parent: &str,
    ) -> ApiResult<()> {
        let rows = mysql_client
            .query(&format!("SHOW DATA FROM `{}`.`{parent}`", reference.database))
            .await?;
        if !Self::has_current_rollup(&rows, &reference.name) {
            return Err(ApiError::not_found(reference.display_name()));
        }
        Ok(())
    }

    /// `SHOW ALTER TABLE ROLLUP` is historical. Before using its parent table
    /// for a read result or a write, confirm the ROLLUP still exists in the
    /// current table metadata.
    async fn current_rollup_parent(
        &self,
        mysql_client: &MySQLClient,
        reference: &MaterializedViewRef,
    ) -> ApiResult<String> {
        let parent = self.find_rollup_parent(mysql_client, reference).await?;
        self.ensure_current_rollup(mysql_client, reference, &parent)
            .await?;
        Ok(parent)
    }

    pub(crate) fn has_current_rollup(rows: &[Value], rollup_name: &str) -> bool {
        rows.iter()
            .any(|row| Self::row_string(row, "IndexName").as_deref() == Some(rollup_name))
    }

    async fn get_materialized_view_exact(
        &self,
        mysql_client: &MySQLClient,
        reference: &MaterializedViewRef,
    ) -> ApiResult<MaterializedView> {
        reference.validate()?;
        let materialized_views = match reference.kind {
            MaterializedViewKind::Async => {
                self.list_async_materialized_views(mysql_client, &reference.database)
                    .await?
            },
            MaterializedViewKind::Rollup => {
                self.list_rollups(mysql_client, &reference.database).await?
            },
        };
        let materialized_view = materialized_views
            .into_iter()
            .find(|materialized_view| materialized_view.name == reference.name)
            .ok_or_else(|| ApiError::not_found(reference.display_name()))?;
        if reference.kind == MaterializedViewKind::Rollup {
            let parent =
                Self::rollup_parent_from_definition(&materialized_view.definition, reference)?;
            self.ensure_current_rollup(mysql_client, reference, &parent)
                .await?;
        }
        Ok(materialized_view)
    }

    /// This is deliberately a bounded, conservative extractor rather than a
    /// SQL parser. It recognizes unambiguous FROM/JOIN identifiers and excludes
    /// CTE aliases; all results remain partial because Doris definitions can
    /// include external catalogs and dialect constructs we do not model here.
    pub(crate) fn extract_doris_dependencies(
        definition: &str,
        default_database: &str,
    ) -> (Vec<MaterializedViewDependency>, Vec<String>) {
        const MAX_DEFINITION_BYTES: usize = 100_000;
        const MAX_DEPENDENCIES: usize = 64;
        static CTE_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?is)(?:\bwith|,)\s*(`[^`]+`|[a-z_][a-z0-9_]*)\s*(?:\([^)]*\))?\s+as\s*\(")
                .expect("CTE regex is valid")
        });
        static SOURCE_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?is)\b(?:from|join)\s+((?:`[^`]+`|[a-z_][a-z0-9_]*)(?:\s*\.\s*(?:`[^`]+`|[a-z_][a-z0-9_]*)){0,2})")
                .expect("source regex is valid")
        });
        static COMMENT_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"(?s)/\*.*?\*/|--[^\r\n]*").expect("comment regex is valid"));
        static NESTED_SOURCE_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?is)\b(?:from|join)\s*\(").expect("nested source regex is valid")
        });

        let mut warnings = Vec::new();
        if definition.len() > MAX_DEFINITION_BYTES {
            warnings.push("Doris definition exceeds the dependency parser limit; no inferred dependencies were returned.".to_string());
            return (Vec::new(), warnings);
        }
        let definition_without_comments = COMMENT_RE.replace_all(definition, " ");
        if definition_without_comments.len() != definition.len() {
            warnings.push("Comments were ignored while extracting Doris dependencies.".to_string());
        }
        if NESTED_SOURCE_RE.is_match(&definition_without_comments) {
            warnings.push("Nested query sources are not expanded into dependencies.".to_string());
        }
        let cte_names: HashSet<String> = CTE_RE
            .captures_iter(&definition_without_comments)
            .filter_map(|capture| {
                capture
                    .get(1)
                    .map(|value| Self::unquote_identifier(value.as_str()))
            })
            .collect();
        let mut dependencies = Vec::new();
        let mut seen = HashSet::new();
        for capture in SOURCE_RE.captures_iter(&definition_without_comments) {
            let Some(value) = capture.get(1) else {
                continue;
            };
            let parts: Vec<String> = value
                .as_str()
                .split('.')
                .map(|part| Self::unquote_identifier(part.trim()))
                .collect();
            if parts.len() == 1 && cte_names.contains(&parts[0]) {
                continue;
            }
            let (catalog, database, name) = match parts.as_slice() {
                [name] => (None, Some(default_database.to_string()), name.clone()),
                [database, name] => (None, Some(database.clone()), name.clone()),
                [catalog, database, name] => {
                    warnings.push(format!("External or qualified source `{}` is shown without confirming its object type.", value.as_str()));
                    (Some(catalog.clone()), Some(database.clone()), name.clone())
                },
                _ => continue,
            };
            let identity = format!(
                "{}:{}:{}",
                catalog.as_deref().unwrap_or_default(),
                database.as_deref().unwrap_or_default(),
                name
            );
            if !seen.insert(identity) {
                continue;
            }
            dependencies.push(MaterializedViewDependency {
                object: DependencyObject { catalog, database, name, kind: RelationKind::Unknown },
                evidence: DependencyEvidence::Partial,
                source: DependencySource::DorisDefinition,
                evidence_snippet: Some(value.as_str().to_string()),
            });
            if dependencies.len() == MAX_DEPENDENCIES {
                warnings.push(
                    "Dependency result reached the parser limit; remaining sources were omitted."
                        .to_string(),
                );
                break;
            }
        }
        (dependencies, warnings)
    }

    fn unquote_identifier(value: &str) -> String {
        value.trim().trim_matches('`').replace("``", "`")
    }

    /// Helper to get string value from JSON
    fn get_str(row: &Value, key: &str) -> String {
        row.get(key)
            .map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => v.to_string().trim_matches('"').to_string(),
            })
            .unwrap_or_else(|| "0".to_string())
    }

    /// Parse Doris SHOW BACKENDS result to Backend struct
    /// Doris SHOW BACKENDS columns (similar to StarRocks but with slight differences):
    /// BackendId, Host, HeartbeatPort, BePort, HttpPort, BrpcPort, LastStartTime, LastHeartbeat,
    /// Alive, SystemDecommissioned, TabletNum, DataUsedCapacity, TrashUsedCapacity, AvailCapacity,
    /// TotalCapacity, UsedPct, MaxDiskUsedPct, RemoteUsedCapacity, Tag, ErrMsg, Version, Status
    fn parse_backend_row(row: &Value) -> Option<Backend> {
        Some(Backend {
            backend_id: Self::get_str(row, "BackendId"),
            host: Self::get_str(row, "Host"),
            heartbeat_port: Self::get_str(row, "HeartbeatPort"),
            be_port: Self::get_str(row, "BePort"),
            http_port: Self::get_str(row, "HttpPort"),
            brpc_port: Self::get_str(row, "BrpcPort"),
            last_start_time: Self::get_str(row, "LastStartTime"),
            last_heartbeat: Self::get_str(row, "LastHeartbeat"),
            alive: Self::get_str(row, "Alive"),
            system_decommissioned: Self::get_str(row, "SystemDecommissioned"),
            cluster_decommissioned: "false".to_string(),
            tablet_num: Self::get_str(row, "TabletNum"),
            data_used_capacity: Self::get_str(row, "DataUsedCapacity"),
            avail_capacity: Self::get_str(row, "AvailCapacity"),
            total_capacity: Self::get_str(row, "TotalCapacity"),
            used_pct: Self::get_str(row, "UsedPct"),
            max_disk_used_pct: Self::get_str(row, "MaxDiskUsedPct"),
            err_msg: Self::get_str(row, "ErrMsg"),
            version: Self::get_str(row, "Version"),
            status: Self::get_str(row, "Status"),

            data_total_capacity: "0".to_string(),
            data_used_pct: "0".to_string(),
            cpu_cores: Self::get_str(row, "CpuCores"),
            mem_limit: "0".to_string(),
            num_running_queries: "0".to_string(),
            mem_used_pct: "0".to_string(),
            cpu_used_pct: "0".to_string(),
            data_cache_metrics: "".to_string(),
            location: Self::get_str(row, "Tag"),
            status_code: "0".to_string(),
            has_storage_path: "".to_string(),
            starlet_port: "0".to_string(),
            worker_id: "0".to_string(),
            warehouse_name: "".to_string(),
        })
    }

    /// Parse Doris SHOW FRONTENDS result to Frontend struct
    /// Doris SHOW FRONTENDS columns:
    /// Name, Host, EditLogPort, HttpPort, QueryPort, RpcPort, Role, IsMaster, ClusterId, Join, Alive,
    /// ReplayedJournalId, LastHeartbeat, IsHelper, ErrMsg, Version, CurrentConnected
    fn parse_frontend_row(row: &Value) -> Option<Frontend> {
        Some(Frontend {
            name: Self::get_str(row, "Name"),
            host: Self::get_str(row, "Host"),
            edit_log_port: Self::get_str(row, "EditLogPort"),
            http_port: Self::get_str(row, "HttpPort"),
            query_port: Self::get_str(row, "QueryPort"),
            rpc_port: Self::get_str(row, "RpcPort"),
            role: Self::get_str(row, "Role"),
            is_master: Some(Self::get_str(row, "IsMaster")),
            cluster_id: Self::get_str(row, "ClusterId"),
            join: Self::get_str(row, "Join"),
            alive: Self::get_str(row, "Alive"),
            replayed_journal_id: Self::get_str(row, "ReplayedJournalId"),
            last_heartbeat: Self::get_str(row, "LastHeartbeat"),
            err_msg: Self::get_str(row, "ErrMsg"),
            version: Self::get_str(row, "Version"),
            is_helper: Some(Self::get_str(row, "IsHelper")),
            start_time: None,
        })
    }

    /// Parse Doris SHOW PROCESSLIST result to Query struct
    /// Doris SHOW PROCESSLIST columns:
    /// CurrentConnected, Id, User, Host, LoginTime, Catalog, Db, Command, Time, State, QueryId, Info
    fn parse_query_row(row: &Value) -> Option<Query> {
        let query_id = Self::get_str(row, "QueryId");
        let connection_id = Self::get_str(row, "Id");

        Some(Query {
            query_id: if query_id.is_empty() { connection_id.clone() } else { query_id },
            connection_id,
            database: Self::get_str(row, "Db"),
            user: Self::get_str(row, "User"),
            scan_bytes: "0".to_string(),
            process_rows: "0".to_string(),
            cpu_time: "0".to_string(),
            exec_time: Self::get_str(row, "Time"),
            sql: Self::get_str(row, "Info"),
            start_time: Some(Self::get_str(row, "LoginTime")),
            fe_ip: None,
            memory_usage: None,
            disk_spill_size: None,
            exec_progress: None,
            warehouse: None,
            custom_query_id: None,
            resource_group: None,
        })
    }

    // ========================================
    // Permission Management Helper Methods for Doris
    // ========================================

    /// Add _PRIV suffix to permissions for Doris
    fn add_priv_suffix(permissions: &[&str]) -> Vec<String> {
        permissions
            .iter()
            .map(|p| if p.ends_with("_PRIV") { p.to_string() } else { format!("{}_PRIV", p) })
            .collect()
    }

    /// Build resource path for Doris
    /// Supports:
    /// - catalog level: CATALOG catalog_name or ALL CATALOGS  
    /// - database level: database.* or catalog.database.* or *.* (all databases)
    /// - table level: database.table or catalog.database.table or database.* (all tables)
    fn build_resource_path(
        resource_type: &str,
        catalog: Option<&str>,
        database: &str,
        table: Option<&str>,
    ) -> String {
        match resource_type.to_uppercase().as_str() {
            "CATALOG" => {
                // Catalog level permissions
                let catalog = catalog.unwrap_or(database);
                if catalog == "*" {
                    "ALL CATALOGS".to_string()
                } else {
                    format!("CATALOG {}", catalog)
                }
            },
            "TABLE" => {
                // Table level permissions
                if database == "*" {
                    // All databases, all tables
                    catalog.map_or_else(|| "*.*".to_string(), |catalog| format!("{catalog}.*.*"))
                } else if let Some(table_name) = table {
                    if table_name == "*" {
                        // Specific database, all tables
                        catalog.map_or_else(
                            || format!("{database}.*"),
                            |catalog| format!("{catalog}.{database}.*"),
                        )
                    } else {
                        // Specific database and table
                        catalog.map_or_else(
                            || format!("{database}.{table_name}"),
                            |catalog| format!("{catalog}.{database}.{table_name}"),
                        )
                    }
                } else {
                    // No table specified, default to all tables in database
                    catalog.map_or_else(
                        || format!("{database}.*"),
                        |catalog| format!("{catalog}.{database}.*"),
                    )
                }
            },
            _ => {
                // Database level permissions (default)
                if database == "*" {
                    catalog.map_or_else(|| "*.*".to_string(), |catalog| format!("{catalog}.*.*"))
                } else {
                    catalog.map_or_else(
                        || format!("{database}.*"),
                        |catalog| format!("{catalog}.{database}.*"),
                    )
                }
            },
        }
    }

    /// Parse a GRANT statement into structured permission data
    #[allow(dead_code)]
    fn parse_grant_statement(statement: &str) -> Option<DorisParsedGrant> {
        let statement = statement.trim();

        // Check if it's a role grant: GRANT 'role_name' TO 'user'@'%'
        if statement.starts_with("GRANT '") || statement.starts_with("GRANT \"") {
            // Role grant
            let role_start = 7; // After "GRANT '"
            if let Some(role_end) = statement[role_start..]
                .find('\'')
                .or_else(|| statement[role_start..].find('"'))
            {
                let role_name = &statement[role_start..role_start + role_end];
                return Some(DorisParsedGrant {
                    privileges: vec!["ROLE".to_string()],
                    resource_type: "ROLE".to_string(),
                    resource_path: role_name.to_string(),
                    granted_role: Some(role_name.to_string()),
                });
            }
        }

        // Regular privilege grant: GRANT privileges ON resource TO user
        if !statement.starts_with("GRANT ") {
            return None;
        }

        // Find "ON" keyword
        let on_pos = statement.find(" ON ")?;
        let privileges_str = &statement[6..on_pos]; // After "GRANT "

        // Find "TO" keyword
        let to_pos = statement.find(" TO ")?;
        let resource_str = &statement[on_pos + 4..to_pos]; // After " ON "

        // Parse privileges (Doris uses _priv suffix like Select_priv, Load_priv)
        let privileges: Vec<String> = privileges_str
            .split(',')
            .map(|s| {
                let trimmed = s.trim().to_uppercase();
                // Remove _PRIV suffix if present
                trimmed.trim_end_matches("_PRIV").to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        // Parse resource (e.g., "db_name.*", "db_name.table_name", "*.*")
        let resource_path = resource_str.trim().to_string();
        let resource_type = if resource_path == "*.*" || resource_path == "*.*.*" {
            "GLOBAL".to_string()
        } else if resource_path.ends_with(".*") || resource_path.ends_with(".*.*") {
            "DATABASE".to_string()
        } else if resource_path.contains('.') {
            "TABLE".to_string()
        } else {
            "CATALOG".to_string()
        };

        Some(DorisParsedGrant { privileges, resource_type, resource_path, granted_role: None })
    }

    /// Parse Doris resource privileges format: "resource_path: Priv1, Priv2; resource_path2: Priv3"
    fn parse_doris_resource_privs(
        privs_str: &str,
        resource_type: &str,
        permissions: &mut Vec<crate::models::DbUserPermissionDto>,
        id_counter: &mut i32,
    ) {
        // Split by semicolon for multiple resources
        for resource_entry in privs_str.split(';') {
            let resource_entry = resource_entry.trim();
            if resource_entry.is_empty() {
                continue;
            }

            // Split by colon to separate resource path from privileges
            if let Some(colon_pos) = resource_entry.rfind(':') {
                let resource_path = resource_entry[..colon_pos].trim();
                let privs_part = resource_entry[colon_pos + 1..].trim();

                // Parse individual privileges
                for privilege in privs_part.split(',') {
                    let privilege = privilege.trim().replace("_priv", "").to_uppercase();
                    if !privilege.is_empty() {
                        permissions.push(crate::models::DbUserPermissionDto {
                            id: *id_counter,
                            privilege_type: privilege,
                            resource_type: resource_type.to_string(),
                            resource_path: resource_path.to_string(),
                            granted_role: None,
                        });
                        *id_counter += 1;
                    }
                }
            }
        }
    }
}

/// Helper struct for parsed GRANT statement (Doris)
#[allow(dead_code)]
struct DorisParsedGrant {
    privileges: Vec<String>,
    resource_type: String,
    resource_path: String,
    granted_role: Option<String>,
}

#[async_trait]
impl ClusterAdapter for DorisAdapter {
    fn cluster_type(&self) -> ClusterType {
        ClusterType::Doris
    }

    fn cluster(&self) -> &Cluster {
        &self.cluster
    }

    fn get_base_url(&self) -> String {
        let protocol = if self.cluster.enable_ssl { "https" } else { "http" };
        format!("{}://{}:{}", protocol, self.cluster.fe_host, self.cluster.fe_http_port)
    }

    async fn get_backends(&self) -> ApiResult<Vec<Backend>> {
        tracing::debug!("[Doris] Fetching backends from cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await.map_err(|e| {
            tracing::error!(
                "[Doris] Failed to get MySQL client for cluster {}: {}",
                self.cluster.name,
                e
            );
            e
        })?;

        let rows = mysql_client.query("SHOW BACKENDS").await.map_err(|e| {
            tracing::error!(
                "[Doris] SHOW BACKENDS failed for cluster {}: {}",
                self.cluster.name,
                e
            );
            e
        })?;

        let backends: Vec<Backend> = rows.iter().filter_map(Self::parse_backend_row).collect();
        tracing::info!(
            "[Doris] Retrieved {} backends from cluster {}",
            backends.len(),
            self.cluster.name
        );

        Ok(backends)
    }

    async fn get_frontends(&self) -> ApiResult<Vec<Frontend>> {
        tracing::debug!("[Doris] Fetching frontends from cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await.map_err(|e| {
            tracing::error!(
                "[Doris] Failed to get MySQL client for cluster {}: {}",
                self.cluster.name,
                e
            );
            e
        })?;

        let rows = mysql_client.query("SHOW FRONTENDS").await.map_err(|e| {
            tracing::error!(
                "[Doris] SHOW FRONTENDS failed for cluster {}: {}",
                self.cluster.name,
                e
            );
            e
        })?;

        let frontends: Vec<Frontend> = rows.iter().filter_map(Self::parse_frontend_row).collect();
        tracing::info!(
            "[Doris] Retrieved {} frontends from cluster {}",
            frontends.len(),
            self.cluster.name
        );

        Ok(frontends)
    }

    async fn drop_backend(&self, host: &str, heartbeat_port: &str) -> ApiResult<()> {
        let sql = format!("ALTER SYSTEM DROP BACKEND \"{}:{}\"", host, heartbeat_port);

        tracing::info!(
            "Dropping backend node {}:{} from Doris cluster {}",
            host,
            heartbeat_port,
            self.cluster.name
        );
        self.execute_sql(&sql).await
    }

    async fn get_sessions(&self) -> ApiResult<Vec<crate::models::Session>> {
        use crate::models::Session;

        let mysql_client = self.mysql_client().await?;
        let (_, rows) = mysql_client.query_raw("SHOW PROCESSLIST").await?;

        let mut sessions = Vec::new();
        for row in rows {
            if row.len() >= 12 {
                sessions.push(Session {
                    id: row.get(1).cloned().unwrap_or_default(),
                    user: row.get(2).cloned().unwrap_or_default(),
                    host: row.get(3).cloned().unwrap_or_default(),
                    db: row.get(6).cloned(),
                    command: row.get(7).cloned().unwrap_or_default(),
                    time: row.get(8).cloned().unwrap_or_else(|| "0".to_string()),
                    state: row.get(9).cloned().unwrap_or_default(),
                    info: row.get(11).cloned(),
                });
            }
        }

        tracing::debug!(
            "Retrieved {} sessions from Doris cluster {}",
            sessions.len(),
            self.cluster.name
        );
        Ok(sessions)
    }

    async fn get_queries(&self) -> ApiResult<Vec<Query>> {
        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client.query("SHOW PROCESSLIST").await?;

        tracing::debug!(
            "Retrieved {} queries from Doris cluster {}",
            rows.len(),
            self.cluster.name
        );

        Ok(rows.iter().filter_map(Self::parse_query_row).collect())
    }

    async fn get_runtime_info(&self) -> ApiResult<RuntimeInfo> {
        let url = format!("{}/api/show_runtime_info", self.get_base_url());

        let response = self
            .http_client
            .get(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => resp
                .json()
                .await
                .map_err(|e| ApiError::cluster_connection_failed(format!("Parse failed: {}", e))),
            Ok(resp) => {
                tracing::warn!("Doris runtime info API returned {}, using default", resp.status());
                Ok(RuntimeInfo::default())
            },
            Err(e) => {
                tracing::warn!("Failed to get Doris runtime info: {}, using default", e);
                Ok(RuntimeInfo::default())
            },
        }
    }

    async fn get_metrics(&self) -> ApiResult<String> {
        let url = format!("{}/metrics", self.get_base_url());

        let response = self
            .http_client
            .get(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .send()
            .await
            .map_err(|e| ApiError::cluster_connection_failed(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ApiError::cluster_connection_failed(format!(
                "HTTP status: {}",
                response.status()
            )));
        }

        response
            .text()
            .await
            .map_err(|e| ApiError::cluster_connection_failed(format!("Read failed: {}", e)))
    }

    fn parse_prometheus_metrics(&self, metrics_text: &str) -> ApiResult<HashMap<String, f64>> {
        let mut metrics = HashMap::new();

        for line in metrics_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((name_part, value_str)) = line.rsplit_once(' ')
                && let Ok(value) = value_str.parse::<f64>()
            {
                let metric_name =
                    if let Some(pos) = name_part.find('{') { &name_part[..pos] } else { name_part };
                metrics.insert(metric_name.to_string(), value);
            }
        }

        Ok(metrics)
    }

    async fn execute_sql(&self, sql: &str) -> ApiResult<()> {
        let url = format!("{}/api/query", self.get_base_url());
        tracing::debug!("Executing SQL on Doris: {}", sql);

        let body = serde_json::json!({ "query": sql });

        // Use connection user for HTTP API operations (not for permission grants)
        // Permission grants use admin user via temporary MySQL connection in execute_request_internal
        let response = self
            .http_client
            .post(&url)
            .basic_auth(&self.cluster.username, self.cluster.get_auth_password())
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("SQL executed successfully on Doris: {}", sql);
                Ok(())
            },
            Ok(resp) => {
                tracing::debug!(
                    "HTTP API returned {}, falling back to MySQL client",
                    resp.status()
                );
                let mysql_client = self.mysql_client().await?;
                mysql_client.execute(sql).await.map(|_| ())
            },
            Err(_) => {
                tracing::debug!("HTTP API failed, falling back to MySQL client");
                let mysql_client = self.mysql_client().await?;
                mysql_client.execute(sql).await.map(|_| ())
            },
        }
    }

    async fn list_catalogs(&self) -> ApiResult<Vec<String>> {
        tracing::debug!("[Doris] Fetching catalogs from cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client.query("SHOW CATALOGS").await?;

        let mut catalogs = Vec::new();
        for row in rows {
            let obj = row
                .as_object()
                .ok_or_else(|| ApiError::internal_error("Failed to parse catalog row"))?;

            if let Some(catalog_name) = obj.get("CatalogName").and_then(|v| v.as_str()) {
                let name = catalog_name.trim().to_string();
                if !name.is_empty() {
                    catalogs.push(name);
                }
            }
        }

        tracing::info!(
            "[Doris] Retrieved {} catalogs from cluster {}",
            catalogs.len(),
            self.cluster.name
        );
        Ok(catalogs)
    }

    async fn list_databases(&self, catalog: Option<&str>) -> ApiResult<Vec<String>> {
        tracing::debug!("[Doris] Fetching databases from cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;

        // Use session mode to ensure SWITCH and SHOW DATABASES run on the same connection
        let mut session = mysql_client.create_session().await?;

        if let Some(cat) = catalog
            && !cat.is_empty()
            && cat != "default_catalog"
        {
            session.use_catalog(cat, &self.cluster.cluster_type).await?;
        }

        let (_, rows, _) = session.execute("SHOW DATABASES").await?;

        let mut databases = Vec::new();
        for row in rows {
            if let Some(db_name) = row.first() {
                let name = db_name.trim().to_string();
                if !name.is_empty() {
                    databases.push(name);
                }
            }
        }

        tracing::info!(
            "[Doris] Retrieved {} databases from cluster {}",
            databases.len(),
            self.cluster.name
        );
        Ok(databases)
    }

    async fn list_materialized_views(
        &self,
        database: Option<&str>,
    ) -> ApiResult<Vec<MaterializedView>> {
        let mysql_client = self.mysql_client().await?;
        let databases = match database {
            Some(database) => {
                MaterializedViewRef::validate_identifier("database", database)?;
                vec![database.to_string()]
            },
            None => <Self as ClusterAdapter>::list_databases(self, None).await?,
        };
        let mut materialized_views = Vec::new();
        for database in databases
            .into_iter()
            .filter(|database| Self::is_user_database(database))
        {
            match self
                .list_async_materialized_views(&mysql_client, &database)
                .await
            {
                Ok(mut views) => materialized_views.append(&mut views),
                Err(error) => tracing::warn!(
                    database,
                    "Unable to list Doris async materialized views: {error}"
                ),
            }
            match self.list_rollups(&mysql_client, &database).await {
                Ok(mut rollups) => materialized_views.append(&mut rollups),
                Err(error) => tracing::warn!(
                    database,
                    "Unable to list Doris ROLLUP materialized views: {error}"
                ),
            }
        }
        Ok(materialized_views)
    }

    async fn get_materialized_view(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<MaterializedView> {
        let mysql_client = self.mysql_client().await?;
        self.get_materialized_view_exact(&mysql_client, reference)
            .await
    }

    async fn get_materialized_view_ddl(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<String> {
        let mysql_client = self.mysql_client().await?;
        let materialized_view = self
            .get_materialized_view_exact(&mysql_client, reference)
            .await?;
        if reference.kind == MaterializedViewKind::Async {
            let (_, rows) = mysql_client
                .query_raw(&format!("SHOW CREATE MATERIALIZED VIEW {}", reference.quoted_name()))
                .await?;
            return rows
                .first()
                .and_then(|row| row.get(1).or_else(|| row.first()))
                .cloned()
                .ok_or_else(|| ApiError::not_found(reference.display_name()));
        }
        Ok(materialized_view.definition)
    }

    async fn create_materialized_view(&self, ddl: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Creating materialized view on cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;
        mysql_client.execute(ddl).await?;

        tracing::info!("[Doris] Materialized view created successfully");
        Ok(())
    }

    async fn drop_materialized_view(&self, reference: &MaterializedViewRef) -> ApiResult<()> {
        let mysql_client = self.mysql_client().await?;
        reference.validate()?;
        let sql = match reference.kind {
            MaterializedViewKind::Async => {
                self.get_materialized_view_exact(&mysql_client, reference)
                    .await?;
                format!("DROP MATERIALIZED VIEW {}", reference.quoted_name())
            },
            MaterializedViewKind::Rollup => {
                let parent = self.current_rollup_parent(&mysql_client, reference).await?;
                format!(
                    "ALTER TABLE `{}`.`{parent}` DROP ROLLUP `{}`",
                    reference.database, reference.name
                )
            },
        };
        mysql_client.execute(&sql).await.map(|_| ())
    }

    async fn refresh_materialized_view(
        &self,
        reference: &MaterializedViewRef,
        request: &RefreshMaterializedViewRequest,
    ) -> ApiResult<()> {
        let mysql_client = self.mysql_client().await?;
        reference.validate()?;
        request.validate()?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Err(ApiError::not_implemented(
                "Doris ROLLUP materialized views refresh synchronously",
            ));
        }
        self.get_materialized_view_exact(&mysql_client, reference)
            .await?;
        let mut sql = format!("REFRESH MATERIALIZED VIEW {}", reference.quoted_name());
        if let Some(partition) = &request.partition {
            sql.push_str(&format!(
                " PARTITION ({}, {})",
                partition.start.sql_literal()?,
                partition.end.sql_literal()?
            ));
        }
        match request.mode {
            RefreshMode::Auto => sql.push_str(" AUTO"),
            RefreshMode::Complete => sql.push_str(" COMPLETE"),
            RefreshMode::Async | RefreshMode::Sync => {
                return Err(ApiError::invalid_data("Doris refresh mode must be auto or complete"));
            },
        }
        mysql_client.execute(&sql).await.map(|_| ())
    }

    async fn cancel_materialized_view_refresh(
        &self,
        reference: &MaterializedViewRef,
        force: bool,
    ) -> ApiResult<()> {
        reference.validate()?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Err(ApiError::not_implemented(
                "Doris ROLLUP materialized views have no refresh job to cancel",
            ));
        }
        let suffix = if force { " FORCE" } else { "" };
        self.mysql_client()
            .await?
            .execute(&format!(
                "CANCEL REFRESH MATERIALIZED VIEW {}{suffix}",
                reference.quoted_name()
            ))
            .await
            .map(|_| ())
    }

    async fn set_materialized_view_state(
        &self,
        reference: &MaterializedViewRef,
        state: MaterializedViewState,
    ) -> ApiResult<()> {
        reference.validate()?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Err(ApiError::not_implemented(
                "Doris ROLLUP materialized views cannot be paused",
            ));
        }
        let action = match state {
            MaterializedViewState::Active => "RESUME",
            MaterializedViewState::Inactive => "PAUSE",
        };
        self.mysql_client()
            .await?
            .execute(&format!("{action} MATERIALIZED VIEW JOB ON {}", reference.quoted_name()))
            .await
            .map(|_| ())
    }

    async fn rename_materialized_view(
        &self,
        reference: &MaterializedViewRef,
        new_name: &str,
    ) -> ApiResult<()> {
        reference.validate()?;
        MaterializedViewRef::validate_identifier("new materialized view", new_name)?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Err(ApiError::not_implemented(
                "Doris ROLLUP renaming is not supported by this API",
            ));
        }
        self.mysql_client()
            .await?
            .execute(&format!(
                "ALTER MATERIALIZED VIEW {} RENAME `{new_name}`",
                reference.quoted_name()
            ))
            .await
            .map(|_| ())
    }

    async fn update_materialized_view_refresh_schedule(
        &self,
        reference: &MaterializedViewRef,
        _schedule: RefreshSchedule,
    ) -> ApiResult<()> {
        reference.validate()?;
        Err(ApiError::not_implemented(
            "Doris refresh schedule updates are unavailable until engine-specific typed syntax is verified",
        ))
    }

    async fn get_materialized_view_dependencies(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<MaterializedViewDependencies> {
        let mysql_client = self.mysql_client().await?;
        let materialized_view = self
            .get_materialized_view_exact(&mysql_client, reference)
            .await?;
        if reference.kind == MaterializedViewKind::Rollup {
            let parent =
                Self::rollup_parent_from_definition(&materialized_view.definition, reference)?;
            return Ok(MaterializedViewDependencies {
                object: reference.clone(),
                dependencies: vec![MaterializedViewDependency {
                    object: DependencyObject {
                        catalog: None,
                        database: Some(reference.database.clone()),
                        name: parent,
                        kind: RelationKind::Table,
                    },
                    evidence: DependencyEvidence::Verified,
                    source: DependencySource::RollupParent,
                    evidence_snippet: None,
                }],
                complete: true,
                warnings: Vec::new(),
            });
        }
        let (dependencies, mut warnings) =
            Self::extract_doris_dependencies(&materialized_view.definition, &reference.database);
        warnings.push("Doris dependencies are conservatively extracted from the stored definition; nested expressions, external catalogs, and unrecognized syntax are not complete lineage.".to_string());
        Ok(MaterializedViewDependencies {
            object: reference.clone(),
            dependencies,
            complete: false,
            warnings,
        })
    }

    async fn list_sql_blacklist(&self) -> ApiResult<Vec<crate::models::SqlBlacklistItem>> {
        use crate::models::SqlBlacklistItem;

        tracing::debug!("[Doris] Fetching SQL block rules from cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client
            .query("SHOW SQL_BLOCK_RULE")
            .await
            .map_err(|e| {
                tracing::error!(
                    "[Doris] SHOW SQL_BLOCK_RULE failed for cluster {}: {}",
                    self.cluster.name,
                    e
                );
                e
            })?;

        let items: Vec<SqlBlacklistItem> = rows
            .into_iter()
            .filter_map(|row| {
                let obj = row.as_object()?;

                Some(SqlBlacklistItem {
                    id: obj.get("Name")?.as_str()?.to_string(),
                    pattern: obj.get("Sql")?.as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        tracing::info!(
            "[Doris] Retrieved {} SQL block rules from cluster {}",
            items.len(),
            self.cluster.name
        );
        Ok(items)
    }

    async fn add_sql_blacklist(&self, pattern: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Adding SQL block rule to cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;

        let rule_name = format!("rule_{}", chrono::Utc::now().timestamp());
        let escaped_pattern = pattern.replace('\'', "''");

        let sql = format!(
            "CREATE SQL_BLOCK_RULE {} PROPERTIES(\"sql\"=\"{}\", \"global\"=\"true\", \"enable\"=\"true\")",
            rule_name, escaped_pattern
        );

        mysql_client.execute(&sql).await.map_err(|e| {
            tracing::error!(
                "[Doris] Failed to create SQL block rule for cluster {}: {}",
                self.cluster.name,
                e
            );
            e
        })?;

        tracing::info!(
            "[Doris] Successfully added SQL block rule {} to cluster {}",
            rule_name,
            self.cluster.name
        );
        Ok(())
    }

    async fn delete_sql_blacklist(&self, id: &str) -> ApiResult<()> {
        tracing::debug!(
            "[Doris] Deleting SQL block rule {} from cluster: {}",
            id,
            self.cluster.name
        );

        let mysql_client = self.mysql_client().await?;

        let sql = format!("DROP SQL_BLOCK_RULE {}", id);

        mysql_client.execute(&sql).await.map_err(|e| {
            tracing::error!(
                "[Doris] Failed to drop SQL block rule {} for cluster {}: {}",
                id,
                self.cluster.name,
                e
            );
            e
        })?;

        tracing::info!(
            "[Doris] Successfully deleted SQL block rule {} from cluster {}",
            id,
            self.cluster.name
        );
        Ok(())
    }

    async fn show_proc_raw(&self, path: &str) -> ApiResult<Vec<Value>> {
        let normalized_path =
            if path.starts_with('/') { path.to_string() } else { format!("/{}", path) };

        let supported_paths = [
            "backends",
            "frontends",
            "dbs",
            "current_queries",
            "transactions",
            "routine_loads",
            "stream_loads",
            "tasks",
            "resources",
            "colocation_group",
            "jobs",
            "monitor",
            "statistic",
            "cluster_balance",
            "brokers",
            "catalogs",
            "current_backend_instances",
            "auth",
            "bdbje",
            "binlog",
            "cluster_health",
            "current_query_stmts",
            "diagnose",
            "trash",
        ];

        let path_name = normalized_path.trim_start_matches('/');

        if !supported_paths.contains(&path_name) {
            match path_name {
                "compactions" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/compactions' not supported, using '/cluster_health/tablet_health' as alternative"
                    );
                    let sql = "SHOW PROC '/cluster_health/tablet_health'".to_string();
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "load_error_hub" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/load_error_hub' not supported, aggregating load errors from SHOW LOAD"
                    );
                    return self.get_load_errors_compromise().await;
                },
                "replications" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/replications' not supported. Replication info is distributed across /backends, /dbs, /cluster_health/tablet_health"
                    );
                    return Ok(Vec::new());
                },
                "historical_nodes" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/historical_nodes' not supported. Doris doesn't have historical nodes concept (StarRocks shared-data mode feature)"
                    );
                    return Ok(Vec::new());
                },
                "meta_recovery" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/meta_recovery' not supported. Doris uses different metadata recovery mechanisms"
                    );
                    return Ok(Vec::new());
                },
                "compute_nodes" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/compute_nodes' not supported, using '/backends' instead (Doris backends serve both storage and compute)"
                    );
                    let sql = "SHOW PROC '/backends'".to_string();
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "global_current_queries" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/global_current_queries' not supported, using '/current_queries' instead"
                    );
                    let sql = "SHOW PROC '/current_queries'".to_string();
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "catalog" => {
                    tracing::info!("[Doris] Mapping '/catalog' to '/catalogs'");
                    let sql = "SHOW PROC '/catalogs'".to_string();
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "warehouses" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/warehouses' not supported. This is a StarRocks shared-data mode feature."
                    );
                    return Ok(Vec::new());
                },
                _ => {
                    tracing::warn!("[Doris] Unsupported SHOW PROC path: {}", normalized_path);
                    return Err(ApiError::not_implemented(format!(
                        "SHOW PROC '{}' is not supported in Doris. Supported paths: {}",
                        normalized_path,
                        supported_paths.join(", ")
                    )));
                },
            }
        }

        let sql = format!("SHOW PROC '{}'", normalized_path);
        tracing::debug!("[Doris] Executing: {}", sql);

        let mysql_client = self.mysql_client().await?;
        mysql_client.query(&sql).await
    }

    async fn list_profiles(&self) -> ApiResult<Vec<crate::models::ProfileListItem>> {
        use crate::models::ProfileListItem;

        let mysql_client = self.mysql_client().await?;
        let (columns, rows) = mysql_client.query_raw("SHOW QUERY PROFILE").await?;

        tracing::info!(
            "[Doris] Profile list query returned {} rows with {} columns",
            rows.len(),
            columns.len()
        );

        // Doris SHOW QUERY PROFILE returns columns in this order (from SummaryProfile.SUMMARY_CAPTIONS):
        // Profile ID, Task Type, Start Time, End Time, Total, Task State, User, Default Catalog, Default Db, Sql Statement
        let profiles: Vec<ProfileListItem> = rows
            .into_iter()
            .filter(|row| {
                // Filter out aborted profiles (Task State column is at index 5)
                let state = row.get(5).map(|s| s.as_str()).unwrap_or("");
                !state.eq_ignore_ascii_case("aborted")
            })
            .map(|row| ProfileListItem {
                // Map Doris columns to ProfileListItem:
                // Profile ID (index 0) -> query_id
                // Start Time (index 2) -> start_time
                // Total (index 4) -> time
                // Task State (index 5) -> state
                // Sql Statement (index 9) -> statement
                query_id: row.first().cloned().unwrap_or_default(),
                start_time: row.get(2).cloned().unwrap_or_default(),
                time: row.get(4).cloned().unwrap_or_default(),
                state: row.get(5).cloned().unwrap_or_default(),
                statement: row.get(9).cloned().unwrap_or_default(),
            })
            .collect();

        tracing::info!(
            "[Doris] Successfully converted {} profiles (Aborted filtered)",
            profiles.len()
        );
        Ok(profiles)
    }

    async fn get_profile(&self, query_id: &str) -> ApiResult<String> {
        // Doris uses HTTP API to get profile
        // According to ProfileAction.java, we can use /api/profile/text?query_id=xxx
        let url = format!(
            "http://{}:{}/api/profile/text?query_id={}",
            self.cluster.fe_host, self.cluster.fe_http_port, query_id
        );

        tracing::debug!("[Doris] Fetching profile from: {}", url);

        let response = self
            .http_client
            .get(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .send()
            .await
            .map_err(|e| {
                tracing::error!("[Doris] Failed to fetch profile: {}", e);
                ApiError::cluster_connection_failed(format!("HTTP request failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("[Doris] Profile API returned status {}: {}", status, error_text);
            if status.as_u16() == 404 {
                return Err(ApiError::not_found(format!(
                    "Profile not found for query: {}",
                    query_id
                )));
            }
            return Err(ApiError::cluster_connection_failed(format!(
                "Profile API failed: {}",
                error_text
            )));
        }

        let profile_text = response.text().await.map_err(|e| {
            tracing::error!("[Doris] Failed to read profile response: {}", e);
            ApiError::cluster_connection_failed(format!("Failed to read response: {}", e))
        })?;

        if profile_text.trim().is_empty() {
            return Err(ApiError::not_found(format!("Profile not found for query: {}", query_id)));
        }

        tracing::info!(
            "[Doris] Successfully fetched profile, length: {} bytes",
            profile_text.len()
        );
        Ok(profile_text)
    }

    async fn create_user(&self, username: &str, password: &str) -> ApiResult<String> {
        let username = Self::escape_sql_literal(username);
        if password.is_empty() {
            Ok(format!("CREATE USER '{}'@'%';", username))
        } else {
            Ok(format!(
                "CREATE USER '{}'@'%' IDENTIFIED BY '{}';",
                username,
                Self::escape_sql_literal(password)
            ))
        }
    }

    async fn create_role(&self, role_name: &str) -> ApiResult<String> {
        Ok(format!("CREATE ROLE '{}';", Self::escape_sql_literal(role_name)))
    }

    async fn grant_permissions(
        &self,
        principal_type: &str,
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str,
        catalog: Option<&str>,
        database: &str,
        table: Option<&str>,
        with_grant_option: bool,
    ) -> ApiResult<String> {
        let priv_permissions = Self::add_priv_suffix(permissions);
        let perm_str = priv_permissions.join(", ");
        let resource = Self::build_resource_path(resource_type, catalog, database, table);

        let with_grant = if with_grant_option { " WITH GRANT OPTION" } else { "" };

        let principal = match principal_type {
            "ROLE" => format!("'{}'", Self::escape_sql_literal(principal_name)),
            "USER" => format!("'{}'@'%'", Self::escape_sql_literal(principal_name)),
            _ => {
                return Err(ApiError::ValidationError(
                    "Principal type must be USER or ROLE".to_string(),
                ));
            },
        };

        Ok(format!("GRANT {} ON {} TO {}{};", perm_str, resource, principal, with_grant))
    }

    async fn revoke_permissions(
        &self,
        principal_type: &str,
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str,
        catalog: Option<&str>,
        database: &str,
        table: Option<&str>,
    ) -> ApiResult<String> {
        let priv_permissions = Self::add_priv_suffix(permissions);
        let perm_str = priv_permissions.join(", ");
        let resource = Self::build_resource_path(resource_type, catalog, database, table);

        let principal = match principal_type {
            "ROLE" => format!("'{}'", Self::escape_sql_literal(principal_name)),
            "USER" => format!("'{}'@'%'", Self::escape_sql_literal(principal_name)),
            _ => {
                return Err(ApiError::ValidationError(
                    "Principal type must be USER or ROLE".to_string(),
                ));
            },
        };

        Ok(format!("REVOKE {} ON {} FROM {};", perm_str, resource, principal))
    }

    async fn grant_role(&self, role_name: &str, username: &str) -> ApiResult<String> {
        Ok(format!(
            "GRANT '{}' TO '{}'@'%';",
            Self::escape_sql_literal(role_name),
            Self::escape_sql_literal(username)
        ))
    }

    async fn list_user_permissions(
        &self,
        username: &str,
    ) -> ApiResult<Vec<crate::models::DbUserPermissionDto>> {
        tracing::debug!("[Doris] Listing permissions for user: {}", username);

        let mut conn = self
            .mysql_pool_manager
            .get_pool(&self.cluster)
            .await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris syntax: SHOW GRANTS FOR 'username'@'%'
        let query_str = format!("SHOW GRANTS FOR '{}'@'%'", username);

        use mysql_async::Row;
        use mysql_async::Value;
        use mysql_async::prelude::Queryable;

        let rows: Vec<Row> = match conn.query(&query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("SHOW GRANTS query failed for user {}: {}", username, e);
                return Ok(Vec::new());
            },
        };

        let mut permissions: Vec<crate::models::DbUserPermissionDto> = Vec::new();
        let mut id_counter: i32 = 1;

        // Helper function to safely get string value from row
        fn get_string_value(row: &Row, col_name: &str) -> Option<String> {
            // Try to get column index by name
            let col_idx = row
                .columns_ref()
                .iter()
                .position(|c| c.name_str() == col_name)?;
            match row.as_ref(col_idx)? {
                Value::NULL => None,
                Value::Bytes(b) => String::from_utf8(b.clone()).ok(),
                v => Some(format!("{:?}", v)),
            }
        }

        for row in rows {
            // Doris 2.x returns multi-column format:
            // UserIdentity, Comment, Password, Roles, GlobalPrivs, CatalogPrivs, DatabasePrivs,
            // TablePrivs, ColPrivs, ResourcePrivs, CloudClusterPrivs, CloudStagePrivs,
            // StorageVaultPrivs, WorkloadGroupPrivs, ComputeGroupPrivs

            // Parse Roles (granted roles)
            if let Some(roles_str) = get_string_value(&row, "Roles")
                && !roles_str.is_empty()
                && roles_str != "NULL"
            {
                for role in roles_str.split(',') {
                    let role = role.trim();
                    if !role.is_empty() {
                        permissions.push(crate::models::DbUserPermissionDto {
                            id: id_counter,
                            privilege_type: "ROLE".to_string(),
                            resource_type: "ROLE".to_string(),
                            resource_path: role.to_string(),
                            granted_role: Some(role.to_string()),
                        });
                        id_counter += 1;
                    }
                }
            }

            // Parse GlobalPrivs (e.g., "Node_priv,Admin_priv")
            if let Some(privs_str) = get_string_value(&row, "GlobalPrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                for privilege in privs_str.split(',') {
                    let privilege = privilege.trim().replace("_priv", "").to_uppercase();
                    if !privilege.is_empty() {
                        permissions.push(crate::models::DbUserPermissionDto {
                            id: id_counter,
                            privilege_type: privilege,
                            resource_type: "GLOBAL".to_string(),
                            resource_path: "*".to_string(),
                            granted_role: None,
                        });
                        id_counter += 1;
                    }
                }
            }

            // Parse CatalogPrivs (e.g., "catalog_name: Select_priv, Insert_priv")
            if let Some(privs_str) = get_string_value(&row, "CatalogPrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "CATALOG",
                    &mut permissions,
                    &mut id_counter,
                );
            }

            // Parse DatabasePrivs (e.g., "internal.information_schema: Select_priv; internal.mysql: Select_priv")
            if let Some(privs_str) = get_string_value(&row, "DatabasePrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "DATABASE",
                    &mut permissions,
                    &mut id_counter,
                );
            }

            // Parse TablePrivs
            if let Some(privs_str) = get_string_value(&row, "TablePrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "TABLE",
                    &mut permissions,
                    &mut id_counter,
                );
            }

            // Parse ColPrivs (column privileges)
            if let Some(privs_str) = get_string_value(&row, "ColPrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "COLUMN",
                    &mut permissions,
                    &mut id_counter,
                );
            }

            // Parse ResourcePrivs
            if let Some(privs_str) = get_string_value(&row, "ResourcePrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "RESOURCE",
                    &mut permissions,
                    &mut id_counter,
                );
            }

            // Parse WorkloadGroupPrivs (e.g., "normal: Usage_priv")
            if let Some(privs_str) = get_string_value(&row, "WorkloadGroupPrivs")
                && !privs_str.is_empty()
                && privs_str != "NULL"
            {
                Self::parse_doris_resource_privs(
                    &privs_str,
                    "WORKLOAD_GROUP",
                    &mut permissions,
                    &mut id_counter,
                );
            }
        }

        tracing::debug!("[Doris] Found {} permissions for user {}", permissions.len(), username);
        Ok(permissions)
    }

    async fn list_role_permissions(
        &self,
        role_name: &str,
    ) -> ApiResult<Vec<crate::models::DbUserPermissionDto>> {
        tracing::debug!("[Doris] Listing permissions for role: {}", role_name);

        // Doris doesn't support SHOW GRANTS FOR ROLE syntax
        // We need to query the role's privileges from system tables or return empty
        // For now, return empty as Doris role permissions are shown inline with user grants
        tracing::info!(
            "[Doris] Role permission query not supported, returning empty list for role: {}",
            role_name
        );
        Ok(Vec::new())
    }

    async fn list_db_accounts(&self) -> ApiResult<Vec<crate::models::DbAccountDto>> {
        tracing::debug!("[Doris] Listing database accounts");

        let mut conn = self
            .mysql_pool_manager
            .get_pool(&self.cluster)
            .await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris uses INFORMATION_SCHEMA.USER_PRIVILEGES
        let query_str =
            "SELECT DISTINCT GRANTEE FROM INFORMATION_SCHEMA.USER_PRIVILEGES ORDER BY GRANTEE";

        use mysql_async::prelude::Queryable;

        let rows: Vec<(String,)> = match conn.query(query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("INFORMATION_SCHEMA query failed: {}", e);
                return Ok(Vec::new());
            },
        };

        let mut accounts: Vec<crate::models::DbAccountDto> = Vec::new();
        for (grantee,) in rows {
            let (account_name, host) = Self::parse_user_identity(&grantee);

            if !accounts
                .iter()
                .any(|a| a.account_name == account_name && a.host == host)
            {
                accounts.push(crate::models::DbAccountDto { account_name, host, roles: vec![] });
            }
        }

        Ok(accounts)
    }

    async fn list_db_roles(&self) -> ApiResult<Vec<crate::models::DbRoleDto>> {
        tracing::debug!("[Doris] Listing database roles");

        let mut conn = self
            .mysql_pool_manager
            .get_pool(&self.cluster)
            .await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris uses SHOW ROLES
        let query_str = "SHOW ROLES";

        use mysql_async::Row;
        use mysql_async::prelude::Queryable;

        let rows: Vec<Row> = match conn.query(query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("SHOW ROLES query failed: {}", e);
                return Ok(Vec::new());
            },
        };

        let mut roles: Vec<crate::models::DbRoleDto> = Vec::new();
        for row in rows {
            // Doris SHOW ROLES returns: Name, Comment, Users, GlobalPrivs, etc.
            let role_name: Option<String> = row.get("Name");

            if let Some(name) = role_name {
                let role_type = if name == "admin"
                    || name == "operator"
                    || name == "public"
                    || name == "root"
                {
                    "built-in".to_string()
                } else {
                    "custom".to_string()
                };

                roles.push(crate::models::DbRoleDto {
                    role_name: name,
                    role_type,
                    permissions_count: None,
                });
            }
        }

        Ok(roles)
    }
}

impl DorisAdapter {
    fn escape_sql_literal(value: &str) -> String {
        value.replace('\'', "''")
    }

    /// Parse user identity string like 'username'@'host' or username@host
    fn parse_user_identity(identity: &str) -> (String, String) {
        if identity.contains('@') {
            let parts: Vec<&str> = identity.splitn(2, '@').collect();
            let account_name = parts[0].trim_matches('\'').trim_matches('"').to_string();
            let host = parts
                .get(1)
                .map(|h| h.trim_matches('\'').trim_matches('"').to_string())
                .unwrap_or_else(|| "%".to_string());
            (account_name, host)
        } else {
            (identity.trim_matches('\'').trim_matches('"').to_string(), "%".to_string())
        }
    }
}
