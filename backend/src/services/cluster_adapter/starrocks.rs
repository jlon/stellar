// StarRocks Adapter
// Purpose: Implement ClusterAdapter trait for StarRocks clusters

use super::ClusterAdapter;
use crate::models::{Backend, Cluster, ClusterType, Frontend, Query, RuntimeInfo};
use crate::services::{MySQLClient, MySQLPoolManager};
use crate::utils::{ApiError, ApiResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

pub struct StarRocksAdapter {
    pub http_client: Client,
    pub cluster: Cluster,
    mysql_pool_manager: Arc<MySQLPoolManager>,
}

impl StarRocksAdapter {
    pub fn new(cluster: Cluster, mysql_pool_manager: Arc<MySQLPoolManager>) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(cluster.connection_timeout as u64))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build HTTP client for cluster {}: {}", cluster.name, e);
                Client::default()
            });

        Self { http_client, cluster, mysql_pool_manager }
    }

    async fn mysql_client(&self) -> ApiResult<MySQLClient> {
        let pool = self.mysql_pool_manager.get_pool(&self.cluster).await?;
        Ok(MySQLClient::from_pool(pool))
    }

    fn normalize_proc_path(path: &str) -> String {
        if path.is_empty() {
            "/".to_string()
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        }
    }

    fn escape_proc_path(path: &str) -> String {
        path.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn build_show_proc_sql(path: &str) -> String {
        let normalized = Self::normalize_proc_path(path);
        let escaped = Self::escape_proc_path(&normalized);
        format!("SHOW PROC \"{}\"", escaped)
    }

    async fn show_proc_entities<T>(&self, path: &str) -> ApiResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let rows = self.show_proc_raw(path).await?;
        let mut entities = Vec::with_capacity(rows.len());

        for row in rows {
            match serde_json::from_value::<T>(row) {
                Ok(value) => entities.push(value),
                Err(e) => {
                    tracing::warn!("Failed to deserialize SHOW PROC '{}' row: {}", path, e);
                },
            }
        }

        Ok(entities)
    }

    /// Get compute nodes for shared-data architecture
    async fn get_compute_nodes(&self) -> ApiResult<Vec<Backend>> {
        let compute_nodes = self.show_proc_entities::<Backend>("/compute_nodes").await?;
        tracing::info!("Retrieved {} compute nodes (shared-data mode)", compute_nodes.len());
        Ok(compute_nodes)
    }
}

#[async_trait]
impl ClusterAdapter for StarRocksAdapter {
    fn cluster_type(&self) -> ClusterType {
        ClusterType::StarRocks
    }

    fn cluster(&self) -> &Cluster {
        &self.cluster
    }

    fn get_base_url(&self) -> String {
        let protocol = if self.cluster.enable_ssl { "https" } else { "http" };
        format!("{}://{}:{}", protocol, self.cluster.fe_host, self.cluster.fe_http_port)
    }

    async fn get_backends(&self) -> ApiResult<Vec<Backend>> {
        if self.cluster.is_shared_data() {
            tracing::info!(
                "Cluster {} is in shared-data mode, fetching compute nodes",
                self.cluster.name
            );
            return self.get_compute_nodes().await;
        }

        tracing::debug!(
            "Cluster {} is in shared-nothing mode, fetching backends",
            self.cluster.name
        );
        self.show_proc_entities::<Backend>("/backends").await
    }

    async fn get_frontends(&self) -> ApiResult<Vec<Frontend>> {
        tracing::debug!("Fetching frontends via MySQL SHOW PROC");
        self.show_proc_entities::<Frontend>("/frontends").await
    }

    async fn drop_backend(&self, host: &str, heartbeat_port: &str) -> ApiResult<()> {
        let sql = if self.cluster.is_shared_data() {
            format!("ALTER SYSTEM DROP COMPUTE NODE \"{}:{}\"", host, heartbeat_port)
        } else {
            format!("ALTER SYSTEM DROP BACKEND \"{}:{}\"", host, heartbeat_port)
        };

        tracing::info!(
            "Dropping {} node {}:{} from cluster {} (mode: {})",
            if self.cluster.is_shared_data() { "compute" } else { "backend" },
            host,
            heartbeat_port,
            self.cluster.name,
            self.cluster.deployment_mode
        );
        self.execute_sql(&sql).await
    }

    async fn get_sessions(&self) -> ApiResult<Vec<crate::models::Session>> {
        use crate::models::Session;

        let mysql_client = self.mysql_client().await?;
        let (_, rows) = mysql_client.query_raw("SHOW PROCESSLIST").await?;

        let mut sessions = Vec::new();
        for row in rows {
            if row.len() >= 7 {
                sessions.push(Session {
                    id: row.get(0).cloned().unwrap_or_default(),
                    user: row.get(1).cloned().unwrap_or_default(),
                    host: row.get(2).cloned().unwrap_or_default(),
                    db: row.get(3).cloned(),
                    command: row.get(4).cloned().unwrap_or_default(),
                    time: row.get(5).cloned().unwrap_or_else(|| "0".to_string()),
                    state: row.get(6).cloned().unwrap_or_default(),
                    info: row.get(7).cloned(),
                });
            }
        }

        Ok(sessions)
    }

    async fn get_queries(&self) -> ApiResult<Vec<Query>> {
        match self.show_proc_entities::<Query>("/current_queries").await {
            Ok(queries) => Ok(queries),
            Err(e) => {
                tracing::warn!(
                    "Failed to retrieve /current_queries via SHOW PROC: {}. Returning empty list.",
                    e
                );
                Ok(Vec::new())
            },
        }
    }

    async fn get_runtime_info(&self) -> ApiResult<RuntimeInfo> {
        let url = format!("{}/api/show_runtime_info", self.get_base_url());

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

        response.json().await.map_err(|e| {
            ApiError::cluster_connection_failed(format!("Failed to parse response: {}", e))
        })
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

        response.text().await.map_err(|e| {
            ApiError::cluster_connection_failed(format!("Failed to read response: {}", e))
        })
    }

    fn parse_prometheus_metrics(
        &self,
        metrics_text: &str,
    ) -> ApiResult<std::collections::HashMap<String, f64>> {
        let mut metrics = std::collections::HashMap::new();

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
        tracing::debug!("Executing SQL: {}", sql);

        let body = serde_json::json!({ "query": sql });

        let response = self
            .http_client
            .post(&url)
            .basic_auth(&self.cluster.username, Some(&self.cluster.password_encrypted))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Failed to execute SQL: {}", e);
                ApiError::cluster_connection_failed(format!("Request failed: {}", e))
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("SQL execution failed with status {}: {}", status, error_text);
            return Err(ApiError::cluster_connection_failed(format!(
                "SQL execution failed: {}",
                error_text
            )));
        }

        tracing::info!("SQL executed successfully: {}", sql);
        Ok(())
    }

    async fn list_catalogs(&self) -> ApiResult<Vec<String>> {
        let mysql_client = self.mysql_client().await?;
        let (_, rows) = mysql_client.query_raw("SHOW CATALOGS").await?;

        let mut catalogs = Vec::new();
        for row in rows {
            if let Some(catalog_name) = row.first() {
                let name = catalog_name.trim().to_string();
                if !name.is_empty() {
                    catalogs.push(name);
                }
            }
        }

        Ok(catalogs)
    }

    async fn list_databases(&self, catalog: Option<&str>) -> ApiResult<Vec<String>> {
        let mysql_client = self.mysql_client().await?;

        if let Some(cat) = catalog {
            let switch_sql = format!("SET CATALOG {}", cat);
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

        Ok(databases)
    }

    async fn list_materialized_views(
        &self,
        database: Option<&str>,
    ) -> ApiResult<Vec<crate::models::MaterializedView>> {
        use crate::services::MaterializedViewService;

        let mysql_client = self.mysql_client().await?;
        let mv_service = MaterializedViewService::new(mysql_client);
        mv_service.list_materialized_views(database).await
    }

    async fn get_materialized_view_ddl(&self, mv_name: &str) -> ApiResult<String> {
        use crate::services::MaterializedViewService;

        let mysql_client = self.mysql_client().await?;
        let mv_service = MaterializedViewService::new(mysql_client);
        mv_service.get_materialized_view_ddl(mv_name).await
    }

    async fn create_materialized_view(&self, ddl: &str) -> ApiResult<()> {
        use crate::services::MaterializedViewService;

        let mysql_client = self.mysql_client().await?;
        let mv_service = MaterializedViewService::new(mysql_client);
        mv_service.create_materialized_view(ddl).await
    }

    async fn drop_materialized_view(&self, mv_name: &str) -> ApiResult<()> {
        use crate::services::MaterializedViewService;

        let mysql_client = self.mysql_client().await?;
        let mv_service = MaterializedViewService::new(mysql_client);
        mv_service.drop_materialized_view(mv_name, false).await
    }

    async fn refresh_materialized_view(
        &self,
        mv_name: &str,
        partition_start: Option<&str>,
        partition_end: Option<&str>,
        force: bool,
        mode: &str,
    ) -> ApiResult<()> {
        use crate::services::MaterializedViewService;

        let mysql_client = self.mysql_client().await?;
        let mv_service = MaterializedViewService::new(mysql_client);
        mv_service
            .refresh_materialized_view(mv_name, partition_start, partition_end, force, mode)
            .await
    }

    async fn alter_materialized_view(&self, _mv_name: &str, ddl: &str) -> ApiResult<()> {
        let mysql_client = self.mysql_client().await?;
        mysql_client.execute(ddl).await?;
        Ok(())
    }

    async fn list_sql_blacklist(&self) -> ApiResult<Vec<crate::models::SqlBlacklistItem>> {
        use crate::models::SqlBlacklistItem;

        let mysql_client = self.mysql_client().await?;
        let rows = mysql_client.query("SHOW SQLBLACKLIST").await?;

        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let obj = row.as_object()?;
                Some(SqlBlacklistItem {
                    id: obj.get("Id")?.as_str()?.to_string(),
                    pattern: obj.get("Pattern")?.as_str()?.to_string(),
                })
            })
            .collect())
    }

    async fn add_sql_blacklist(&self, pattern: &str) -> ApiResult<()> {
        let mysql_client = self.mysql_client().await?;
        let escaped_pattern = pattern.replace('"', "\\\"");
        let sql = format!("ADD SQLBLACKLIST \"{}\"", escaped_pattern);
        mysql_client.execute(&sql).await?;
        Ok(())
    }

    async fn delete_sql_blacklist(&self, id: &str) -> ApiResult<()> {
        let mysql_client = self.mysql_client().await?;
        let sql = format!("DELETE SQLBLACKLIST {}", id);
        mysql_client.execute(&sql).await?;
        Ok(())
    }

    async fn show_proc_raw(&self, path: &str) -> ApiResult<Vec<Value>> {
        let sql = Self::build_show_proc_sql(path);
        let mysql_client = self.mysql_client().await?;
        mysql_client.query(&sql).await
    }

    async fn list_profiles(&self) -> ApiResult<Vec<crate::models::ProfileListItem>> {
        use crate::models::ProfileListItem;

        let mysql_client = self.mysql_client().await?;
        let (columns, rows) = mysql_client.query_raw("SHOW PROFILELIST").await?;

        tracing::info!(
            "Profile list query returned {} rows with {} columns",
            rows.len(),
            columns.len()
        );

        let profiles: Vec<ProfileListItem> = rows
            .into_iter()
            .filter(|row| {
                let state = row.get(3).map(|s| s.as_str()).unwrap_or("");
                !state.eq_ignore_ascii_case("aborted")
            })
            .map(|row| ProfileListItem {
                query_id: row.first().cloned().unwrap_or_default(),
                start_time: row.get(1).cloned().unwrap_or_default(),
                time: row.get(2).cloned().unwrap_or_default(),
                state: row.get(3).cloned().unwrap_or_default(),
                statement: row.get(4).cloned().unwrap_or_default(),
            })
            .collect();

        tracing::info!("Successfully converted {} profiles (Aborted filtered)", profiles.len());
        Ok(profiles)
    }

    async fn get_profile(&self, query_id: &str) -> ApiResult<String> {
        let mysql_client = self.mysql_client().await?;
        let sql = format!("SELECT get_query_profile('{}')", query_id);
        let (_, rows) = mysql_client.query_raw(&sql).await?;

        let profile_content = rows
            .first()
            .and_then(|row| row.first())
            .cloned()
            .unwrap_or_default();

        if profile_content.trim().is_empty() {
            return Err(ApiError::not_found(format!("Profile not found for query: {}", query_id)));
        }

        Ok(profile_content)
    }
}
