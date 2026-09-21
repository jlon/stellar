use crate::db::AppDb;
use crate::db::dialect::RowsAffected;
use crate::db::query as db_query;
use crate::models::{
    Cluster, ClusterHealth, CreateClusterRequest, HealthCheck, HealthStatus, UpdateClusterRequest,
};
use crate::services::{MySQLPoolManager, create_adapter};
use crate::utils::{ApiError, ApiResult};
use chrono::Utc;
use sqlx::Pool;
use std::sync::{Arc, RwLock};
use stellar_macros::app_impl;

#[derive(Clone)]
pub struct ClusterService<DB: AppDb> {
    pool: Pool<DB>,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    active_cluster: Arc<RwLock<Option<Cluster>>>,
}

/// Convert raw error messages into user-friendly messages for health checks
fn simplify_health_check_error(error: &str) -> String {
    let error_lower = error.to_lowercase();

    // Check error patterns using if-else
    if error_lower.contains("28000") || error_lower.contains("access denied") {
        return "认证失败: 请检查用户名和密码是否正确；普通用户需确认有 SHOW PROC 等基础权限"
            .to_string();
    }

    if error_lower.contains("connection refused")
        || error_lower.contains("refused")
        || error_lower.contains("cannot connect")
    {
        return "无法连接: 请检查集群地址和端口是否正确；可用 telnet 验证 FE 查询端口连通性"
            .to_string();
    }

    if error_lower.contains("timeout") {
        return "连接超时: 请检查安全组/防火墙是否放行，以及集群是否正常运行".to_string();
    }

    if error_lower.contains("unknown host") || error_lower.contains("resolve") {
        return "解析失败: 无法解析集群地址；内网地址需确认平台与集群网络互通".to_string();
    }

    // Default: return a generic message with error code if available
    error
        .find("ERROR ")
        .and_then(|code_start| {
            error[code_start..].find(':').map(|code_end| {
                let error_code = &error[code_start + 6..code_start + code_end];
                format!("连接失败 ({}): 请检查集群配置", error_code)
            })
        })
        .unwrap_or_else(|| "连接失败: 请检查集群配置".to_string())
}

#[app_impl]
impl<DB: AppDb> ClusterService<DB> {
    pub fn new(pool: Pool<DB>, mysql_pool_manager: Arc<MySQLPoolManager>) -> Self {
        Self { pool, mysql_pool_manager, active_cluster: Arc::new(RwLock::new(None)) }
    }

    pub fn cached_active_cluster(&self) -> Option<Cluster> {
        self.active_cluster.read().ok()?.clone()
    }

    fn remember_active(&self, cluster: &Cluster) {
        if let Ok(mut guard) = self.active_cluster.write() {
            *guard = Some(cluster.clone());
        }
    }

    fn clear_active(&self) {
        if let Ok(mut guard) = self.active_cluster.write() {
            *guard = None;
        }
    }

    pub async fn create_cluster(
        &self,
        mut req: CreateClusterRequest,
        user_id: i64,
        requestor_org: Option<i64>,
        is_super_admin: bool,
    ) -> ApiResult<Cluster> {
        req.name = req.name.trim().to_string();
        req.fe_host = req.fe_host.trim().to_string();
        req.username = req.username.trim().to_string();
        req.catalog = req.catalog.trim().to_string();
        if let Some(ref mut desc) = req.description {
            *desc = desc.trim().to_string();
        }

        if req.name.is_empty() {
            return Err(ApiError::validation_error("Cluster name cannot be empty"));
        }
        if req.fe_host.is_empty() {
            return Err(ApiError::validation_error("FE host cannot be empty"));
        }
        if req.username.is_empty() {
            return Err(ApiError::validation_error("Username cannot be empty"));
        }

        let existing: Option<Cluster> = db_query::query_as("SELECT * FROM clusters WHERE name = ?")
            .bind(&req.name)
            .fetch_optional(&self.pool)
            .await?;

        if existing.is_some() {
            return Err(ApiError::validation_error("Cluster name already exists"));
        }

        let target_org_id = self
            .resolve_target_org(req.organization_id, requestor_org, is_super_admin)
            .await?;

        let tags_json = req
            .tags
            .map(|t| serde_json::to_string(&t).unwrap_or_default());

        let existing_cluster_count: (i64,) =
            db_query::query_as("SELECT COUNT(*) FROM clusters WHERE organization_id = ?")
                .bind(target_org_id)
                .fetch_one(&self.pool)
                .await?;

        let is_first_cluster = existing_cluster_count.0 == 0;

        let cluster_id = db_query::query(
            "INSERT INTO clusters (name, description, fe_host, fe_http_port, fe_query_port, 
             username, password_encrypted, enable_ssl, connection_timeout, tags, catalog, 
             is_active, created_by, organization_id, deployment_mode, cluster_type, admin_user, admin_password_encrypted)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&req.name)
        .bind(&req.description)
        .bind(&req.fe_host)
        .bind(req.fe_http_port)
        .bind(req.fe_query_port)
        .bind(&req.username)
        .bind(&req.password)
        .bind(req.enable_ssl)
        .bind(req.connection_timeout)
        .bind(&tags_json)
        .bind(&req.catalog)
        .bind(is_first_cluster)
        .bind(user_id)
        .bind(target_org_id)
        .bind(req.deployment_mode.to_string())
        .bind(req.cluster_type.to_string())
        .bind(&req.admin_user)
        .bind(&req.admin_password)
        .insert_id(&self.pool)
        .await?;

        if !is_first_cluster {
            let active_count: (i64,) = db_query::query_as(
                "SELECT COUNT(*) FROM clusters WHERE is_active = TRUE AND organization_id = ?",
            )
            .bind(target_org_id)
            .fetch_one(&self.pool)
            .await?;

            if active_count.0 == 0 {
                db_query::query("UPDATE clusters SET is_active = TRUE WHERE id = ?")
                    .bind(cluster_id)
                    .execute(&self.pool)
                    .await?;
                tracing::info!(
                    "Automatically activated newly created cluster for organization {} (no active cluster existed)",
                    target_org_id
                );
            }
        }

        let cluster: Cluster = db_query::query_as("SELECT * FROM clusters WHERE id = ?")
            .bind(cluster_id)
            .fetch_one(&self.pool)
            .await?;

        tracing::info!("Cluster created successfully: {} (ID: {})", cluster.name, cluster.id);
        tracing::debug!(
            "Cluster details: host={}, port={}, ssl={}, catalog={}, active={}",
            cluster.fe_host,
            cluster.fe_http_port,
            cluster.enable_ssl,
            cluster.catalog,
            cluster.is_active
        );

        Ok(cluster)
    }

    pub async fn list_clusters(&self) -> ApiResult<Vec<Cluster>> {
        let clusters: Vec<Cluster> =
            db_query::query_as("SELECT * FROM clusters ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?;

        Ok(clusters)
    }

    pub async fn get_cluster(&self, cluster_id: i64) -> ApiResult<Cluster> {
        let cluster: Option<Cluster> = db_query::query_as("SELECT * FROM clusters WHERE id = ?")
            .bind(cluster_id)
            .fetch_optional(&self.pool)
            .await?;

        cluster.ok_or_else(|| ApiError::cluster_not_found(cluster_id))
    }

    pub async fn get_active_cluster(&self) -> ApiResult<Cluster> {
        if let Some(cluster) = self.cached_active_cluster() {
            return Ok(cluster);
        }
        let cluster: Option<Cluster> =
            db_query::query_as("SELECT * FROM clusters WHERE is_active = TRUE LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;

        let cluster = cluster.ok_or_else(|| {
            ApiError::not_found("No active cluster found. Please activate a cluster first.")
        })?;
        self.remember_active(&cluster);
        Ok(cluster)
    }

    pub async fn get_active_cluster_by_org(&self, org_id: Option<i64>) -> ApiResult<Cluster> {
        if let Some(cluster) = self.cached_active_cluster() {
            if org_id.is_some() && cluster.organization_id == org_id {
                return Ok(cluster);
            }
        }
        let cluster: Option<Cluster> = if let Some(org) = org_id {
            db_query::query_as(
                "SELECT * FROM clusters WHERE is_active = TRUE AND organization_id = ? LIMIT 1",
            )
            .bind(org)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };

        let cluster = cluster.ok_or_else(|| {
            ApiError::not_found(
                "No active cluster found for your organization. Please activate a cluster first.",
            )
        })?;
        self.remember_active(&cluster);
        Ok(cluster)
    }

    pub async fn set_active_cluster(&self, cluster_id: i64) -> ApiResult<Cluster> {
        let cluster = self.get_cluster(cluster_id).await?;
        let org_id = cluster.organization_id;

        let mut tx = self.pool.begin().await?;

        if let Some(org) = org_id {
            db_query::query("UPDATE clusters SET is_active = FALSE WHERE organization_id = ?")
                .bind(org)
                .execute(&mut *tx)
                .await?;
        } else {
            db_query::query("UPDATE clusters SET is_active = FALSE WHERE organization_id IS NULL")
                .execute(&mut *tx)
                .await?;
        }

        db_query::query(
            "UPDATE clusters SET is_active = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(cluster_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!("Cluster activated: ID {} (org: {:?})", cluster_id, org_id);

        let cluster = self.get_cluster(cluster_id).await?;
        self.remember_active(&cluster);
        Ok(cluster)
    }

    pub async fn update_cluster(
        &self,
        cluster_id: i64,
        req: UpdateClusterRequest,
    ) -> ApiResult<Cluster> {
        let _cluster = self.get_cluster(cluster_id).await?;

        let mut updates = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(name) = &req.name {
            updates.push("name = ?");
            params.push(name.clone());
        }
        if let Some(desc) = &req.description {
            updates.push("description = ?");
            params.push(desc.clone());
        }
        if let Some(host) = &req.fe_host {
            updates.push("fe_host = ?");
            params.push(host.clone());
        }
        if let Some(http_port) = req.fe_http_port {
            updates.push("fe_http_port = ?");
            params.push(http_port.to_string());
        }
        if let Some(query_port) = req.fe_query_port {
            updates.push("fe_query_port = ?");
            params.push(query_port.to_string());
        }
        if let Some(username) = &req.username {
            updates.push("username = ?");
            params.push(username.clone());
        }
        if let Some(password) = &req.password {
            updates.push("password_encrypted = ?");
            params.push(password.clone());
        }
        if let Some(ssl) = req.enable_ssl {
            updates.push("enable_ssl = ?");
            params.push((ssl as i32).to_string());
        }
        if let Some(timeout) = req.connection_timeout {
            updates.push("connection_timeout = ?");
            params.push(timeout.to_string());
        }
        if let Some(tags) = &req.tags {
            updates.push("tags = ?");
            params.push(serde_json::to_string(tags).unwrap_or_default());
        }
        if let Some(catalog) = &req.catalog {
            updates.push("catalog = ?");
            params.push(catalog.clone());
        }
        if let Some(org_id) = req.organization_id {
            updates.push("organization_id = ?");
            params.push(org_id.to_string());
        }
        if let Some(mode) = &req.deployment_mode {
            updates.push("deployment_mode = ?");
            params.push(mode.to_string());
        }
        if let Some(cluster_type) = &req.cluster_type {
            updates.push("cluster_type = ?");
            params.push(cluster_type.to_string());
        }
        if let Some(admin_user) = &req.admin_user {
            updates.push("admin_user = ?");
            params.push(admin_user.clone());
        }
        // Only update admin_password_encrypted if password is provided (not None)
        // If None, keep existing value
        if let Some(admin_password) = &req.admin_password {
            updates.push("admin_password_encrypted = ?");
            params.push(admin_password.clone());
        }

        if updates.is_empty() {
            return self.get_cluster(cluster_id).await;
        }

        updates.push("updated_at = CURRENT_TIMESTAMP");

        let sql = format!("UPDATE clusters SET {} WHERE id = ?", updates.join(", "));

        let mut query = db_query::query(&sql);
        for param in params {
            query = query.bind(param);
        }
        query = query.bind(cluster_id);

        query.execute(&self.pool).await?;

        tracing::info!("Cluster updated: ID {}", cluster_id);

        let updated = self.get_cluster(cluster_id).await?;

        // Keep the active-cluster cache coherent: deployment_mode / connection
        // changes must take effect immediately (feature cards, node management).
        if let Ok(mut guard) = self.active_cluster.write() {
            if guard.as_ref().map(|c| c.id) == Some(cluster_id) {
                *guard = Some(updated.clone());
            }
        }

        Ok(updated)
    }

    pub async fn delete_cluster(&self, cluster_id: i64) -> ApiResult<()> {
        let cluster_record: Option<(bool, Option<i64>)> =
            db_query::query_as("SELECT is_active, organization_id FROM clusters WHERE id = ?")
                .bind(cluster_id)
                .fetch_optional(&self.pool)
                .await?;

        let Some((is_active, cluster_org_id)) = cluster_record else {
            return Err(ApiError::cluster_not_found(cluster_id));
        };

        let mut tx = self.pool.begin().await?;
        db_query::query("DELETE FROM permission_requests WHERE cluster_id = ?")
            .bind(cluster_id)
            .execute(&mut *tx)
            .await?;
        let result = db_query::query("DELETE FROM clusters WHERE id = ?")
            .bind(cluster_id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ApiError::cluster_not_found(cluster_id));
        }
        tx.commit().await?;

        tracing::info!("Cluster deleted: ID {}", cluster_id);

        let cached_is_deleted = self
            .active_cluster
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|cluster| cluster.id == cluster_id))
            .unwrap_or(false);
        if cached_is_deleted {
            self.clear_active();
        }

        if is_active {
            let next_cluster: Option<(i64,)> = if let Some(org_id) = cluster_org_id {
                db_query::query_as(
                    "SELECT id FROM clusters WHERE organization_id = ? ORDER BY created_at DESC LIMIT 1",
                )
                .bind(org_id)
                .fetch_optional(&self.pool)
                .await?
            } else {
                db_query::query_as(
                    "SELECT id FROM clusters WHERE organization_id IS NULL ORDER BY created_at DESC LIMIT 1",
                )
                .fetch_optional(&self.pool)
                .await?
            };

            if let Some((next_id,)) = next_cluster {
                db_query::query("UPDATE clusters SET is_active = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                    .bind(next_id)
                    .execute(&self.pool)
                    .await?;
                tracing::info!("Automatically activated cluster ID {} after deletion", next_id);
                let next = self.get_cluster(next_id).await?;
                self.remember_active(&next);
            }
        }

        Ok(())
    }

    async fn resolve_target_org(
        &self,
        requested_org: Option<i64>,
        requestor_org: Option<i64>,
        is_super_admin: bool,
    ) -> ApiResult<i64> {
        if is_super_admin {
            if let Some(id) = requested_org.or(requestor_org) {
                return Ok(id);
            }
            return self.fetch_default_org_id().await;
        }

        requestor_org.ok_or_else(|| {
            ApiError::forbidden("Organization context required for cluster operations")
        })
    }

    async fn fetch_default_org_id(&self) -> ApiResult<i64> {
        if let Some(id) =
            db_query::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_optional(&self.pool)
                .await?
        {
            return Ok(id);
        }

        db_query::query("INSERT INTO organizations (code, name, description, is_system) VALUES ('default_org', 'Default Organization', 'Auto-created default organization', TRUE)")
            .execute(&self.pool)
            .await?;

        db_query::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Default organization not found"))
    }

    pub async fn get_cluster_health(&self, cluster_id: i64) -> ApiResult<ClusterHealth> {
        let cluster = self.get_cluster(cluster_id).await?;
        let is_shared_data = cluster.is_shared_data();
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());

        let mut checks = Vec::new();
        let mut overall_status = HealthStatus::Healthy;

        match adapter.get_frontends().await {
            Ok(frontends) => {
                let alive_count = frontends.iter().filter(|f| f.alive == "true").count();
                let total_count = frontends.len();

                if total_count == 0 {
                    checks.push(HealthCheck {
                        name: "Frontend Nodes".to_string(),
                        status: "critical".to_string(),
                        message: "No FE nodes found".to_string(),
                    });
                    overall_status = HealthStatus::Critical;
                } else if alive_count == total_count {
                    checks.push(HealthCheck {
                        name: "Frontend Nodes".to_string(),
                        status: "ok".to_string(),
                        message: format!("All {} FE nodes are online", total_count),
                    });
                } else if alive_count > 0 {
                    checks.push(HealthCheck {
                        name: "Frontend Nodes".to_string(),
                        status: "warning".to_string(),
                        message: format!("{}/{} FE nodes are online", alive_count, total_count),
                    });
                    if overall_status == HealthStatus::Healthy {
                        overall_status = HealthStatus::Warning;
                    }
                } else {
                    checks.push(HealthCheck {
                        name: "Frontend Nodes".to_string(),
                        status: "critical".to_string(),
                        message: "No FE nodes are online".to_string(),
                    });
                    overall_status = HealthStatus::Critical;
                }
            },
            Err(e) => {
                checks.push(HealthCheck {
                    name: "Frontend Nodes".to_string(),
                    status: "critical".to_string(),
                    message: format!("Failed to check FE nodes: {}", e),
                });
                overall_status = HealthStatus::Critical;
            },
        }

        let node_type = if is_shared_data { "CN" } else { "BE" };
        match adapter.get_backends().await {
            Ok(backends) => {
                let alive_count = backends.iter().filter(|b| b.alive == "true").count();
                let total_count = backends.len();

                if total_count == 0 {
                    checks.push(HealthCheck {
                        name: "Compute Nodes".to_string(),
                        status: "critical".to_string(),
                        message: format!("No {} nodes found", node_type),
                    });
                    overall_status = HealthStatus::Critical;
                } else if alive_count == total_count {
                    checks.push(HealthCheck {
                        name: "Compute Nodes".to_string(),
                        status: "ok".to_string(),
                        message: format!("All {} {} nodes are online", total_count, node_type),
                    });
                } else if alive_count > 0 {
                    checks.push(HealthCheck {
                        name: "Compute Nodes".to_string(),
                        status: "warning".to_string(),
                        message: format!(
                            "{}/{} {} nodes are online",
                            alive_count, total_count, node_type
                        ),
                    });
                    if overall_status == HealthStatus::Healthy {
                        overall_status = HealthStatus::Warning;
                    }
                } else {
                    checks.push(HealthCheck {
                        name: "Compute Nodes".to_string(),
                        status: "critical".to_string(),
                        message: format!("No {} nodes are online", node_type),
                    });
                    overall_status = HealthStatus::Critical;
                }
            },
            Err(e) => {
                checks.push(HealthCheck {
                    name: "Compute Nodes".to_string(),
                    status: "warning".to_string(),
                    message: format!("Failed to check {} nodes: {}", node_type, e),
                });
                if overall_status == HealthStatus::Healthy {
                    overall_status = HealthStatus::Warning;
                }
            },
        }

        Ok(ClusterHealth { status: overall_status, checks, last_check_time: Utc::now() })
    }

    pub async fn get_cluster_health_for_cluster(
        &self,
        cluster: &Cluster,
    ) -> ApiResult<ClusterHealth> {
        use crate::services::MySQLClient;

        let mut checks = Vec::new();
        let mut overall_status = HealthStatus::Healthy;

        match self.mysql_pool_manager.get_pool(cluster).await {
            Ok(pool) => {
                let mysql_client = MySQLClient::from_pool(pool);

                match mysql_client.query("SELECT 1").await {
                    Ok(_) => {
                        checks.push(HealthCheck {
                            name: "Database Connection".to_string(),
                            status: "ok".to_string(),
                            message: "Connection successful".to_string(),
                        });

                        let adapter =
                            create_adapter(cluster.clone(), self.mysql_pool_manager.clone());
                        match adapter.get_runtime_info().await {
                            Ok(_) => {
                                checks.push(HealthCheck {
                                    name: "FE Availability".to_string(),
                                    status: "ok".to_string(),
                                    message: "FE is reachable and responding".to_string(),
                                });
                            },
                            Err(e) => {
                                checks.push(HealthCheck {
                                    name: "FE Availability".to_string(),
                                    status: "warning".to_string(),
                                    message: format!("FE HTTP check failed: {}", e),
                                });
                                if overall_status == HealthStatus::Healthy {
                                    overall_status = HealthStatus::Warning;
                                }
                            },
                        }

                        let node_type = if cluster.is_shared_data() { "CN" } else { "BE" };
                        match adapter.get_backends().await {
                            Ok(backends) => {
                                let alive_count =
                                    backends.iter().filter(|b| b.alive == "true").count();
                                let total_count = backends.len();

                                if total_count == 0 {
                                    checks.push(HealthCheck {
                                        name: "Compute Nodes".to_string(),
                                        status: "warning".to_string(),
                                        message: format!("No {} nodes found", node_type),
                                    });
                                    if overall_status == HealthStatus::Healthy {
                                        overall_status = HealthStatus::Warning;
                                    }
                                } else if alive_count == total_count {
                                    checks.push(HealthCheck {
                                        name: "Compute Nodes".to_string(),
                                        status: "ok".to_string(),
                                        message: format!(
                                            "All {} {} nodes are online",
                                            total_count, node_type
                                        ),
                                    });
                                } else if alive_count > 0 {
                                    checks.push(HealthCheck {
                                        name: "Compute Nodes".to_string(),
                                        status: "warning".to_string(),
                                        message: format!(
                                            "{}/{} {} nodes are online",
                                            alive_count, total_count, node_type
                                        ),
                                    });
                                    if overall_status == HealthStatus::Healthy {
                                        overall_status = HealthStatus::Warning;
                                    }
                                } else {
                                    checks.push(HealthCheck {
                                        name: "Compute Nodes".to_string(),
                                        status: "critical".to_string(),
                                        message: format!("No {} nodes are online", node_type),
                                    });
                                    overall_status = HealthStatus::Critical;
                                }
                            },
                            Err(e) => {
                                let error_msg = simplify_health_check_error(&e.to_string());
                                checks.push(HealthCheck {
                                    name: "Compute Nodes".to_string(),
                                    status: "warning".to_string(),
                                    message: format!(
                                        "Failed to check {} nodes: {}",
                                        node_type, error_msg
                                    ),
                                });
                                if overall_status == HealthStatus::Healthy {
                                    overall_status = HealthStatus::Warning;
                                }
                            },
                        }
                    },
                    Err(e) => {
                        let error_msg = simplify_health_check_error(&e.to_string());
                        checks.push(HealthCheck {
                            name: "Database Connection".to_string(),
                            status: "critical".to_string(),
                            message: error_msg,
                        });
                        overall_status = HealthStatus::Critical;
                    },
                }
            },
            Err(e) => {
                let error_msg = simplify_health_check_error(&e.to_string());
                checks.push(HealthCheck {
                    name: "Connection Pool".to_string(),
                    status: "critical".to_string(),
                    message: error_msg,
                });
                overall_status = HealthStatus::Critical;
            },
        }

        Ok(ClusterHealth { status: overall_status, checks, last_check_time: Utc::now() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
    use std::time::Duration;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("test db");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("enable foreign keys");
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    #[tokio::test]
    async fn delete_cluster_with_permission_request_succeeds() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO organizations (code, name, description, is_system) VALUES ('org', 'Org', '', 0)",
        )
        .execute(&pool)
        .await
        .expect("org");
        sqlx::query(
            "INSERT INTO users (username, password_hash, email, organization_id) VALUES ('u1', 'x', 'u1@t.com', 1)",
        )
        .execute(&pool)
        .await
        .expect("user");
        sqlx::query(
            "INSERT INTO clusters (name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, deployment_mode, cluster_type, is_active, organization_id)
             VALUES ('c1', '127.0.0.1', 8030, 9030, 'root', 'p', 'default_catalog', 'shared_nothing', 'starrocks', 0, 1)",
        )
        .execute(&pool)
        .await
        .expect("cluster");
        sqlx::query(
            "INSERT INTO permission_requests (cluster_id, applicant_id, applicant_org_id, request_type, request_details, reason, status)
             VALUES (1, 1, 1, 'grant_permission', '{}', 'test', 'failed')",
        )
        .execute(&pool)
        .await
        .expect("permission request");

        let service = ClusterService::new(pool.clone(), Arc::new(MySQLPoolManager::new()));
        service
            .delete_cluster(1)
            .await
            .expect("delete should succeed");

        let clusters: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clusters WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("count clusters");
        let requests: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM permission_requests WHERE cluster_id = 1")
                .fetch_one(&pool)
                .await
                .expect("count requests");
        assert_eq!(clusters.0, 0);
        assert_eq!(requests.0, 0);
    }
}
