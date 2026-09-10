use crate::models::{
    Cluster, ClusterHealth, CreateClusterRequest, HealthCheck, HealthStatus, UpdateClusterRequest,
};
use crate::services::sr_physical::{DeploymentCipher, EncryptedSecret};
use crate::services::{MySQLPoolManager, create_adapter};
use crate::utils::{ApiError, ApiResult};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct ClusterService {
    pool: SqlitePool,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    password_cipher: Option<DeploymentCipher>,
}

/// Convert raw error messages into user-friendly messages for health checks
fn simplify_health_check_error(error: &str) -> String {
    let error_lower = error.to_lowercase();

    // Check error patterns using if-else
    if error_lower.contains("28000") || error_lower.contains("access denied") {
        return "认证失败: 请检查用户名和密码是否正确".to_string();
    }

    if error_lower.contains("connection refused") || error_lower.contains("refused") || error_lower.contains("cannot connect") {
        return "无法连接: 请检查集群地址和端口是否正确".to_string();
    }

    if error_lower.contains("timeout") {
        return "连接超时: 请检查网络连接和集群状态".to_string();
    }

    if error_lower.contains("unknown host") || error_lower.contains("resolve") {
        return "解析失败: 无法解析集群地址，请检查是否输入正确".to_string();
    }

    // Default: return a generic message with error code if available
    error.find("ERROR ")
        .and_then(|code_start| {
            error[code_start..].find(':').map(|code_end| {
                let error_code = &error[code_start + 6..code_start + code_end];
                format!("连接失败 ({}): 请检查集群配置", error_code)
            })
        })
        .unwrap_or_else(|| "连接失败: 请检查集群配置".to_string())
}

fn cluster_password_aad(organization_id: i64, kind: &str) -> String {
    format!("stellar:cluster:{organization_id}:{kind}")
}

impl ClusterService {
    pub fn new(pool: SqlitePool, mysql_pool_manager: Arc<MySQLPoolManager>) -> Self {
        Self { pool, mysql_pool_manager, password_cipher: None }
    }

    pub fn with_password_encryption_key(mut self, encryption_key: &str) -> Self {
        self.password_cipher = DeploymentCipher::from_key_material(encryption_key).ok();
        self
    }

    pub async fn create_cluster(
        &self,
        req: CreateClusterRequest,
        user_id: i64,
        requestor_org: Option<i64>,
        is_super_admin: bool,
    ) -> ApiResult<Cluster> {
        self.create_cluster_with_activation(req, user_id, requestor_org, is_super_admin, true)
            .await
    }

    pub async fn create_cluster_with_activation(
        &self,
        mut req: CreateClusterRequest,
        user_id: i64,
        requestor_org: Option<i64>,
        is_super_admin: bool,
        activate: bool,
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

        let existing: Option<Cluster> = sqlx::query_as("SELECT * FROM clusters WHERE name = ?")
            .bind(&req.name)
            .fetch_optional(&self.pool)
            .await?;

        if existing.is_some() {
            return Err(ApiError::validation_error("Cluster name already exists"));
        }

        let target_org_id = self
            .resolve_target_org(req.organization_id, requestor_org, is_super_admin)
            .await?;
        req.password = self.encrypt_password(&req.password, target_org_id, "password")?;
        req.admin_password = req
            .admin_password
            .as_deref()
            .map(|password| self.encrypt_password(password, target_org_id, "admin_password"))
            .transpose()?;

        let tags_json = req
            .tags
            .map(|t| serde_json::to_string(&t).unwrap_or_default());

        let existing_cluster_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM clusters WHERE organization_id = ?")
                .bind(target_org_id)
                .fetch_one(&self.pool)
                .await?;

        let is_first_cluster = existing_cluster_count.0 == 0;

        let result = sqlx::query(
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
        .bind(if is_first_cluster && activate { 1 } else { 0 })
        .bind(user_id)
        .bind(target_org_id)
        .bind(req.deployment_mode.to_string())
        .bind(req.cluster_type.to_string())
        .bind(&req.admin_user)
        .bind(&req.admin_password)
        .execute(&self.pool)
        .await?;

        let cluster_id = result.last_insert_rowid();

        if activate && !is_first_cluster {
            let active_count: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM clusters WHERE is_active = 1 AND organization_id = ?",
            )
            .bind(target_org_id)
            .fetch_one(&self.pool)
            .await?;

            if active_count.0 == 0 {
                sqlx::query("UPDATE clusters SET is_active = 1 WHERE id = ?")
                    .bind(cluster_id)
                    .execute(&self.pool)
                    .await?;
                tracing::info!(
                    "Automatically activated newly created cluster for organization {} (no active cluster existed)",
                    target_org_id
                );
            }
        }

        let cluster: Cluster = sqlx::query_as("SELECT * FROM clusters WHERE id = ?")
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

        self.decrypt_cluster(cluster)
    }

    pub async fn list_clusters(&self) -> ApiResult<Vec<Cluster>> {
        let clusters: Vec<Cluster> =
            sqlx::query_as("SELECT * FROM clusters ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await?;

        clusters
            .into_iter()
            .map(|cluster| self.decrypt_cluster(cluster))
            .collect()
    }

    pub async fn get_cluster(&self, cluster_id: i64) -> ApiResult<Cluster> {
        let cluster: Option<Cluster> = sqlx::query_as("SELECT * FROM clusters WHERE id = ?")
            .bind(cluster_id)
            .fetch_optional(&self.pool)
            .await?;

        cluster
            .ok_or_else(|| ApiError::cluster_not_found(cluster_id))
            .and_then(|cluster| self.decrypt_cluster(cluster))
    }

    pub async fn get_active_cluster(&self) -> ApiResult<Cluster> {
        let cluster: Option<Cluster> =
            sqlx::query_as("SELECT * FROM clusters WHERE is_active = 1 LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;

        let cluster = cluster.ok_or_else(|| {
            ApiError::not_found("No active cluster found. Please activate a cluster first.")
        })?;
        self.decrypt_cluster(cluster)
    }

    pub async fn get_active_cluster_by_org(&self, org_id: Option<i64>) -> ApiResult<Cluster> {
        let cluster: Option<Cluster> = if let Some(org) = org_id {
            sqlx::query_as(
                "SELECT * FROM clusters WHERE is_active = 1 AND organization_id = ? LIMIT 1",
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
        self.decrypt_cluster(cluster)
    }

    pub async fn set_active_cluster(&self, cluster_id: i64) -> ApiResult<Cluster> {
        let cluster = self.get_cluster(cluster_id).await?;
        let org_id = cluster.organization_id;

        let mut tx = self.pool.begin().await?;

        if let Some(org) = org_id {
            sqlx::query("UPDATE clusters SET is_active = 0 WHERE organization_id = ?")
                .bind(org)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("UPDATE clusters SET is_active = 0 WHERE organization_id IS NULL")
                .execute(&mut *tx)
                .await?;
        }

        sqlx::query(
            "UPDATE clusters SET is_active = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(cluster_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!("Cluster activated: ID {} (org: {:?})", cluster_id, org_id);

        self.get_cluster(cluster_id).await
    }

    pub async fn update_cluster(
        &self,
        cluster_id: i64,
        req: UpdateClusterRequest,
    ) -> ApiResult<Cluster> {
        let cluster = self.get_cluster(cluster_id).await?;

        let credential_organization_id = req
            .organization_id
            .or(cluster.organization_id)
            .unwrap_or_default();
        let organization_changed =
            req.organization_id.is_some() && req.organization_id != cluster.organization_id;
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
            params.push(self.encrypt_password(password, credential_organization_id, "password")?);
        } else if organization_changed && self.password_cipher.is_some() {
            updates.push("password_encrypted = ?");
            params.push(self.encrypt_password(
                &cluster.password_encrypted,
                credential_organization_id,
                "password",
            )?);
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
            params.push(self.encrypt_password(
                admin_password,
                credential_organization_id,
                "admin_password",
            )?);
        } else if organization_changed
            && self.password_cipher.is_some()
            && let Some(admin_password) = cluster.admin_password_encrypted.as_deref()
        {
            updates.push("admin_password_encrypted = ?");
            params.push(self.encrypt_password(
                admin_password,
                credential_organization_id,
                "admin_password",
            )?);
        }

        if updates.is_empty() {
            return self.get_cluster(cluster_id).await;
        }

        updates.push("updated_at = CURRENT_TIMESTAMP");

        let sql = format!("UPDATE clusters SET {} WHERE id = ?", updates.join(", "));

        let mut query = sqlx::query(&sql);
        for param in params {
            query = query.bind(param);
        }
        query = query.bind(cluster_id);

        query.execute(&self.pool).await?;

        tracing::info!("Cluster updated: ID {}", cluster_id);

        self.get_cluster(cluster_id).await
    }

    pub async fn delete_cluster(&self, cluster_id: i64) -> ApiResult<()> {
        let cluster_record: Option<(bool, Option<i64>)> =
            sqlx::query_as("SELECT is_active, organization_id FROM clusters WHERE id = ?")
                .bind(cluster_id)
                .fetch_optional(&self.pool)
                .await?;

        let is_active = cluster_record.map(|r| r.0).unwrap_or(false);
        let cluster_org_id = cluster_record.and_then(|r| r.1);

        let result = sqlx::query("DELETE FROM clusters WHERE id = ?")
            .bind(cluster_id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::cluster_not_found(cluster_id));
        }

        tracing::info!("Cluster deleted: ID {}", cluster_id);

        if is_active {
            let next_cluster: Option<(i64,)> = if let Some(org_id) = cluster_org_id {
                sqlx::query_as(
                    "SELECT id FROM clusters WHERE organization_id = ? ORDER BY created_at DESC LIMIT 1",
                )
                .bind(org_id)
                .fetch_optional(&self.pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT id FROM clusters WHERE organization_id IS NULL ORDER BY created_at DESC LIMIT 1",
                )
                .fetch_optional(&self.pool)
                .await?
            };

            if let Some((next_id,)) = next_cluster {
                sqlx::query("UPDATE clusters SET is_active = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
                    .bind(next_id)
                    .execute(&self.pool)
                    .await?;
                tracing::info!("Automatically activated cluster ID {} after deletion", next_id);
            }
        }

        Ok(())
    }

    fn encrypt_password(
        &self,
        password: &str,
        organization_id: i64,
        kind: &str,
    ) -> ApiResult<String> {
        let Some(cipher) = &self.password_cipher else {
            return Ok(password.to_owned());
        };
        let encrypted = cipher.encrypt(password, &cluster_password_aad(organization_id, kind))?;
        Ok(format!(
            "srenc1:{}:{}:{}",
            encrypted.key_version,
            STANDARD.encode(encrypted.nonce),
            STANDARD.encode(encrypted.ciphertext)
        ))
    }

    fn decrypt_cluster(&self, mut cluster: Cluster) -> ApiResult<Cluster> {
        let organization_id = cluster.organization_id.unwrap_or_default();
        cluster.password_encrypted =
            self.decrypt_password(&cluster.password_encrypted, organization_id, "password")?;
        if let Some(admin_password) = cluster.admin_password_encrypted.as_deref() {
            cluster.admin_password_encrypted =
                Some(self.decrypt_password(admin_password, organization_id, "admin_password")?);
        }
        Ok(cluster)
    }

    fn decrypt_password(&self, value: &str, organization_id: i64, kind: &str) -> ApiResult<String> {
        let Some(encoded) = value.strip_prefix("srenc1:") else {
            return Ok(value.to_owned());
        };
        let mut parts = encoded.split(':');
        let key_version = parts.next().and_then(|value| value.parse::<i64>().ok());
        let nonce = parts.next().and_then(|value| STANDARD.decode(value).ok());
        let ciphertext = parts.next().and_then(|value| STANDARD.decode(value).ok());
        if parts.next().is_some()
            || key_version.is_none()
            || nonce.is_none()
            || ciphertext.is_none()
        {
            return Err(ApiError::internal_error("stored cluster credential is malformed"));
        }
        let cipher = self.password_cipher.as_ref().ok_or_else(|| {
            ApiError::forbidden("APP_SR_PHYSICAL_ENCRYPTION_KEY is required to use this cluster")
        })?;
        cipher.decrypt(
            &EncryptedSecret {
                key_version: key_version.unwrap(),
                nonce: nonce.unwrap(),
                ciphertext: ciphertext.unwrap(),
            },
            &cluster_password_aad(organization_id, kind),
        )
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
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
                .fetch_optional(&self.pool)
                .await?
        {
            return Ok(id);
        }

        sqlx::query("INSERT INTO organizations (code, name, description, is_system) VALUES ('default_org', 'Default Organization', 'Auto-created default organization', 1)")
            .execute(&self.pool)
            .await?;

        sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'default_org'")
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

                if alive_count == total_count {
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
                                    message: format!("Failed to check {} nodes: {}", node_type, error_msg),
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
    use std::sync::Arc;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::ClusterService;
    use crate::{models::UpdateClusterRequest, services::MySQLPoolManager};

    #[tokio::test]
    async fn encrypts_cluster_passwords_when_the_deployment_key_is_configured() {
        let pool = SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .expect("lazy pool should be created");
        let service = ClusterService::new(pool, Arc::new(MySQLPoolManager::new()))
            .with_password_encryption_key("cluster-password-encryption-test-key");

        let stored = service
            .encrypt_password("secret", 7, "password")
            .expect("encryption should succeed");

        assert!(stored.starts_with("srenc1:"));
        assert_ne!(stored, "secret");
        assert_eq!(service.decrypt_password(&stored, 7, "password").unwrap(), "secret");
    }

    #[tokio::test]
    async fn reencrypts_passwords_when_a_cluster_changes_organization() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        let service = ClusterService::new(pool.clone(), Arc::new(MySQLPoolManager::new()))
            .with_password_encryption_key("cluster-password-encryption-test-key");
        let old_password = service.encrypt_password("secret", 1, "password").unwrap();
        let cluster_id = sqlx::query(
            "INSERT INTO clusters (name, fe_host, username, password_encrypted, organization_id) VALUES ('cluster', '127.0.0.1', 'operator', ?, 1)",
        )
        .bind(&old_password)
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();
        sqlx::query(
            "INSERT INTO organizations (code, name, description, is_system) VALUES ('test_org', 'Test Organization', '', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let organization_id: i64 =
            sqlx::query_scalar("SELECT id FROM organizations WHERE code = 'test_org'")
                .fetch_one(&pool)
                .await
                .unwrap();

        service
            .update_cluster(
                cluster_id,
                UpdateClusterRequest {
                    name: None,
                    description: None,
                    fe_host: None,
                    fe_http_port: None,
                    fe_query_port: None,
                    username: None,
                    password: None,
                    enable_ssl: None,
                    connection_timeout: None,
                    tags: None,
                    catalog: None,
                    organization_id: Some(organization_id),
                    deployment_mode: None,
                    cluster_type: None,
                    admin_user: None,
                    admin_password: None,
                },
            )
            .await
            .unwrap();

        let stored: String =
            sqlx::query_scalar("SELECT password_encrypted FROM clusters WHERE id = ?")
                .bind(cluster_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(stored, old_password);
        assert_eq!(
            service
                .get_cluster(cluster_id)
                .await
                .unwrap()
                .password_encrypted,
            "secret"
        );
    }
}
