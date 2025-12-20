use axum::{
    extract::{Path, State, Extension, Query},
    Json,
};
use std::sync::Arc;
use sqlx::Row;

use crate::AppState;
use crate::models::{
    SubmitRequestDto, ApprovalDto, PermissionRequestResponse, RequestQueryFilter,
    PaginatedResponse, DbAccountDto, DbRoleDto,
};
use crate::utils::ApiResult;

/// List my permission requests
#[utoipa::path(
    get,
    path = "/api/permission-requests/my",
    params(
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("request_type" = Option<String>, Query, description = "Filter by request type"),
        ("page" = Option<i64>, Query, description = "Page number"),
        ("page_size" = Option<i64>, Query, description = "Page size"),
    ),
    responses(
        (status = 200, description = "List of my requests", body = PaginatedResponse<PermissionRequestResponse>),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn list_my_requests(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Query(filter): Query<RequestQueryFilter>,
) -> ApiResult<Json<PaginatedResponse<PermissionRequestResponse>>> {
    tracing::debug!("User {} listing their requests", user_id);

    let result = state.permission_request_service.list_my_requests(user_id, filter).await?;
    Ok(Json(result))
}

/// List pending approval requests (for approvers)
#[utoipa::path(
    get,
    path = "/api/permission-requests/pending",
    params(
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("request_type" = Option<String>, Query, description = "Filter by request type"),
        ("page" = Option<i64>, Query, description = "Page number"),
        ("page_size" = Option<i64>, Query, description = "Page size"),
    ),
    responses(
        (status = 200, description = "List of pending requests for approval", body = Vec<PermissionRequestResponse>),
        (status = 401, description = "Unauthorized")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn list_pending_approvals(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Query(filter): Query<RequestQueryFilter>,
) -> ApiResult<Json<Vec<PermissionRequestResponse>>> {
    tracing::debug!("User {} listing pending approvals", user_id);

    // Get user's org_id from database
    let user = sqlx::query(
        "SELECT organization_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let org_id: Option<i64> = user.get("organization_id");
    let org_id = org_id.unwrap_or(0);

    // TODO: Check if user is super_admin from roles table
    // For now, assume any user can view approvals for their org
    let is_super_admin = false;

    let result = state.permission_request_service
        .list_pending_approvals(org_id, is_super_admin, filter)
        .await?;

    Ok(Json(result))
}

/// Get request details
#[utoipa::path(
    get,
    path = "/api/permission-requests/{request_id}",
    params(
        ("request_id" = i64, Path, description = "Request ID"),
    ),
    responses(
        (status = 200, description = "Request details", body = PermissionRequestResponse),
        (status = 404, description = "Request not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn get_request(
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<i64>,
) -> ApiResult<Json<PermissionRequestResponse>> {
    tracing::debug!("Getting request details for request_id: {}", request_id);

    let request = state.permission_request_service.get_request_detail(request_id).await?;
    Ok(Json(request))
}

/// Submit a new permission request
#[utoipa::path(
    post,
    path = "/api/permission-requests",
    request_body = SubmitRequestDto,
    responses(
        (status = 200, description = "Request submitted successfully", body = i64),
        (status = 400, description = "Bad request")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn submit_request(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Json(req): Json<SubmitRequestDto>,
) -> ApiResult<Json<i64>> {
    tracing::info!("User {} submitting permission request", user_id);

    let request_id = state.permission_request_service.submit_request(user_id, req).await?;

    tracing::info!("Permission request created: id={}", request_id);
    Ok(Json(request_id))
}

/// Approve a permission request
#[utoipa::path(
    post,
    path = "/api/permission-requests/{request_id}/approve",
    params(
        ("request_id" = i64, Path, description = "Request ID"),
    ),
    request_body = ApprovalDto,
    responses(
        (status = 200, description = "Request approved successfully"),
        (status = 404, description = "Request not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn approve_request(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Path(request_id): Path<i64>,
    Json(dto): Json<ApprovalDto>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::info!("User {} approving request {}", user_id, request_id);

    state.permission_request_service.approve_request(request_id, user_id, dto).await?;

    tracing::info!("Request {} approved by user {}", request_id, user_id);
    Ok(Json(serde_json::json!({"status": "approved"})))
}

/// Reject a permission request
#[utoipa::path(
    post,
    path = "/api/permission-requests/{request_id}/reject",
    params(
        ("request_id" = i64, Path, description = "Request ID"),
    ),
    request_body = ApprovalDto,
    responses(
        (status = 200, description = "Request rejected successfully"),
        (status = 404, description = "Request not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn reject_request(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Path(request_id): Path<i64>,
    Json(dto): Json<ApprovalDto>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::info!("User {} rejecting request {}", user_id, request_id);

    state.permission_request_service.reject_request(request_id, user_id, dto).await?;

    tracing::info!("Request {} rejected by user {}", request_id, user_id);
    Ok(Json(serde_json::json!({"status": "rejected"})))
}

/// Cancel a pending request (by applicant only)
#[utoipa::path(
    post,
    path = "/api/permission-requests/{request_id}/cancel",
    params(
        ("request_id" = i64, Path, description = "Request ID"),
    ),
    responses(
        (status = 200, description = "Request cancelled successfully"),
        (status = 403, description = "Forbidden"),
        (status = 404, description = "Request not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Permission Requests"
)]
pub async fn cancel_request(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
    Path(request_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::info!("User {} cancelling request {}", user_id, request_id);

    state.permission_request_service.cancel_request(request_id, user_id).await?;

    tracing::info!("Request {} cancelled by user {}", request_id, user_id);
    Ok(Json(serde_json::json!({"status": "cancelled"})))
}

/// List database accounts (real-time query)
#[utoipa::path(
    get,
    path = "/api/clusters/{cluster_id}/db-auth/accounts",
    params(
        ("cluster_id" = i64, Path, description = "Cluster ID"),
    ),
    responses(
        (status = 200, description = "List of database accounts", body = Vec<DbAccountDto>),
        (status = 404, description = "Cluster not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Database Authentication"
)]
pub async fn list_db_accounts(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<i64>,
) -> ApiResult<Json<Vec<DbAccountDto>>> {
    tracing::debug!("Listing database accounts for cluster {}", cluster_id);

    let accounts = state.db_auth_query_service.list_accounts(cluster_id).await?;
    Ok(Json(accounts))
}

/// List database roles (real-time query)
#[utoipa::path(
    get,
    path = "/api/clusters/{cluster_id}/db-auth/roles",
    params(
        ("cluster_id" = i64, Path, description = "Cluster ID"),
    ),
    responses(
        (status = 200, description = "List of database roles", body = Vec<DbRoleDto>),
        (status = 404, description = "Cluster not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Database Authentication"
)]
pub async fn list_db_roles(
    State(state): State<Arc<AppState>>,
    Path(cluster_id): Path<i64>,
) -> ApiResult<Json<Vec<DbRoleDto>>> {
    tracing::debug!("Listing database roles for cluster {}", cluster_id);

    let roles = state.db_auth_query_service.list_roles(cluster_id).await?;
    Ok(Json(roles))
}

/// Preview SQL for permission request
#[utoipa::path(
    post,
    path = "/api/db-auth/preview-sql",
    request_body = SubmitRequestDto,
    responses(
        (status = 200, description = "SQL preview", body = serde_json::Value),
        (status = 400, description = "Bad request")
    ),
    security(("bearer_auth" = [])),
    tag = "Database Authentication"
)]
pub async fn preview_sql(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<SubmitRequestDto>,
) -> ApiResult<Json<serde_json::Value>> {
    tracing::debug!("Previewing SQL for request type: {}", req.request_type);

    // Use the static method from permission_request_service
    let sql = crate::services::PermissionRequestService::generate_preview_sql_static(&req.request_type, &req.request_details)?;

    Ok(Json(serde_json::json!({
        "sql": sql,
        "request_type": req.request_type
    })))
}

/// List database accounts for active cluster
#[utoipa::path(
    get,
    path = "/api/clusters/db-auth/accounts",
    responses(
        (status = 200, description = "List of database accounts", body = Vec<DbAccountDto>),
        (status = 404, description = "No active cluster found")
    ),
    security(("bearer_auth" = [])),
    tag = "Database Authentication"
)]
pub async fn list_db_accounts_active(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
) -> ApiResult<Json<Vec<DbAccountDto>>> {
    tracing::debug!("Listing database accounts for active cluster of user {}", user_id);

    // Get user's organization_id
    let user = sqlx::query(
        "SELECT organization_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let org_id: Option<i64> = user.get("organization_id");

    // Get active cluster for this organization
    let active_cluster = state.cluster_service.get_active_cluster_by_org(org_id).await?;

    // List accounts for the active cluster
    let accounts = state.db_auth_query_service.list_accounts(active_cluster.id).await?;
    Ok(Json(accounts))
}

/// List database roles for active cluster
#[utoipa::path(
    get,
    path = "/api/clusters/db-auth/roles",
    responses(
        (status = 200, description = "List of database roles", body = Vec<DbRoleDto>),
        (status = 404, description = "No active cluster found")
    ),
    security(("bearer_auth" = [])),
    tag = "Database Authentication"
)]
pub async fn list_db_roles_active(
    State(state): State<Arc<AppState>>,
    Extension(user_id): Extension<i64>,
) -> ApiResult<Json<Vec<DbRoleDto>>> {
    tracing::debug!("Listing database roles for active cluster of user {}", user_id);

    // Get user's organization_id
    let user = sqlx::query(
        "SELECT organization_id FROM users WHERE id = ?"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let org_id: Option<i64> = user.get("organization_id");

    // Get active cluster for this organization
    let active_cluster = state.cluster_service.get_active_cluster_by_org(org_id).await?;

    // List roles for the active cluster
    let roles = state.db_auth_query_service.list_roles(active_cluster.id).await?;
    Ok(Json(roles))
}
