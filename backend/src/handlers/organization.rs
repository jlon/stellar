use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::models::{CreateOrganizationRequest, OrganizationResponse, UpdateOrganizationRequest};
use crate::utils::ApiResult;

// List organizations (super admin sees all; others see only their own)
#[utoipa::path(
    get,
    path = "/api/organizations",
    responses(
        (status = 200, description = "List of organizations", body = Vec<OrganizationResponse>)
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Organizations"
)]
#[app_db]
pub async fn list_organizations(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<OrganizationResponse>>> {
    tracing::debug!(
        "Listing organizations for user {} (org: {:?}, super_admin: {})",
        org_ctx.user_id,
        org_ctx.organization_id,
        org_ctx.is_super_admin
    );

    let orgs = state
        .organization_service
        .list_organizations(org_ctx.organization_id, org_ctx.is_super_admin)
        .await?;
    Ok(Json(orgs))
}

// Get organization by ID
#[utoipa::path(
    get,
    path = "/api/organizations/{id}",
    params(
        ("id" = i64, Path, description = "Organization ID")
    ),
    responses(
        (status = 200, description = "Organization details", body = OrganizationResponse),
        (status = 404, description = "Organization not found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Organizations"
)]
#[app_db]
pub async fn get_organization(
    State(state): State<Arc<AppState<DB>>>,
    Path(id): Path<i64>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<OrganizationResponse>> {
    let org = state
        .organization_service
        .get_organization(id, org_ctx.organization_id, org_ctx.is_super_admin)
        .await?;
    Ok(Json(org))
}

// Create organization (super admin only)
#[utoipa::path(
    post,
    path = "/api/organizations",
    request_body = CreateOrganizationRequest,
    responses(
        (status = 200, description = "Organization created successfully", body = OrganizationResponse),
        (status = 400, description = "Bad request")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Organizations"
)]
#[app_db]
pub async fn create_organization(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Json(req): Json<CreateOrganizationRequest>,
) -> ApiResult<Json<OrganizationResponse>> {
    if !org_ctx.is_super_admin {
        return Err(crate::utils::ApiError::forbidden(
            "Only super administrators can create organizations",
        ));
    }

    tracing::info!("Creating organization: {} by super admin {}", req.code, org_ctx.user_id);

    let org = state.organization_service.create_organization(req).await?;
    tracing::info!("Organization created successfully: {} (ID: {})", org.code, org.id);

    state
        .casbin_service
        .reload_policies_from_db(&state.db)
        .await?;
    tracing::info!("Reloaded Casbin policies after organization creation");

        crate::services::op_audit::log_op_best_effort(
        &state.db,
        crate::services::op_audit::OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "create",
            target_type: "organization",
            target_id: Some(org.id),
            target_name: &org.name,
        },
    )
    .await;
        
Ok(Json(org))
}

// Update organization
#[utoipa::path(
    put,
    path = "/api/organizations/{id}",
    params(
        ("id" = i64, Path, description = "Organization ID")
    ),
    request_body = UpdateOrganizationRequest,
    responses(
        (status = 200, description = "Organization updated successfully", body = OrganizationResponse),
        (status = 404, description = "Organization not found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Organizations"
)]
#[app_db]
pub async fn update_organization(
    State(state): State<Arc<AppState<DB>>>,
    Path(id): Path<i64>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Json(req): Json<UpdateOrganizationRequest>,
) -> ApiResult<Json<OrganizationResponse>> {
    let org = state
        .organization_service
        .update_organization(id, req, org_ctx.organization_id, org_ctx.is_super_admin)
        .await?;
    tracing::info!("Organization updated: ID {} by user {}", org.id, org_ctx.user_id);
        crate::services::op_audit::log_op_best_effort(
        &state.db,
        crate::services::op_audit::OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "update",
            target_type: "organization",
            target_id: Some(org.id),
            target_name: &org.name,
        },
    )
    .await;
        
Ok(Json(org))
}

// Delete organization (super admin only, cannot delete system orgs)
#[utoipa::path(
    delete,
    path = "/api/organizations/{id}",
    params(
        ("id" = i64, Path, description = "Organization ID")
    ),
    responses(
        (status = 200, description = "Organization deleted successfully"),
        (status = 404, description = "Organization not found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Organizations"
)]
#[app_db]
pub async fn delete_organization(
    State(state): State<Arc<AppState<DB>>>,
    Path(id): Path<i64>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<serde_json::Value>> {
    if !org_ctx.is_super_admin {
        return Err(crate::utils::ApiError::forbidden(
            "Only super administrators can delete organizations",
        ));
    }

    tracing::warn!("Organization deletion request for ID: {} by user {}", id, org_ctx.user_id);

    state
        .organization_service
        .delete_organization(id, org_ctx.organization_id, org_ctx.is_super_admin)
        .await?;

    tracing::warn!("Organization deleted successfully: ID {} by user {}", id, org_ctx.user_id);
        crate::services::op_audit::log_op_best_effort(
        &state.db,
        crate::services::op_audit::OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "delete",
            target_type: "organization",
            target_id: Some(id),
            target_name: "",
        },
    )
    .await;
        
Ok(Json(serde_json::json!({"message": "Organization deleted successfully"})))
}
