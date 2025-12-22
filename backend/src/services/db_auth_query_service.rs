use std::sync::Arc;

use crate::models::{DbAccountDto, DbRoleDto, DbUserPermissionDto};
use crate::services::{MySQLPoolManager, ClusterService};
use crate::services::cluster_adapter::create_adapter;
use crate::utils::ApiResult;

/// Service for querying database accounts and roles from OLAP engines (StarRocks/Doris)
/// Uses ClusterAdapter for database-specific SQL dialect handling
#[derive(Clone)]
pub struct DbAuthQueryService {
    mysql_pool_manager: Arc<MySQLPoolManager>,
    cluster_service: Arc<ClusterService>,
}

impl DbAuthQueryService {
    pub fn new(mysql_pool_manager: Arc<MySQLPoolManager>, cluster_service: Arc<ClusterService>) -> Self {
        Self {
            mysql_pool_manager,
            cluster_service,
        }
    }

    /// Query all database accounts from the cluster
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_accounts(&self, cluster_id: i64) -> ApiResult<Vec<DbAccountDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());
        
        match adapter.list_db_accounts().await {
            Ok(accounts) => Ok(accounts),
            Err(e) => {
                tracing::warn!("Failed to query accounts for cluster {}: {}", cluster_id, e);
                Ok(Vec::new())
            }
        }
    }

    /// Query all database roles from the cluster
    /// Uses ClusterAdapter for database-specific implementation
    pub async fn list_roles(&self, cluster_id: i64) -> ApiResult<Vec<DbRoleDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let adapter = create_adapter(cluster, self.mysql_pool_manager.clone());
        
        match adapter.list_db_roles().await {
            Ok(roles) => Ok(roles),
            Err(e) => {
                tracing::warn!("Failed to query roles for cluster {}: {}", cluster_id, e);
                Ok(Vec::new())
            }
        }
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
        
        match adapter.list_user_permissions(username).await {
            Ok(permissions) => Ok(permissions),
            Err(e) => {
                tracing::warn!(
                    "Failed to query permissions for user {} on cluster {}: {}",
                    username, cluster_id, e
                );
                Ok(Vec::new())
            }
        }
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
        
        match adapter.list_role_permissions(role_name).await {
            Ok(permissions) => Ok(permissions),
            Err(e) => {
                tracing::warn!(
                    "Failed to query permissions for role {} on cluster {}: {}",
                    role_name, cluster_id, e
                );
                Ok(Vec::new())
            }
        }
    }
}
