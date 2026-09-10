use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

use crate::middleware::permission_extractor;
use crate::services::casbin_service::CasbinService;
use crate::utils::{ApiError, JwtUtil};
use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AuthState {
    pub jwt_util: Arc<JwtUtil>,
    pub casbin_service: Arc<CasbinService>,
    pub db: SqlitePool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct OrgContext {
    pub user_id: i64,
    pub username: String,
    pub organization_id: Option<i64>,
    pub is_super_admin: bool,
}

/// Authentication + authorization middleware.
/// 1. 验证 JWT
/// 2. 将 `user_id` 写入 request extensions
/// 3. 根据 URI/Method 推导权限码并交给 Casbin 检查
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let uri_full = req.uri().to_string();
    let uri = uri_full.split('?').next().unwrap_or(&uri_full).to_string();
    let method = req.method().to_string();

    tracing::debug!("Auth middleware processing: {} {}", method, uri);

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            tracing::warn!("Missing authorization header for {} {}", method, uri);
            ApiError::unauthorized("Missing authorization header")
        })?;

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        tracing::warn!("Invalid authorization header format for {} {}", method, uri);
        ApiError::unauthorized("Invalid authorization header format")
    })?;

    let claims = state.jwt_util.verify_token(token).map_err(|err| {
        tracing::warn!("JWT verification failed for {} {}: {:?}", method, uri, err);
        err
    })?;

    let user_id = claims.sub.parse::<i64>().unwrap_or_default();
    tracing::debug!(
        "JWT token verified for user {} (ID: {}) on {} {}",
        claims.username,
        user_id,
        method,
        uri
    );

    let (is_super_admin, organization_id): (bool, Option<i64>) = sqlx::query_as(
        r#"
        SELECT
            COALESCE(EXISTS (
                SELECT 1
                FROM user_roles ur
                JOIN roles r ON r.id = ur.role_id
                WHERE ur.user_id = ? AND r.code = 'super_admin'
            ), 0) as is_super_admin,
            NULLIF(u.organization_id, 0) as organization_id
        FROM users u
        WHERE u.id = ?
        "#,
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or((false, None));

    let organization_id = if organization_id.is_none() {
        fetch_org_from_user_organizations(&state.db, user_id).await?
    } else {
        organization_id
    };

    req.extensions_mut().insert(user_id);
    req.extensions_mut().insert(claims.username.clone());

    let org_ctx =
        OrgContext { user_id, username: claims.username.clone(), organization_id, is_super_admin };
    req.extensions_mut().insert(org_ctx.clone());

    ensure_not_read_only_adoption_mutation(&state.db, &org_ctx, &method, &uri).await?;

    if let Some((resource, action)) = permission_extractor::extract_permission(&method, &uri) {
        let resource_scope = if org_ctx.is_super_admin || org_ctx.organization_id.is_none() {
            crate::services::casbin_service::CasbinService::format_resource_key(None, &resource)
        } else {
            crate::services::casbin_service::CasbinService::format_resource_key(
                org_ctx.organization_id,
                &resource,
            )
        };

        tracing::debug!(
            "Checking permission for user {} -> {}:{}",
            user_id,
            resource_scope,
            action
        );

        let allowed = state
            .casbin_service
            .enforce(user_id, &resource_scope, &action)
            .await
            .unwrap_or(false);

        if !allowed {
            tracing::warn!(
                "Permission denied for user {} on {} {} (resource={}, action={})",
                user_id,
                method,
                uri,
                resource,
                action
            );
            return Err(ApiError::unauthorized(format!(
                "Permission denied: no access to {} {}",
                resource, action
            )));
        }

        tracing::debug!("Permission granted for user {} on {} {}", user_id, method, uri);
    }

    Ok(next.run(req).await)
}

async fn ensure_not_read_only_adoption_mutation(
    db: &SqlitePool,
    org_ctx: &OrgContext,
    method: &str,
    path: &str,
) -> Result<(), ApiError> {
    if !mutates_cluster(path, method) {
        return Ok(());
    }

    let cluster_id = path
        .strip_prefix("/api/clusters/")
        .and_then(|suffix| suffix.split('/').next())
        .and_then(|segment| segment.parse::<i64>().ok());
    let adopted: Option<i64> = if let Some(cluster_id) = cluster_id {
        sqlx::query_scalar(
            "SELECT cluster_id FROM sr_managed_clusters WHERE cluster_id = ? AND status = 'adopted_read_only' AND (? IS NULL OR organization_id = ?) LIMIT 1",
        )
        .bind(cluster_id)
        .bind((!org_ctx.is_super_admin).then_some(org_ctx.organization_id).flatten())
        .bind((!org_ctx.is_super_admin).then_some(org_ctx.organization_id).flatten())
        .fetch_optional(db)
        .await?
    } else {
        sqlx::query_scalar(
            "SELECT m.cluster_id FROM sr_managed_clusters m JOIN clusters c ON c.id = m.cluster_id WHERE m.status = 'adopted_read_only' AND c.is_active = 1 AND (? IS NULL OR m.organization_id = ?) LIMIT 1",
        )
        .bind((!org_ctx.is_super_admin).then_some(org_ctx.organization_id).flatten())
        .bind((!org_ctx.is_super_admin).then_some(org_ctx.organization_id).flatten())
        .fetch_optional(db)
        .await?
    };
    if adopted.is_some() {
        return Err(ApiError::forbidden(
            "read-only adopted clusters cannot be changed through Stellar",
        ));
    }
    Ok(())
}

fn mutates_cluster(path: &str, method: &str) -> bool {
    if !matches!(method, "POST" | "PUT" | "PATCH" | "DELETE") || !path.starts_with("/api/clusters")
    {
        return false;
    }
    !matches!(path, "/api/clusters" | "/api/clusters/health/test")
        && !path.ends_with("/sql/diagnose")
}

// Helper to fetch organization from user_organizations when users.organization_id is NULL
async fn fetch_org_from_user_organizations(
    db: &SqlitePool,
    user_id: i64,
) -> Result<Option<i64>, ApiError> {
    Ok(sqlx::query_scalar::<_, i64>(
        r#"SELECT organization_id FROM user_organizations WHERE user_id = ?"#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::mutates_cluster;

    #[test]
    fn only_marks_existing_cluster_mutations() {
        assert!(!mutates_cluster("/api/clusters", "POST"));
        assert!(!mutates_cluster("/api/clusters/health/test", "POST"));
        assert!(!mutates_cluster("/api/clusters/7/sql/diagnose", "POST"));
        assert!(mutates_cluster("/api/clusters/7", "PUT"));
        assert!(mutates_cluster("/api/clusters/queries/execute", "POST"));
    }
}
