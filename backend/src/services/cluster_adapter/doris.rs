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
                tracing::error!("Failed to build HTTP client for Doris cluster {}: {}", cluster.name, e);
                Client::default()
            });

        Self { http_client, cluster, mysql_pool_manager }
    }

    async fn mysql_client(&self) -> ApiResult<MySQLClient> {
        let pool = self.mysql_pool_manager.get_pool(&self.cluster).await?;
        Ok(MySQLClient::from_pool(pool))
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
            cluster_decommissioned: "false".to_string(), // Doris doesn't have this
            tablet_num: Self::get_str(row, "TabletNum"),
            data_used_capacity: Self::get_str(row, "DataUsedCapacity"),
            avail_capacity: Self::get_str(row, "AvailCapacity"),
            total_capacity: Self::get_str(row, "TotalCapacity"),
            used_pct: Self::get_str(row, "UsedPct"),
            max_disk_used_pct: Self::get_str(row, "MaxDiskUsedPct"),
            err_msg: Self::get_str(row, "ErrMsg"),
            version: Self::get_str(row, "Version"),
            status: Self::get_str(row, "Status"),
            // Default values for StarRocks-specific fields
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
        // Doris uses QueryId field for the actual query ID, Id is the connection ID
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
        // Doris uses SHOW BACKENDS instead of SHOW PROC '/backends'
        tracing::debug!("[Doris] Fetching backends from cluster: {}", self.cluster.name);
        
        let mysql_client = self.mysql_client().await.map_err(|e| {
            tracing::error!("[Doris] Failed to get MySQL client for cluster {}: {}", self.cluster.name, e);
            e
        })?;
        
        let rows = mysql_client.query("SHOW BACKENDS").await.map_err(|e| {
            tracing::error!("[Doris] SHOW BACKENDS failed for cluster {}: {}", self.cluster.name, e);
            e
        })?;

        let backends: Vec<Backend> = rows.iter().filter_map(Self::parse_backend_row).collect();
        tracing::info!("[Doris] Retrieved {} backends from cluster {}", backends.len(), self.cluster.name);

        Ok(backends)
    }

    async fn get_frontends(&self) -> ApiResult<Vec<Frontend>> {
        // Doris uses SHOW FRONTENDS instead of SHOW PROC '/frontends'
        tracing::debug!("[Doris] Fetching frontends from cluster: {}", self.cluster.name);
        
        let mysql_client = self.mysql_client().await.map_err(|e| {
            tracing::error!("[Doris] Failed to get MySQL client for cluster {}: {}", self.cluster.name, e);
            e
        })?;
        
        let rows = mysql_client.query("SHOW FRONTENDS").await.map_err(|e| {
            tracing::error!("[Doris] SHOW FRONTENDS failed for cluster {}: {}", self.cluster.name, e);
            e
        })?;

        let frontends: Vec<Frontend> = rows.iter().filter_map(Self::parse_frontend_row).collect();
        tracing::info!("[Doris] Retrieved {} frontends from cluster {}", frontends.len(), self.cluster.name);

        Ok(frontends)
    }

    async fn drop_backend(&self, host: &str, heartbeat_port: &str) -> ApiResult<()> {
        // Doris uses ALTER SYSTEM DROP BACKEND
        let sql = format!("ALTER SYSTEM DROP BACKEND \"{}:{}\"", host, heartbeat_port);

        tracing::info!("Dropping backend node {}:{} from Doris cluster {}", host, heartbeat_port, self.cluster.name);
        self.execute_sql(&sql).await
    }

    async fn get_sessions(&self) -> ApiResult<Vec<crate::models::Session>> {
        use crate::models::Session;
        
        let mysql_client = self.mysql_client().await?;
        let (_, rows) = mysql_client.query_raw("SHOW PROCESSLIST").await?;
        
        let mut sessions = Vec::new();
        for row in rows {
            // Doris SHOW PROCESSLIST columns: CurrentConnected(0), Id(1), User(2), Host(3), LoginTime(4), 
            // Catalog(5), Db(6), Command(7), Time(8), State(9), QueryId(10), Info(11)
            if row.len() >= 12 {
                sessions.push(Session {
                    id: row.get(1).cloned().unwrap_or_default(),  // Id
                    user: row.get(2).cloned().unwrap_or_default(), // User
                    host: row.get(3).cloned().unwrap_or_default(), // Host
                    db: row.get(6).cloned(),                        // Db
                    command: row.get(7).cloned().unwrap_or_default(), // Command
                    time: row.get(8).cloned().unwrap_or_else(|| "0".to_string()), // Time
                    state: row.get(9).cloned().unwrap_or_default(), // State
                    info: row.get(11).cloned(),                     // Info
                });
            }
        }
        
        tracing::debug!("Retrieved {} sessions from Doris cluster {}", sessions.len(), self.cluster.name);
        Ok(sessions)
    }

    async fn get_queries(&self) -> ApiResult<Vec<Query>> {
        // Doris uses SHOW PROCESSLIST (same as MySQL)
        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client.query("SHOW PROCESSLIST").await?;

        tracing::debug!("Retrieved {} queries from Doris cluster {}", rows.len(), self.cluster.name);

        Ok(rows.iter().filter_map(Self::parse_query_row).collect())
    }

    async fn get_runtime_info(&self) -> ApiResult<RuntimeInfo> {
        // Doris has similar HTTP API structure
        let url = format!("{}/api/show_runtime_info", self.get_base_url());

        let response = self
            .http_client
            .get(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                resp.json().await.map_err(|e| ApiError::cluster_connection_failed(format!("Parse failed: {}", e)))
            },
            Ok(resp) => {
                // Doris might not have this API, return default
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
        // Doris exposes Prometheus metrics at /metrics (same as StarRocks)
        let url = format!("{}/metrics", self.get_base_url());

        let response = self
            .http_client
            .get(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .send()
            .await
            .map_err(|e| ApiError::cluster_connection_failed(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(ApiError::cluster_connection_failed(format!("HTTP status: {}", response.status())));
        }

        response.text().await.map_err(|e| ApiError::cluster_connection_failed(format!("Read failed: {}", e)))
    }

    fn parse_prometheus_metrics(&self, metrics_text: &str) -> ApiResult<HashMap<String, f64>> {
        // Prometheus format is same for both StarRocks and Doris
        let mut metrics = HashMap::new();

        for line in metrics_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((name_part, value_str)) = line.rsplit_once(' ')
                && let Ok(value) = value_str.parse::<f64>()
            {
                let metric_name = if let Some(pos) = name_part.find('{') { &name_part[..pos] } else { name_part };
                metrics.insert(metric_name.to_string(), value);
            }
        }

        Ok(metrics)
    }

    async fn execute_sql(&self, sql: &str) -> ApiResult<()> {
        // Doris also supports HTTP API for SQL execution
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
                // Fallback: use MySQL client for SQL execution
                tracing::debug!("HTTP API returned {}, falling back to MySQL client", resp.status());
                let mysql_client = self.mysql_client().await?;
                mysql_client.execute(sql).await.map(|_| ())
            },
            Err(_) => {
                // Fallback: use MySQL client for SQL execution
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
            let obj = row.as_object().ok_or_else(|| {
                ApiError::internal_error("Failed to parse catalog row")
            })?;
            
            // Doris SHOW CATALOGS: CatalogName column contains the catalog name
            if let Some(catalog_name) = obj.get("CatalogName").and_then(|v| v.as_str()) {
                let name = catalog_name.trim().to_string();
                if !name.is_empty() {
                    catalogs.push(name);
                }
            }
        }
        
        tracing::info!("[Doris] Retrieved {} catalogs from cluster {}", catalogs.len(), self.cluster.name);
        Ok(catalogs)
    }

    async fn list_databases(&self, catalog: Option<&str>) -> ApiResult<Vec<String>> {
        tracing::debug!("[Doris] Fetching databases from cluster: {}", self.cluster.name);
        
        let mysql_client = self.mysql_client().await?;
        
        // Switch catalog if specified (Doris uses SWITCH catalog_name)
        if let Some(cat) = catalog {
            let switch_sql = format!("SWITCH {}", cat);
            mysql_client.execute(&switch_sql).await?;
        }
        
        let (_, rows) = mysql_client.query_raw("SHOW DATABASES").await?;
        
        let mut databases = Vec::new();
        for row in rows {
            if let Some(db_name) = row.first() {
                let name = db_name.trim().to_string();
                if !name.is_empty() {
                    databases.push(name);
                }
            }
        }
        
        tracing::info!("[Doris] Retrieved {} databases from cluster {}", databases.len(), self.cluster.name);
        Ok(databases)
    }

    async fn list_sql_blacklist(&self) -> ApiResult<Vec<crate::models::SqlBlacklistItem>> {
        use crate::models::SqlBlacklistItem;
        
        tracing::debug!("[Doris] Fetching SQL block rules from cluster: {}", self.cluster.name);
        
        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client.query("SHOW SQL_BLOCK_RULE").await.map_err(|e| {
            tracing::error!("[Doris] SHOW SQL_BLOCK_RULE failed for cluster {}: {}", self.cluster.name, e);
            e
        })?;
        
        let items: Vec<SqlBlacklistItem> = rows
            .into_iter()
            .filter_map(|row| {
                let obj = row.as_object()?;
                // Doris SQL_BLOCK_RULE columns: Name, Sql, SqlHash, PartitionNum, TabletNum, Cardinality, Global, Enable
                Some(SqlBlacklistItem {
                    id: obj.get("Name")?.as_str()?.to_string(),
                    pattern: obj.get("Sql")?.as_str().unwrap_or("").to_string(),
                })
            })
            .collect();
        
        tracing::info!("[Doris] Retrieved {} SQL block rules from cluster {}", items.len(), self.cluster.name);
        Ok(items)
    }

    async fn add_sql_blacklist(&self, pattern: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Adding SQL block rule to cluster: {}", self.cluster.name);
        
        let mysql_client = self.mysql_client().await?;
        
        // Doris uses CREATE SQL_BLOCK_RULE with a unique name
        let rule_name = format!("rule_{}", chrono::Utc::now().timestamp());
        let escaped_pattern = pattern.replace('\'', "''");
        
        // Doris SQL_BLOCK_RULE syntax: CREATE SQL_BLOCK_RULE rule_name PROPERTIES("sql"="pattern", "global"="true", "enable"="true")
        let sql = format!(
            "CREATE SQL_BLOCK_RULE {} PROPERTIES(\"sql\"=\"{}\", \"global\"=\"true\", \"enable\"=\"true\")",
            rule_name, escaped_pattern
        );
        
        mysql_client.execute(&sql).await.map_err(|e| {
            tracing::error!("[Doris] Failed to create SQL block rule for cluster {}: {}", self.cluster.name, e);
            e
        })?;
        
        tracing::info!("[Doris] Successfully added SQL block rule {} to cluster {}", rule_name, self.cluster.name);
        Ok(())
    }

    async fn delete_sql_blacklist(&self, id: &str) -> ApiResult<()> {
        tracing::debug!("[Doris] Deleting SQL block rule {} from cluster: {}", id, self.cluster.name);
        
        let mysql_client = self.mysql_client().await?;
        
        // Doris uses DROP SQL_BLOCK_RULE rule_name
        let sql = format!("DROP SQL_BLOCK_RULE {}", id);
        
        mysql_client.execute(&sql).await.map_err(|e| {
            tracing::error!("[Doris] Failed to drop SQL block rule {} for cluster {}: {}", id, self.cluster.name, e);
            e
        })?;
        
        tracing::info!("[Doris] Successfully deleted SQL block rule {} from cluster {}", id, self.cluster.name);
        Ok(())
    }

    async fn show_proc_raw(&self, path: &str) -> ApiResult<Vec<Value>> {
        // Doris doesn't support SHOW PROC command
        // Map common paths to equivalent Doris commands
        let sql = match path.trim_start_matches('/') {
            "backends" => "SHOW BACKENDS",
            "frontends" => "SHOW FRONTENDS",
            "dbs" => "SHOW DATABASES",
            "current_queries" => "SHOW PROCESSLIST",
            "compactions" => {
                // Doris doesn't have direct compaction view, return empty
                tracing::warn!("Doris doesn't support SHOW PROC '/compactions', returning empty");
                return Ok(Vec::new());
            },
            path => {
                tracing::warn!("Unsupported SHOW PROC path for Doris: {}", path);
                return Err(ApiError::cluster_connection_failed(format!("SHOW PROC '{}' not supported for Doris", path)));
            },
        };

        let mysql_client = self.mysql_client().await?;
        mysql_client.query(sql).await
    }
}

