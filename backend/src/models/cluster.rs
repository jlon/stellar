use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

/// Cluster type for OLAP engine
///
/// DB 编解码使用 `impl_string_backed_db_type`（与 String 同构，兼容 VARCHAR/TEXT），
/// 不用 `sqlx::Type` derive：强枚举的 per-backend impl 与 VARCHAR 列不兼容（MySQL 实测）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum ClusterType {
    /// StarRocks OLAP engine
    #[default]
    StarRocks,
    /// Apache Doris OLAP engine
    Doris,
}

impl std::fmt::Display for ClusterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClusterType::StarRocks => write!(f, "starrocks"),
            ClusterType::Doris => write!(f, "doris"),
        }
    }
}

impl ClusterType {
    /// Returns the display name for UI and prompts
    pub const fn display_name(&self) -> &'static str {
        match self {
            ClusterType::StarRocks => "StarRocks",
            ClusterType::Doris => "Doris",
        }
    }

    /// Parse from string (case-insensitive)
    pub fn from_str_loose(s: &str) -> Self {
        if s.eq_ignore_ascii_case("doris") { ClusterType::Doris } else { ClusterType::StarRocks }
    }
}

/// Deployment mode for cluster
///
/// DB 编解码同 `ClusterType`，见 `impl_string_backed_db_type`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    /// Shared-nothing architecture: BE nodes handle both storage and compute
    #[default]
    SharedNothing,
    /// Shared-data architecture: CN nodes for compute, separate object storage (S3/HDFS)
    SharedData,
}

impl DeploymentMode {
    /// Parse from string (case-insensitive; unknown values fall back to default)
    pub fn from_str_loose(s: &str) -> Self {
        if s.eq_ignore_ascii_case("shared_data") { Self::SharedData } else { Self::SharedNothing }
    }
}

impl std::fmt::Display for DeploymentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentMode::SharedNothing => write!(f, "shared_nothing"),
            DeploymentMode::SharedData => write!(f, "shared_data"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct Cluster {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fe_host: String,
    pub fe_http_port: i32,
    pub fe_query_port: i32,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_encrypted: String,
    pub enable_ssl: bool,
    pub connection_timeout: i32,
    pub tags: Option<String>,
    pub catalog: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<i64>,
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub deployment_mode: DeploymentMode,
    #[serde(default)]
    pub cluster_type: ClusterType,
    /// Admin user for permission execution (optional, only visible to org admins and super admins)
    pub admin_user: Option<String>,
    /// Admin password encrypted (optional, never serialized)
    #[serde(skip_serializing)]
    pub admin_password_encrypted: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClusterRequest {
    pub name: String,
    pub description: Option<String>,
    pub fe_host: String,
    #[serde(default = "default_http_port")]
    pub fe_http_port: i32,
    #[serde(default = "default_query_port")]
    pub fe_query_port: i32,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub enable_ssl: bool,
    #[serde(default = "default_timeout")]
    pub connection_timeout: i32,
    pub tags: Option<Vec<String>>,
    #[serde(default = "default_catalog")]
    pub catalog: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub deployment_mode: DeploymentMode,
    #[serde(default)]
    pub cluster_type: ClusterType,
    /// Admin user for permission execution (optional, only configurable by org admins and super admins)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_user: Option<String>,
    /// Admin password (optional, only configurable by org admins and super admins)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_password: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateClusterRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub fe_host: Option<String>,
    pub fe_http_port: Option<i32>,
    pub fe_query_port: Option<i32>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub enable_ssl: Option<bool>,
    pub connection_timeout: Option<i32>,
    pub tags: Option<Vec<String>>,
    pub catalog: Option<String>,
    pub organization_id: Option<i64>,
    pub deployment_mode: Option<DeploymentMode>,
    pub cluster_type: Option<ClusterType>,
    /// Admin user for permission execution (optional, only configurable by org admins and super admins)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_user: Option<String>,
    /// Admin password (optional, only configurable by org admins and super admins)
    /// If provided, will update admin_password_encrypted; if None, keeps existing value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_password: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ClusterResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub fe_host: String,
    pub fe_http_port: i32,
    pub fe_query_port: i32,
    pub username: String,
    pub enable_ssl: bool,
    pub connection_timeout: i32,
    pub tags: Vec<String>,
    pub catalog: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<i64>,
    pub deployment_mode: DeploymentMode,
    pub cluster_type: ClusterType,
    /// Admin user for permission execution (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_user: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ClusterHealth {
    pub status: HealthStatus,
    pub checks: Vec<HealthCheck>,
    pub last_check_time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

fn default_http_port() -> i32 {
    8030
}

fn default_query_port() -> i32 {
    9030
}

fn default_timeout() -> i32 {
    10
}

fn default_catalog() -> String {
    "default_catalog".to_string()
}

impl From<Cluster> for ClusterResponse {
    fn from(cluster: Cluster) -> Self {
        let tags = cluster
            .tags
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();

        Self {
            id: cluster.id,
            name: cluster.name,
            description: cluster.description,
            fe_host: cluster.fe_host,
            fe_http_port: cluster.fe_http_port,
            fe_query_port: cluster.fe_query_port,
            username: cluster.username,
            enable_ssl: cluster.enable_ssl,
            connection_timeout: cluster.connection_timeout,
            tags,
            catalog: cluster.catalog,
            is_active: cluster.is_active,
            created_at: cluster.created_at,
            updated_at: cluster.updated_at,
            organization_id: cluster.organization_id,
            deployment_mode: cluster.deployment_mode,
            cluster_type: cluster.cluster_type,
            admin_user: cluster.admin_user,
        }
    }
}

impl Cluster {
    /// Check if cluster is using shared-data (storage-compute separated) architecture
    pub fn is_shared_data(&self) -> bool {
        self.deployment_mode == DeploymentMode::SharedData
    }

    /// Check if cluster is using shared-nothing (storage-compute integrated) architecture
    pub fn is_shared_nothing(&self) -> bool {
        self.deployment_mode == DeploymentMode::SharedNothing
    }

    /// Check if cluster is StarRocks type
    pub fn is_starrocks(&self) -> bool {
        self.cluster_type == ClusterType::StarRocks
    }

    /// Check if cluster is Doris type
    pub fn is_doris(&self) -> bool {
        self.cluster_type == ClusterType::Doris
    }

    /// Get password for authentication - returns None if password is empty
    /// This is used for proper handling of no-password clusters
    pub fn get_auth_password(&self) -> Option<&str> {
        if self.password_encrypted.is_empty() { None } else { Some(&self.password_encrypted) }
    }

    /// Get execution credentials for permission operations
    /// Returns admin user credentials if configured, otherwise falls back to connection user
    pub fn get_execution_credentials(&self) -> (&str, Option<&str>) {
        if let (Some(admin_user), Some(admin_pass)) =
            (&self.admin_user, &self.admin_password_encrypted)
            && !admin_pass.is_empty()
        {
            return (admin_user, Some(admin_pass));
        }
        // Fallback to connection user
        (&self.username, self.get_auth_password())
    }
}

// 数据库编解码：与 String 同构（VARCHAR/TEXT 列），两后端通用
crate::impl_string_backed_db_type!(ClusterType, |s| ClusterType::from_str_loose(s));
crate::impl_string_backed_db_type!(DeploymentMode, |s| DeploymentMode::from_str_loose(s));
