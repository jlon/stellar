use crate::db::AppDb;
use crate::db::query as db_query;
use crate::models::{AssignUserRoleRequest, Role, RoleResponse};
use crate::services::casbin_service::CasbinService;
use crate::utils::{ApiError, ApiResult};
use sqlx::Pool;
use std::sync::Arc;
use stellar_macros::app_impl;

#[derive(Clone)]
pub struct UserRoleService<DB: AppDb> {
    pool: Pool<DB>,
    casbin_service: Arc<CasbinService>,
}

#[app_impl]
impl<DB: AppDb> UserRoleService<DB> {
    pub fn new(pool: Pool<DB>, casbin_service: Arc<CasbinService>) -> Self {
        Self { pool, casbin_service }
    }

    /// Get user's roles
    pub async fn get_user_roles(&self, user_id: i64) -> ApiResult<Vec<RoleResponse>> {
        let roles: Vec<Role> = db_query::query_as(
            r#"
            SELECT r.*
            FROM roles r
            JOIN user_roles ur ON r.id = ur.role_id
            WHERE ur.user_id = ?
            ORDER BY r.name
            "#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(roles.into_iter().map(|r| r.into()).collect())
    }

    /// Assign role to user
    pub async fn assign_role_to_user(
        &self,
        user_id: i64,
        req: AssignUserRoleRequest,
    ) -> ApiResult<()> {
        let role: Role = db_query::query_as("SELECT * FROM roles WHERE id = ?")
            .bind(req.role_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Role not found"))?;

        let existing: Option<(i64,)> =
            db_query::query_as("SELECT id FROM user_roles WHERE user_id = ? AND role_id = ?")
                .bind(user_id)
                .bind(req.role_id)
                .fetch_optional(&self.pool)
                .await?;

        if existing.is_some() {
            return Err(ApiError::validation_error("User already has this role"));
        }

        db_query::query("INSERT INTO user_roles (user_id, role_id) VALUES (?, ?)")
            .bind(user_id)
            .bind(req.role_id)
            .execute(&self.pool)
            .await?;

        self.casbin_service
            .add_role_for_user(user_id, req.role_id)
            .await?;

        tracing::info!("Role {} assigned to user {}", role.name, user_id);

        Ok(())
    }

    /// Remove role from user
    pub async fn remove_role_from_user(&self, user_id: i64, role_id: i64) -> ApiResult<()> {
        let existing: Option<(i64,)> =
            db_query::query_as("SELECT id FROM user_roles WHERE user_id = ? AND role_id = ?")
                .bind(user_id)
                .bind(role_id)
                .fetch_optional(&self.pool)
                .await?;

        if existing.is_none() {
            return Err(ApiError::not_found("User role assignment not found"));
        }

        db_query::query("DELETE FROM user_roles WHERE user_id = ? AND role_id = ?")
            .bind(user_id)
            .bind(role_id)
            .execute(&self.pool)
            .await?;

        self.casbin_service
            .remove_role_for_user(user_id, role_id)
            .await?;

        tracing::info!("Role {} removed from user {}", role_id, user_id);

        Ok(())
    }

    /// Get all roles for a user (including role details)
    pub async fn get_user_roles_detailed(&self, user_id: i64) -> ApiResult<Vec<RoleResponse>> {
        self.get_user_roles(user_id).await
    }
}
