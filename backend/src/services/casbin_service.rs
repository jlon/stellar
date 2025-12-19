use crate::utils::{ApiError, ApiResult};
use casbin::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Casbin service for RBAC permission checking
///
/// Uses in-memory adapter and loads policies from database dynamically
pub struct CasbinService {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl CasbinService {
    /// Create a new Casbin service with RBAC model
    pub async fn new() -> ApiResult<Self> {
        let model_str = r#"
[request_definition]
r = sub, obj, act

[policy_definition]
p = sub, obj, act

[role_definition]
g = _, _

[policy_effect]
e = some(where (p.eft == allow))

[matchers]
m = g(r.sub, p.sub) && r.obj == p.obj && r.act == p.act
"#;

        let model = DefaultModel::from_str(model_str).await.map_err(|e| {
            tracing::error!("Failed to create Casbin model: {:?}", e);
            ApiError::internal_error(format!("Failed to initialize Casbin model: {}", e))
        })?;

        let adapter = casbin::MemoryAdapter::default();

        let enforcer = Enforcer::new(model, adapter).await.map_err(|e| {
            tracing::error!("Failed to create Casbin enforcer: {:?}", e);
            ApiError::internal_error(format!("Failed to initialize Casbin enforcer: {}", e))
        })?;

        tracing::info!("Casbin service initialized successfully");

        Ok(Self { enforcer: Arc::new(RwLock::new(enforcer)) })
    }

    /// Check if a user has permission for a resource and action
    ///
    /// SECURITY NOTE: Uses "u:<user_id>" prefix for users and "r:<role_id>" prefix for roles
    /// to prevent ID collision vulnerability where user_id == role_id could cause
    /// permission bypass in Casbin's g() function.
    pub async fn enforce(&self, user_id: i64, resource: &str, action: &str) -> ApiResult<bool> {
        let enforcer = self.enforcer.read().await;

        let user_subject = format!("u:{}", user_id);

        let permitted = enforcer
            .enforce(vec![user_subject, resource.to_string(), action.to_string()])
            .map_err(|e| {
                tracing::error!("Casbin enforce error: {:?}", e);
                ApiError::internal_error(format!("Permission check failed: {}", e))
            })?;

        Ok(permitted)
    }

    /// Add a policy rule: role has permission to access resource with action
    pub async fn add_policy(&self, role_id: i64, resource: &str, action: &str) -> ApiResult<bool> {
        let mut enforcer = self.enforcer.write().await;

        let parts = vec![format!("r:{}", role_id), resource.to_string(), action.to_string()];

        let added = enforcer.add_policy(parts).await.map_err(|e| {
            tracing::error!("Failed to add policy: {:?}", e);
            ApiError::internal_error(format!("Failed to add policy: {}", e))
        })?;

        Ok(added)
    }

    /// Remove a policy rule
    pub async fn remove_policy(
        &self,
        role_id: i64,
        resource: &str,
        action: &str,
    ) -> ApiResult<bool> {
        let mut enforcer = self.enforcer.write().await;

        let parts = vec![format!("r:{}", role_id), resource.to_string(), action.to_string()];

        let removed = enforcer.remove_policy(parts).await.map_err(|e| {
            tracing::error!("Failed to remove policy: {:?}", e);
            ApiError::internal_error(format!("Failed to remove policy: {}", e))
        })?;

        Ok(removed)
    }

    /// Add role assignment: user has role
    pub async fn add_role_for_user(&self, user_id: i64, role_id: i64) -> ApiResult<bool> {
        let mut enforcer = self.enforcer.write().await;

        let parts = vec![format!("u:{}", user_id), format!("r:{}", role_id)];

        let added = enforcer.add_grouping_policy(parts).await.map_err(|e| {
            tracing::error!("Failed to add role for user: {:?}", e);
            ApiError::internal_error(format!("Failed to assign role: {}", e))
        })?;

        Ok(added)
    }

    /// Remove role assignment: user no longer has role
    pub async fn remove_role_for_user(&self, user_id: i64, role_id: i64) -> ApiResult<bool> {
        let mut enforcer = self.enforcer.write().await;

        let parts = vec![format!("u:{}", user_id), format!("r:{}", role_id)];

        let removed = enforcer.remove_grouping_policy(parts).await.map_err(|e| {
            tracing::error!("Failed to remove role for user: {:?}", e);
            ApiError::internal_error(format!("Failed to remove role: {}", e))
        })?;

        Ok(removed)
    }

    /// Load all policies from database into Casbin
    /// This should be called after role-permission mappings change
    pub async fn reload_policies_from_db(&self, pool: &sqlx::SqlitePool) -> ApiResult<()> {
        let mut enforcer = self.enforcer.write().await;

        enforcer.clear_policy().await.map_err(|e| {
            tracing::error!("Failed to clear policies: {:?}", e);
            ApiError::internal_error(format!("Failed to clear policies: {}", e))
        })?;

        let role_permissions: Vec<(i64, Option<i64>, String, String)> = sqlx::query_as(
            r#"
            SELECT rp.role_id, r.organization_id, p.code, COALESCE(p.action, '') as action
            FROM role_permissions rp
            JOIN permissions p ON rp.permission_id = p.id
            JOIN roles r ON r.id = rp.role_id
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load role permissions: {:?}", e);
            ApiError::internal_error(format!("Failed to load policies: {}", e))
        })?;

        for (role_id, org_id, code, action) in role_permissions {
            let parts: Vec<&str> = code.split(':').collect();
            if parts.len() >= 2 {
                let resource = parts[1].to_string();

                let act = if !action.is_empty() {
                    action
                } else if parts.len() >= 3 {
                    parts[2..].join(":")
                } else {
                    "view".to_string()
                };

                let scoped_resource = Self::format_resource_key(org_id, &resource);

                let policy_parts =
                    vec![format!("r:{}", role_id), scoped_resource.clone(), act.clone()];
                let _ = enforcer.add_policy(policy_parts).await;
            }
        }

        let user_roles: Vec<(i64, i64)> = sqlx::query_as("SELECT user_id, role_id FROM user_roles")
            .fetch_all(pool)
            .await
            .map_err(|e| {
                tracing::error!("Failed to load user roles: {:?}", e);
                ApiError::internal_error(format!("Failed to load user roles: {}", e))
            })?;

        for (user_id, role_id) in user_roles {
            let grouping_parts = vec![format!("u:{}", user_id), format!("r:{}", role_id)];
            let _ = enforcer.add_grouping_policy(grouping_parts).await;
        }

        tracing::info!("Policies reloaded from database successfully");
        Ok(())
    }
}

impl CasbinService {
    pub(crate) fn format_resource_key(org_id: Option<i64>, resource: &str) -> String {
        match org_id {
            Some(id) => format!("org:{}:{}", id, resource),
            None => format!("system:{}", resource),
        }
    }
}
