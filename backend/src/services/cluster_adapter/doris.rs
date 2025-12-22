// Doris Adapter
// Purpose: Implement ClusterAdapter trait for Apache Doris clusters
// Reference: https://doris.apache.org/zh-CN/docs/4.x/gettingStarted/quick-start

use super::ClusterAdapter;
use crate::models::{Backend, Cluster, ClusterType, Frontend, Query, RuntimeInfo};
use crate::services::{MySQLClient, MySQLPoolManager};
use crate::utils::{ApiError, ApiResult};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Type of materialized view in Doris
enum MaterializedViewType {
    /// Async MV (independent table) - database name
    AsyncMV(String),
    /// Rollup (sync MV, part of table) - (database name, table name)
    Rollup(String, String),
}

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
        Ok(MySQLClient::from_pool(pool))
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

    /// Find materialized view by name, returns type and location
    ///
    /// # Search Strategy
    /// 1. Check if it's an async MV (independent table) by querying each database
    /// 2. Check if it's a Rollup (sync MV) by DESC ALL on each table
    async fn find_materialized_view(
        &self,
        mysql_client: &MySQLClient,
        mv_name: &str,
    ) -> ApiResult<MaterializedViewType> {
        let (_, db_rows) = mysql_client.query_raw("SHOW DATABASES").await?;

        for db_row in db_rows {
            if let Some(db_name) = db_row.first() {
                if db_name == "information_schema"
                    || db_name == "mysql"
                    || db_name == "__internal_schema"
                {
                    continue;
                }

                let check_table_sql = format!("SELECT 1 FROM {}.{} LIMIT 1", db_name, mv_name);
                if mysql_client.query_raw(&check_table_sql).await.is_ok() {
                    tracing::debug!(
                        "[Doris] Found async MV '{}' in database '{}'",
                        mv_name,
                        db_name
                    );
                    return Ok(MaterializedViewType::AsyncMV(db_name.clone()));
                }

                let show_tables_sql = format!("SHOW TABLES FROM {}", db_name);
                if let Ok((_, table_rows)) = mysql_client.query_raw(&show_tables_sql).await {
                    for table_row in table_rows {
                        if let Some(table_name) = table_row.first() {
                            let desc_sql = format!("DESC {}.{} ALL", db_name, table_name);
                            if let Ok((cols, index_rows)) = mysql_client.query_raw(&desc_sql).await
                            {
                                let index_name_col = cols.iter().position(|c| c == "IndexName");

                                if let Some(idx) = index_name_col {
                                    for index_row in index_rows {
                                        if let Some(index_name) = index_row.get(idx) {
                                            if index_name == mv_name {
                                                tracing::debug!(
                                                    "[Doris] Found Rollup '{}' in table '{}.{}'",
                                                    mv_name,
                                                    db_name,
                                                    table_name
                                                );
                                                return Ok(MaterializedViewType::Rollup(
                                                    db_name.clone(),
                                                    table_name.clone(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(ApiError::not_found(format!("Materialized view {} not found in any database", mv_name)))
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
            .map(|p| {
                if p.ends_with("_PRIV") {
                    p.to_string()
                } else {
                    format!("{}_PRIV", p)
                }
            })
            .collect()
    }

    /// Build resource path for Doris
    /// Supports:
    /// - catalog level: CATALOG catalog_name or ALL CATALOGS  
    /// - database level: database.* or catalog.database.* or *.* (all databases)
    /// - table level: database.table or catalog.database.table or database.* (all tables)
    fn build_resource_path(resource_type: &str, database: &str, table: Option<&str>) -> String {
        match resource_type.to_uppercase().as_str() {
            "CATALOG" => {
                // Catalog level permissions
                if database == "*" {
                    "ALL CATALOGS".to_string()
                } else {
                    format!("CATALOG {}", database)
                }
            }
            "TABLE" => {
                // Table level permissions
                if database == "*" {
                    // All databases, all tables
                    "*.*".to_string()
                } else if let Some(table_name) = table {
                    if table_name == "*" {
                        // Specific database, all tables
                        format!("{}.*", database)
                    } else {
                        // Specific database and table
                        format!("{}.{}", database, table_name)
                    }
                } else {
                    // No table specified, default to all tables in database
                    format!("{}.*", database)
                }
            }
            _ => {
                // Database level permissions (default)
                if database == "*" {
                    "*.*".to_string()
                } else {
                    format!("{}.*", database)
                }
            }
        }
    }

    /// Parse a GRANT statement into structured permission data
    fn parse_grant_statement(statement: &str) -> Option<DorisParsedGrant> {
        let statement = statement.trim();
        
        // Check if it's a role grant: GRANT 'role_name' TO 'user'@'%'
        if statement.starts_with("GRANT '") || statement.starts_with("GRANT \"") {
            // Role grant
            let role_start = 7; // After "GRANT '"
            if let Some(role_end) = statement[role_start..].find('\'').or_else(|| statement[role_start..].find('"')) {
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

        Some(DorisParsedGrant {
            privileges,
            resource_type,
            resource_path,
            granted_role: None,
        })
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

        let response = self
            .http_client
            .post(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
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

        if let Some(cat) = catalog {
            if !cat.is_empty() && cat != "default_catalog" {
                session.use_catalog(cat, &self.cluster.cluster_type).await?;
            }
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
    ) -> ApiResult<Vec<crate::models::MaterializedView>> {
        use crate::models::MaterializedView;

        tracing::debug!(
            "[Doris] Listing materialized views (Rollups) from cluster: {}",
            self.cluster.name
        );

        let mysql_client = self.mysql_client().await?;
        let mut mvs = Vec::new();

        let databases = if let Some(db) = database {
            vec![db.to_string()]
        } else {
            <Self as ClusterAdapter>::list_databases(self, None).await?
        };

        for db in databases {
            if db.starts_with("__") || db == "information_schema" || db == "mysql" || db == "sys" {
                continue;
            }

            tracing::info!("[Doris] Scanning database: {}", db);

            let tables_sql = format!("SHOW TABLES FROM {}", db);
            let (_, table_rows) = match mysql_client.query_raw(&tables_sql).await {
                Ok(result) => {
                    tracing::info!("[Doris] Found {} tables in database {}", result.1.len(), db);
                    result
                },
                Err(e) => {
                    tracing::warn!("[Doris] Failed to list tables in database {}: {}", db, e);
                    continue;
                },
            };

            for table_row in table_rows {
                if let Some(table_name) = table_row.first() {
                    let row_count = match mysql_client
                        .query_raw(&format!("SELECT COUNT(*) FROM `{}`.`{}`", db, table_name))
                        .await
                    {
                        Ok((_, count_rows)) => count_rows
                            .first()
                            .and_then(|row| row.first())
                            .and_then(|v| v.parse::<i64>().ok()),
                        Err(_) => None,
                    };

                    let create_time = match mysql_client.query_raw(&format!("SELECT CREATE_TIME FROM information_schema.TABLES WHERE TABLE_SCHEMA='{}' AND TABLE_NAME='{}'", db, table_name)).await {
                        Ok((_, time_rows)) => {
                            time_rows.first()
                                .and_then(|row| row.first())
                                .map(|s| s.to_string())
                        },
                        Err(_) => None,
                    };

                    tracing::info!("[Doris] Checking if {}.{} is async MV", db, table_name);
                    let is_async_mv = match mysql_client
                        .query_raw(&format!(
                            "SHOW CREATE MATERIALIZED VIEW `{}`.`{}`",
                            db, table_name
                        ))
                        .await
                    {
                        Ok(_) => {
                            tracing::info!(
                                "[Doris] ✅ {}.{} is an async materialized view",
                                db,
                                table_name
                            );
                            true
                        },
                        Err(e) => {
                            tracing::debug!(
                                "[Doris] ❌ {}.{} is NOT an async MV: {}",
                                db,
                                table_name,
                                e
                            );
                            false
                        },
                    };

                    if is_async_mv {
                        tracing::debug!("[Doris] Found async MV: {}.{}", db, table_name);
                        mvs.push(MaterializedView {
                            id: format!("{}.{}", db, table_name),
                            name: table_name.clone(),
                            database_name: db.clone(),
                            text: format!("Async materialized view in database {}", db),
                            rows: row_count,
                            refresh_type: "ASYNC".to_string(),
                            is_active: true,
                            partition_type: Some("UNPARTITIONED".to_string()),
                            task_id: None,
                            task_name: None,
                            last_refresh_start_time: create_time.clone(),
                            last_refresh_finished_time: create_time,
                            last_refresh_duration: None,
                            last_refresh_state: Some("SUCCESS".to_string()),
                        });
                        continue;
                    }

                    let desc_sql = format!("DESC `{}`.`{}` ALL", db, table_name);
                    if let Ok((_, rows)) = mysql_client.query_raw(&desc_sql).await {
                        let mut seen_indexes = std::collections::HashSet::new();

                        for row in rows {
                            if let Some(index_name) = row.first() {
                                if index_name == table_name || index_name.is_empty() {
                                    continue;
                                }

                                if seen_indexes.insert(index_name.clone()) {
                                    tracing::debug!(
                                        "[Doris] Found rollup: {} in table {}.{}",
                                        index_name,
                                        db,
                                        table_name
                                    );

                                    let (rollup_state, rollup_create_time, rollup_finish_time) =
                                        match mysql_client
                                            .query_raw(&format!(
                                                "SHOW ALTER TABLE ROLLUP FROM `{}`",
                                                db
                                            ))
                                            .await
                                        {
                                            Ok((_, job_rows)) => {
                                                let mut state = "FINISHED".to_string();
                                                let mut create_t = create_time.clone();
                                                let mut finish_t = create_time.clone();

                                                for job_row in job_rows {
                                                    if job_row.get(1) == Some(table_name)
                                                        && job_row.get(5) == Some(index_name)
                                                    {
                                                        state = job_row
                                                            .get(8)
                                                            .unwrap_or(&"FINISHED".to_string())
                                                            .clone();
                                                        create_t =
                                                            job_row.get(2).map(|s| s.clone());
                                                        finish_t =
                                                            job_row.get(3).map(|s| s.clone());
                                                        break;
                                                    }
                                                }
                                                (state, create_t, finish_t)
                                            },
                                            Err(_) => (
                                                "FINISHED".to_string(),
                                                create_time.clone(),
                                                create_time.clone(),
                                            ),
                                        };

                                    mvs.push(MaterializedView {
                                        id: format!("{}.{}.{}", db, table_name, index_name),
                                        name: index_name.clone(),
                                        database_name: db.clone(),
                                        text: format!("Rollup of table {}.{}", db, table_name),
                                        rows: row_count,
                                        refresh_type: "ROLLUP".to_string(),
                                        is_active: rollup_state == "FINISHED",
                                        partition_type: Some("UNPARTITIONED".to_string()),
                                        task_id: None,
                                        task_name: None,
                                        last_refresh_start_time: rollup_create_time,
                                        last_refresh_finished_time: rollup_finish_time,
                                        last_refresh_duration: None,
                                        last_refresh_state: Some(rollup_state),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        tracing::info!("[Doris] Retrieved {} materialized views (Rollups)", mvs.len());
        Ok(mvs)
    }

    async fn get_materialized_view_ddl(&self, mv_name: &str) -> ApiResult<String> {
        tracing::debug!(
            "[Doris] Getting MV DDL for {} from cluster: {}",
            mv_name,
            self.cluster.name
        );

        let mysql_client = self.mysql_client().await?;

        let databases = <Self as ClusterAdapter>::list_databases(self, None).await?;

        for db in databases {
            if db.starts_with("__") || db == "information_schema" || db == "mysql" {
                continue;
            }

            let tables_sql = format!("SHOW TABLES FROM {}", db);
            let (_, table_rows) = mysql_client.query_raw(&tables_sql).await?;

            for table_row in table_rows {
                if let Some(table_name) = table_row.first() {
                    let sql = format!("DESC {}.{} ALL", db, table_name);
                    if let Ok((_, rows)) = mysql_client.query_raw(&sql).await {
                        for row in rows {
                            if let Some(index_name) = row.first() {
                                if index_name == mv_name {
                                    let ddl_sql =
                                        format!("SHOW CREATE TABLE {}.{}", db, table_name);
                                    let (_, ddl_rows) = mysql_client.query_raw(&ddl_sql).await?;
                                    if let Some(ddl_row) = ddl_rows.first() {
                                        if let Some(ddl) = ddl_row.get(1) {
                                            return Ok(ddl.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(ApiError::not_found(format!(
            "Materialized view '{}' not found or DDL unavailable",
            mv_name
        )))
    }

    async fn create_materialized_view(&self, ddl: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Creating materialized view on cluster: {}", self.cluster.name);

        let mysql_client = self.mysql_client().await?;
        mysql_client.execute(ddl).await?;

        tracing::info!("[Doris] Materialized view created successfully");
        Ok(())
    }

    async fn drop_materialized_view(&self, mv_name: &str) -> ApiResult<()> {
        tracing::debug!(
            "[Doris] Dropping materialized view {} from cluster: {}",
            mv_name,
            self.cluster.name
        );

        let mysql_client = self.mysql_client().await?;

        let mv_info = self.find_materialized_view(&mysql_client, mv_name).await?;

        match mv_info {
            MaterializedViewType::AsyncMV(db_name) => {
                let sql = format!("DROP MATERIALIZED VIEW IF EXISTS `{}`.`{}`", db_name, mv_name);
                tracing::debug!("[Doris] Executing: {}", sql);
                mysql_client.execute(&sql).await?;
                tracing::info!(
                    "[Doris] Async materialized view '{}.{}' dropped successfully",
                    db_name,
                    mv_name
                );
            },
            MaterializedViewType::Rollup(db_name, table_name) => {
                let sql =
                    format!("ALTER TABLE `{}`.`{}` DROP ROLLUP `{}`", db_name, table_name, mv_name);
                tracing::debug!("[Doris] Executing: {}", sql);
                mysql_client.execute(&sql).await?;
                tracing::info!(
                    "[Doris] Rollup '{}' dropped from table '{}.{}'",
                    mv_name,
                    db_name,
                    table_name
                );
            },
        }

        Ok(())
    }

    async fn refresh_materialized_view(
        &self,
        mv_name: &str,
        partition_start: Option<&str>,
        partition_end: Option<&str>,
        _force: bool,
        mode: &str,
    ) -> ApiResult<()> {
        tracing::debug!(
            "[Doris] Refreshing materialized view {} on cluster: {}",
            mv_name,
            self.cluster.name
        );

        let mysql_client = self.mysql_client().await?;

        let mv_info = self.find_materialized_view(&mysql_client, mv_name).await?;

        match mv_info {
            MaterializedViewType::AsyncMV(db_name) => {
                let sql = if let (Some(start), Some(end)) = (partition_start, partition_end) {
                    format!(
                        "REFRESH MATERIALIZED VIEW {}.{} PARTITION ({}, {})",
                        db_name, mv_name, start, end
                    )
                } else if mode.to_uppercase() == "COMPLETE" {
                    format!("REFRESH MATERIALIZED VIEW {}.{} COMPLETE", db_name, mv_name)
                } else {
                    format!("REFRESH MATERIALIZED VIEW {}.{} AUTO", db_name, mv_name)
                };

                tracing::debug!("[Doris] Executing: {}", sql);
                mysql_client.execute(&sql).await?;
                tracing::info!(
                    "[Doris] Async materialized view {} refreshed successfully",
                    mv_name
                );
            },
            MaterializedViewType::Rollup(db_name, table_name) => {
                tracing::warn!(
                    "[Doris] Rollup '{}' in table '{}.{}' is automatically maintained",
                    mv_name,
                    db_name,
                    table_name
                );
                return Err(ApiError::not_implemented(format!(
                    "Doris Rollup '{}' is a synchronous materialized view that is automatically maintained in real-time. \
                     Manual refresh is not supported. The Rollup data is always up-to-date with the base table '{}.{}'.",
                    mv_name, db_name, table_name
                )));
            },
        }

        Ok(())
    }

    async fn alter_materialized_view(&self, mv_name: &str, alter_clause: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Altering materialized view on cluster: {}", self.cluster.name);

        let clause_upper = alter_clause.trim().to_uppercase();
        let mysql_client = self.mysql_client().await?;

        if clause_upper == "ACTIVE" || clause_upper == "INACTIVE" {
            let mv_info = self.find_materialized_view(&mysql_client, mv_name).await?;

            match mv_info {
                MaterializedViewType::AsyncMV(db_name) => {
                    let alter_sql = if clause_upper == "ACTIVE" {
                        format!("RESUME MATERIALIZED VIEW JOB ON {}.{}", db_name, mv_name)
                    } else {
                        format!("PAUSE MATERIALIZED VIEW JOB ON {}.{}", db_name, mv_name)
                    };

                    tracing::debug!("[Doris] Executing: {}", alter_sql);
                    mysql_client.execute(&alter_sql).await?;
                },
                MaterializedViewType::Rollup(_, _) => {
                    return Err(ApiError::not_implemented(format!(
                        "Doris Rollup '{}' is a synchronous materialized view that is always active. \
                         ACTIVE/INACTIVE operations are only supported for asynchronous materialized views. \
                         Rollups are automatically maintained in real-time and cannot be paused.",
                        mv_name
                    )));
                },
            }
        } else {
            let alter_sql = format!("ALTER MATERIALIZED VIEW {} {}", mv_name, alter_clause);
            tracing::debug!("[Doris] Executing: {}", alter_sql);

            let result = mysql_client.execute(&alter_sql).await;
            if result.is_err() {
                tracing::debug!(
                    "[Doris] ALTER MATERIALIZED VIEW failed, trying ALTER TABLE for Rollup"
                );

                let mv_info = self.find_materialized_view(&mysql_client, mv_name).await?;
                match mv_info {
                    MaterializedViewType::AsyncMV(_) => {
                        result?;
                    },
                    MaterializedViewType::Rollup(db_name, table_name) => {
                        let rollup_sql =
                            format!("ALTER TABLE `{}`.`{}` {}", db_name, table_name, alter_clause);
                        tracing::debug!("[Doris] Executing: {}", rollup_sql);
                        mysql_client.execute(&rollup_sql).await?;
                    },
                }
            } else {
                result?;
            }
        }

        tracing::info!("[Doris] Materialized view {} altered successfully", mv_name);
        Ok(())
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
                    let sql = format!("SHOW PROC '/cluster_health/tablet_health'");
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
                    let sql = format!("SHOW PROC '/backends'");
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "global_current_queries" => {
                    tracing::info!(
                        "[Doris] SHOW PROC '/global_current_queries' not supported, using '/current_queries' instead"
                    );
                    let sql = format!("SHOW PROC '/current_queries'");
                    let mysql_client = self.mysql_client().await?;
                    return mysql_client.query(&sql).await;
                },
                "catalog" => {
                    tracing::info!("[Doris] Mapping '/catalog' to '/catalogs'");
                    let sql = format!("SHOW PROC '/catalogs'");
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
                query_id: row.get(0).cloned().unwrap_or_default(),
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
        if password.is_empty() {
            Ok(format!("CREATE USER '{}'@'%';", username))
        } else {
            Ok(format!(
                "CREATE USER '{}'@'%' IDENTIFIED BY '{}';",
                username, password
            ))
        }
    }

    async fn create_role(&self, role_name: &str) -> ApiResult<String> {
        Ok(format!("CREATE ROLE '{}';", role_name))
    }

    async fn grant_permissions(
        &self,
        principal_type: &str,
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str,
        database: &str,
        table: Option<&str>,
        with_grant_option: bool,
    ) -> ApiResult<String> {
        let priv_permissions = Self::add_priv_suffix(permissions);
        let perm_str = priv_permissions.join(", ");
        let resource = Self::build_resource_path(resource_type, database, table);
        
        let with_grant = if with_grant_option {
            " WITH GRANT OPTION"
        } else {
            ""
        };

        let principal = match principal_type {
            "ROLE" => format!("'{}'", principal_name),
            "USER" => format!("'{}'@'%'", principal_name),
            _ => return Err(ApiError::ValidationError(
                "Principal type must be USER or ROLE".to_string(),
            )),
        };

        Ok(format!(
            "GRANT {} ON {} TO {};{}",
            perm_str, resource, principal, with_grant
        ))
    }

    async fn revoke_permissions(
        &self,
        principal_type: &str,
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str,
        database: &str,
        table: Option<&str>,
    ) -> ApiResult<String> {
        let priv_permissions = Self::add_priv_suffix(permissions);
        let perm_str = priv_permissions.join(", ");
        let resource = Self::build_resource_path(resource_type, database, table);
        
        let principal = match principal_type {
            "ROLE" => format!("'{}'", principal_name),
            "USER" => format!("'{}'@'%'", principal_name),
            _ => return Err(ApiError::ValidationError(
                "Principal type must be USER or ROLE".to_string(),
            )),
        };

        Ok(format!(
            "REVOKE {} ON {} FROM {};",
            perm_str, resource, principal
        ))
    }

    async fn grant_role(&self, role_name: &str, username: &str) -> ApiResult<String> {
        Ok(format!(
            "GRANT '{}' TO '{}'@'%';",
            role_name, username
        ))
    }

    async fn list_user_permissions(&self, username: &str) -> ApiResult<Vec<crate::models::DbUserPermissionDto>> {
        tracing::debug!("[Doris] Listing permissions for user: {}", username);
        
        let mut conn = self.mysql_pool_manager.get_pool(&self.cluster).await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris syntax: SHOW GRANTS FOR 'username'@'%'
        let query_str = format!("SHOW GRANTS FOR '{}'@'%'", username);

        use mysql_async::prelude::Queryable;
        use mysql_async::Row;
        use mysql_async::Value;
        
        let rows: Vec<Row> = match conn.query(&query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("SHOW GRANTS query failed for user {}: {}", username, e);
                return Ok(Vec::new());
            }
        };

        let mut permissions: Vec<crate::models::DbUserPermissionDto> = Vec::new();
        let mut id_counter: i32 = 1;

        // Helper function to safely get string value from row
        fn get_string_value(row: &Row, col_name: &str) -> Option<String> {
            // Try to get column index by name
            let col_idx = row.columns_ref().iter().position(|c| c.name_str() == col_name)?;
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
            if let Some(roles_str) = get_string_value(&row, "Roles") {
                if !roles_str.is_empty() && roles_str != "NULL" {
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
            }

            // Parse GlobalPrivs (e.g., "Node_priv,Admin_priv")
            if let Some(privs_str) = get_string_value(&row, "GlobalPrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
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
            }

            // Parse CatalogPrivs (e.g., "catalog_name: Select_priv, Insert_priv")
            if let Some(privs_str) = get_string_value(&row, "CatalogPrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "CATALOG", &mut permissions, &mut id_counter);
                }
            }

            // Parse DatabasePrivs (e.g., "internal.information_schema: Select_priv; internal.mysql: Select_priv")
            if let Some(privs_str) = get_string_value(&row, "DatabasePrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "DATABASE", &mut permissions, &mut id_counter);
                }
            }

            // Parse TablePrivs
            if let Some(privs_str) = get_string_value(&row, "TablePrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "TABLE", &mut permissions, &mut id_counter);
                }
            }

            // Parse ColPrivs (column privileges)
            if let Some(privs_str) = get_string_value(&row, "ColPrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "COLUMN", &mut permissions, &mut id_counter);
                }
            }

            // Parse ResourcePrivs
            if let Some(privs_str) = get_string_value(&row, "ResourcePrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "RESOURCE", &mut permissions, &mut id_counter);
                }
            }

            // Parse WorkloadGroupPrivs (e.g., "normal: Usage_priv")
            if let Some(privs_str) = get_string_value(&row, "WorkloadGroupPrivs") {
                if !privs_str.is_empty() && privs_str != "NULL" {
                    Self::parse_doris_resource_privs(&privs_str, "WORKLOAD_GROUP", &mut permissions, &mut id_counter);
                }
            }
        }

        tracing::debug!("[Doris] Found {} permissions for user {}", permissions.len(), username);
        Ok(permissions)
    }

    async fn list_role_permissions(&self, role_name: &str) -> ApiResult<Vec<crate::models::DbUserPermissionDto>> {
        tracing::debug!("[Doris] Listing permissions for role: {}", role_name);
        
        // Doris doesn't support SHOW GRANTS FOR ROLE syntax
        // We need to query the role's privileges from system tables or return empty
        // For now, return empty as Doris role permissions are shown inline with user grants
        tracing::info!("[Doris] Role permission query not supported, returning empty list for role: {}", role_name);
        Ok(Vec::new())
    }

    async fn list_db_accounts(&self) -> ApiResult<Vec<crate::models::DbAccountDto>> {
        tracing::debug!("[Doris] Listing database accounts");
        
        let mut conn = self.mysql_pool_manager.get_pool(&self.cluster).await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris uses INFORMATION_SCHEMA.USER_PRIVILEGES
        let query_str = "SELECT DISTINCT GRANTEE FROM INFORMATION_SCHEMA.USER_PRIVILEGES ORDER BY GRANTEE";

        use mysql_async::prelude::Queryable;
        
        let rows: Vec<(String,)> = match conn.query(query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("INFORMATION_SCHEMA query failed: {}", e);
                return Ok(Vec::new());
            }
        };

        let mut accounts: Vec<crate::models::DbAccountDto> = Vec::new();
        for (grantee,) in rows {
            let (account_name, host) = Self::parse_user_identity(&grantee);
            
            if !accounts.iter().any(|a| a.account_name == account_name && a.host == host) {
                accounts.push(crate::models::DbAccountDto {
                    account_name,
                    host,
                    roles: vec![],
                });
            }
        }

        Ok(accounts)
    }

    async fn list_db_roles(&self) -> ApiResult<Vec<crate::models::DbRoleDto>> {
        tracing::debug!("[Doris] Listing database roles");
        
        let mut conn = self.mysql_pool_manager.get_pool(&self.cluster).await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Doris uses SHOW ROLES
        let query_str = "SHOW ROLES";

        use mysql_async::prelude::Queryable;
        use mysql_async::Row;
        
        let rows: Vec<Row> = match conn.query(query_str).await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!("SHOW ROLES query failed: {}", e);
                return Ok(Vec::new());
            }
        };

        let mut roles: Vec<crate::models::DbRoleDto> = Vec::new();
        for row in rows {
            // Doris SHOW ROLES returns: Name, Comment, Users, GlobalPrivs, etc.
            let role_name: Option<String> = row.get("Name");
            
            if let Some(name) = role_name {
                let role_type = if name == "admin" || name == "operator" || name == "public" || name == "root" {
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
    /// Parse user identity string like 'username'@'host' or username@host
    fn parse_user_identity(identity: &str) -> (String, String) {
        if identity.contains('@') {
            let parts: Vec<&str> = identity.splitn(2, '@').collect();
            let account_name = parts[0].trim_matches('\'').trim_matches('"').to_string();
            let host = parts.get(1)
                .map(|h| h.trim_matches('\'').trim_matches('"').to_string())
                .unwrap_or_else(|| "%".to_string());
            (account_name, host)
        } else {
            (identity.trim_matches('\'').trim_matches('"').to_string(), "%".to_string())
        }
    }
}
