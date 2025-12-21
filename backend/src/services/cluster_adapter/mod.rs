// Cluster Adapter Module
// Purpose: Provide unified interface for different OLAP engines (StarRocks, Doris)
// Design: Static dispatch via trait for zero-cost abstraction

mod doris;
mod starrocks;

pub use doris::DorisAdapter;
pub use starrocks::StarRocksAdapter;

use crate::models::{Backend, Cluster, ClusterType, Frontend, Query, RuntimeInfo};
use crate::services::MySQLPoolManager;
use crate::utils::ApiResult;
use async_trait::async_trait;
use std::sync::Arc;

/// Cluster adapter trait - unified interface for StarRocks and Doris
#[async_trait]
pub trait ClusterAdapter: Send + Sync {
    /// Get cluster type
    fn cluster_type(&self) -> ClusterType;

    /// Get cluster reference
    fn cluster(&self) -> &Cluster;

    /// Get base HTTP URL for FE
    fn get_base_url(&self) -> String;

    /// Get backend/compute nodes list
    async fn get_backends(&self) -> ApiResult<Vec<Backend>>;

    /// Get frontend nodes list
    async fn get_frontends(&self) -> ApiResult<Vec<Frontend>>;

    /// Drop a backend node
    async fn drop_backend(&self, host: &str, heartbeat_port: &str) -> ApiResult<()>;

    /// Get all active sessions
    async fn get_sessions(&self) -> ApiResult<Vec<crate::models::Session>>;

    /// Get current running queries
    async fn get_queries(&self) -> ApiResult<Vec<Query>>;

    /// Get runtime info
    async fn get_runtime_info(&self) -> ApiResult<RuntimeInfo>;

    /// Get Prometheus metrics
    async fn get_metrics(&self) -> ApiResult<String>;

    /// Parse Prometheus metrics to HashMap
    fn parse_prometheus_metrics(
        &self,
        metrics_text: &str,
    ) -> ApiResult<std::collections::HashMap<String, f64>>;

    /// List all catalogs
    async fn list_catalogs(&self) -> ApiResult<Vec<String>>;

    /// List databases in a catalog
    async fn list_databases(&self, catalog: Option<&str>) -> ApiResult<Vec<String>>;

    /// List materialized views
    async fn list_materialized_views(
        &self,
        database: Option<&str>,
    ) -> ApiResult<Vec<crate::models::MaterializedView>>;

    /// Get materialized view DDL
    async fn get_materialized_view_ddl(&self, mv_name: &str) -> ApiResult<String>;

    /// Create materialized view
    async fn create_materialized_view(&self, ddl: &str) -> ApiResult<()>;

    /// Drop materialized view
    async fn drop_materialized_view(&self, mv_name: &str) -> ApiResult<()>;

    /// Refresh materialized view
    ///
    /// # Parameters
    /// - `mv_name`: Materialized view name
    /// - `partition_start`: Optional partition start (for partition refresh)
    /// - `partition_end`: Optional partition end (for partition refresh)
    /// - `force`: Force refresh (StarRocks specific)
    /// - `mode`: Refresh mode ("complete", "auto", etc.)
    async fn refresh_materialized_view(
        &self,
        mv_name: &str,
        partition_start: Option<&str>,
        partition_end: Option<&str>,
        force: bool,
        mode: &str,
    ) -> ApiResult<()>;

    /// Alter materialized view
    async fn alter_materialized_view(&self, mv_name: &str, ddl: &str) -> ApiResult<()>;

    /// List SQL blacklist rules
    async fn list_sql_blacklist(&self) -> ApiResult<Vec<crate::models::SqlBlacklistItem>>;

    /// Add SQL blacklist rule
    async fn add_sql_blacklist(&self, pattern: &str) -> ApiResult<()>;

    /// Delete SQL blacklist rule
    async fn delete_sql_blacklist(&self, id: &str) -> ApiResult<()>;

    /// Execute SQL command via HTTP API
    async fn execute_sql(&self, sql: &str) -> ApiResult<()>;

    /// Execute SHOW PROC command and return raw results
    async fn show_proc_raw(&self, path: &str) -> ApiResult<Vec<serde_json::Value>>;

    /// List query profiles
    async fn list_profiles(&self) -> ApiResult<Vec<crate::models::ProfileListItem>>;

    /// Get profile detail by query_id
    async fn get_profile(&self, query_id: &str) -> ApiResult<String>;

    // ========================================
    // Permission Management Methods
    // ========================================

    /// Create a new database user
    async fn create_user(&self, username: &str, password: &str) -> ApiResult<String>;

    /// Create a new database role
    async fn create_role(&self, role_name: &str) -> ApiResult<String>;

    /// Grant permissions to a user or role
    async fn grant_permissions(
        &self,
        principal_type: &str, // "USER" or "ROLE"
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str, // "DATABASE" or "TABLE"
        database: &str,
        table: Option<&str>,
        with_grant_option: bool,
    ) -> ApiResult<String>;

    /// Revoke permissions from a user
    async fn revoke_permissions(
        &self,
        principal_type: &str, // "USER" or "ROLE"
        principal_name: &str,
        permissions: &[&str],
        resource_type: &str, // "DATABASE" or "TABLE"
        database: &str,
        table: Option<&str>,
    ) -> ApiResult<String>;

    /// Grant a role to a user
    async fn grant_role(&self, role_name: &str, username: &str) -> ApiResult<String>;

    /// List user permissions (for "我的权限" dashboard)
    async fn list_user_permissions(&self, username: &str) -> ApiResult<Vec<crate::models::DbUserPermissionDto>>;

    /// List all database accounts
    async fn list_db_accounts(&self) -> ApiResult<Vec<crate::models::DbAccountDto>>;

    /// List all database roles
    async fn list_db_roles(&self) -> ApiResult<Vec<crate::models::DbRoleDto>>;
}

/// Create adapter based on cluster type (factory method)
pub fn create_adapter(
    cluster: Cluster,
    pool_manager: Arc<MySQLPoolManager>,
) -> Box<dyn ClusterAdapter> {
    match cluster.cluster_type {
        ClusterType::Doris => Box::new(DorisAdapter::new(cluster, pool_manager)),
        ClusterType::StarRocks => Box::new(StarRocksAdapter::new(cluster, pool_manager)),
    }
}

/// Create adapter with specific type (for compile-time type safety)
pub fn create_starrocks_adapter(
    cluster: Cluster,
    pool_manager: Arc<MySQLPoolManager>,
) -> StarRocksAdapter {
    StarRocksAdapter::new(cluster, pool_manager)
}

pub fn create_doris_adapter(cluster: Cluster, pool_manager: Arc<MySQLPoolManager>) -> DorisAdapter {
    DorisAdapter::new(cluster, pool_manager)
}
