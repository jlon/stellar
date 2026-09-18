use crate::db::AppDb;
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::models::{DbAccountDto, DbRoleDto, DbUserPermissionDto};
use crate::services::cluster_adapter::create_adapter;
use crate::services::{ClusterService, MySQLPoolManager};
use crate::utils::ApiResult;

/// Service for querying database accounts and roles from OLAP engines (StarRocks/Doris)
/// Uses ClusterAdapter for database-specific SQL dialect handling
#[derive(Clone)]
pub struct DbAuthQueryService<DB: AppDb> {
    mysql_pool_manager: Arc<MySQLPoolManager>,
    cluster_service: Arc<ClusterService<DB>>,
}

#[app_impl]
impl<DB: AppDb> DbAuthQueryService<DB> {
    pub fn new(
        mysql_pool_manager: Arc<MySQLPoolManager>,
        cluster_service: Arc<ClusterService<DB>>,
    ) -> Self {
        Self { mysql_pool_manager, cluster_service }
    }

    /// Query all database accounts from the cluster
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_accounts(&self, cluster_id: i64) -> ApiResult<Vec<DbAccountDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());

        adapter.list_db_accounts().await
    }

    /// Query all database roles from the cluster
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_roles(&self, cluster_id: i64) -> ApiResult<Vec<DbRoleDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());

        adapter.list_db_roles().await
    }

    /// List current user's database permissions on a cluster
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_user_permissions(
        &self,
        cluster_id: i64,
        username: &str,
    ) -> ApiResult<Vec<DbUserPermissionDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());

        adapter.list_user_permissions(username).await
    }

    /// Query permissions for a specific role
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_role_permissions(
        &self,
        cluster_id: i64,
        role_name: &str,
    ) -> ApiResult<Vec<DbUserPermissionDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());

        adapter.list_role_permissions(role_name).await
    }
}
