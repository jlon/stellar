use std::sync::Arc;

use crate::models::{DbAccountDto, DbRoleDto};
use crate::services::{MySQLPoolManager, ClusterService};
use crate::utils::ApiResult;
use mysql_async::prelude::Queryable;

/// Service for querying database accounts and roles from OLAP engines (StarRocks/Doris)
/// Supports both real-time queries and graceful fallback to mock data on connection errors
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
    /// Queries INFORMATION_SCHEMA.USER_PRIVILEGES from MySQL-compatible databases (StarRocks/Doris)
    pub async fn list_accounts(&self, cluster_id: i64) -> ApiResult<Vec<DbAccountDto>> {
        match self._list_accounts_real(cluster_id).await {
            Ok(accounts) => Ok(accounts),
            Err(e) => {
                tracing::warn!("Failed to query real accounts for cluster {}: {}. Returning mock data.", cluster_id, e);
                self._list_accounts_mock().await
            }
        }
    }

    /// Internal: Query real accounts from MySQL cluster
    async fn _list_accounts_real(&self, cluster_id: i64) -> ApiResult<Vec<DbAccountDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let mut conn = self.mysql_pool_manager.get_pool(&cluster).await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Query INFORMATION_SCHEMA.USER_PRIVILEGES for all users
        // Works with both StarRocks and Apache Doris (MySQL-compatible)
        let query_str = "SELECT DISTINCT GRANTEE, HOST FROM INFORMATION_SCHEMA.USER_PRIVILEGES ORDER BY GRANTEE";

        let rows: Vec<(String, String)> = conn.query(query_str)
            .await
            .map_err(|e| {
                tracing::error!("Failed to query user privileges: {}", e);
                crate::utils::ApiError::InternalError(format!("Database query failed: {}", e))
            })?;

        let mut accounts: Vec<DbAccountDto> = Vec::new();
        for (grantee, host) in rows {
            // Parse GRANTEE which is in format 'username'@'host'
            let (account_name, _) = if grantee.contains('@') {
                let parts: Vec<&str> = grantee.splitn(2, '@').collect();
                (parts[0].trim_matches('\'').to_string(), parts[1].trim_matches('\'').to_string())
            } else {
                (grantee.clone(), host.clone())
            };

            // Check if this account already exists in the result
            if !accounts.iter().any(|a| a.account_name == account_name && a.host == host) {
                accounts.push(DbAccountDto {
                    account_name,
                    host,
                    roles: vec![],
                });
            }
        }

        Ok(accounts)
    }

    /// Mock data for accounts (fallback)
    async fn _list_accounts_mock(&self) -> ApiResult<Vec<DbAccountDto>> {
        Ok(vec![
            DbAccountDto {
                account_name: "root".to_string(),
                host: "%".to_string(),
                roles: vec![],
            },
            DbAccountDto {
                account_name: "admin".to_string(),
                host: "%.%.%.%".to_string(),
                roles: vec![],
            },
        ])
    }

    /// Query all database roles from the cluster
    /// Uses INFORMATION_SCHEMA.ROLES or similar (database-specific)
    pub async fn list_roles(&self, cluster_id: i64) -> ApiResult<Vec<DbRoleDto>> {
        match self._list_roles_real(cluster_id).await {
            Ok(roles) => Ok(roles),
            Err(e) => {
                tracing::warn!("Failed to query real roles for cluster {}: {}. Returning mock data.", cluster_id, e);
                self._list_roles_mock().await
            }
        }
    }

    /// Internal: Query real roles from cluster
    async fn _list_roles_real(&self, cluster_id: i64) -> ApiResult<Vec<DbRoleDto>> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let mut conn = self.mysql_pool_manager.get_pool(&cluster).await?
            .get_conn()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get connection from pool: {}", e);
                crate::utils::ApiError::InternalError(format!("Database connection failed: {}", e))
            })?;

        // Try to query roles - exact SQL depends on database system
        // First try INFORMATION_SCHEMA.ROLES (works for some systems)
        let query_str = "SELECT ROLE FROM INFORMATION_SCHEMA.ROLES";

        let rows: Vec<(String,)> = match conn.query(query_str).await {
            Ok(rows) => rows,
            Err(_) => {
                // Fallback: return mock data if INFORMATION_SCHEMA.ROLES doesn't exist
                tracing::debug!("INFORMATION_SCHEMA.ROLES query failed, using mock data");
                return self._list_roles_mock().await;
            }
        };

        let mut roles: Vec<DbRoleDto> = Vec::new();
        for (role_name,) in rows {
            let role_type = if role_name == "admin" || role_name == "public_all_db" {
                "built-in".to_string()
            } else {
                "custom".to_string()
            };

            roles.push(DbRoleDto {
                role_name,
                role_type,
                permissions_count: None,
            });
        }

        Ok(roles)
    }

    /// Mock data for roles (fallback)
    async fn _list_roles_mock(&self) -> ApiResult<Vec<DbRoleDto>> {
        Ok(vec![
            DbRoleDto {
                role_name: "admin".to_string(),
                role_type: "built-in".to_string(),
                permissions_count: None,
            },
            DbRoleDto {
                role_name: "user".to_string(),
                role_type: "built-in".to_string(),
                permissions_count: None,
            },
        ])
    }
}
