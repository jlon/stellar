use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::handlers::query::parse_sql_statements;
use crate::{
    AppState,
    models::{
        CreateMaterializedViewRequest, MaterializedView, MaterializedViewDDL,
        MaterializedViewDependencies, MaterializedViewKind, MaterializedViewRef,
        RefreshMaterializedViewRequest, RenameMaterializedViewRequest,
        UpdateMaterializedViewStateRequest, UpdateRefreshScheduleRequest,
    },
    services::{
        create_adapter,
        op_audit::{OpAuditEntry, log_op_best_effort},
    },
    utils::ApiResult,
};

#[derive(Debug, Deserialize)]
pub struct ListMVParams {
    pub database: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CancelRefreshParams {
    #[serde(default)]
    pub force: bool,
}

type MaterializedViewPath = (String, String, MaterializedViewKind);

fn materialized_view_ref(path: MaterializedViewPath) -> ApiResult<MaterializedViewRef> {
    let (database, name, kind) = path;
    let reference = MaterializedViewRef { database, name, kind };
    reference.validate()?;
    Ok(reference)
}

pub(crate) fn validate_create_materialized_view_request(
    request: &CreateMaterializedViewRequest,
) -> ApiResult<()> {
    request.validate()?;
    if parse_sql_statements(&request.sql).len() != 1 {
        return Err(crate::utils::ApiError::invalid_data(
            "exactly one CREATE MATERIALIZED VIEW statement is required",
        ));
    }
    Ok(())
}

macro_rules! active_cluster {
    ($state:expr, $org_ctx:expr) => {
        if $org_ctx.is_super_admin {
            $state.cluster_service.get_active_cluster().await?
        } else {
            $state
                .cluster_service
                .get_active_cluster_by_org($org_ctx.organization_id)
                .await?
        }
    };
}

macro_rules! audit_materialized_view_write {
    ($state:expr, $org_ctx:expr, $action:expr, $target_name:expr) => {
        log_op_best_effort(
            &$state.db,
            OpAuditEntry {
                user_id: $org_ctx.user_id,
                username: &$org_ctx.username,
                organization_id: $org_ctx.organization_id,
                action: $action,
                target_type: "materialized_view",
                target_id: None,
                target_name: $target_name,
            },
        )
        .await
    };
}

/// GET /api/clusters/materialized_views
#[utoipa::path(
    get,
    path = "/api/clusters/materialized_views",
    params(("database" = Option<String>, Query, description = "Database filter")),
    responses((status = 200, body = Vec<MaterializedView>)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn list_materialized_views(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Query(params): Query<ListMVParams>,
) -> ApiResult<Json<Vec<MaterializedView>>> {
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    Ok(Json(
        adapter
            .list_materialized_views(params.database.as_deref())
            .await?,
    ))
}

/// GET /api/clusters/materialized_views/{database}/{name}/{kind}
#[utoipa::path(
    get,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    responses((status = 200, body = MaterializedView)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn get_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
) -> ApiResult<Json<MaterializedView>> {
    let reference = materialized_view_ref(path)?;
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    Ok(Json(adapter.get_materialized_view(&reference).await?))
}

/// GET /api/clusters/materialized_views/{database}/{name}/{kind}/ddl
#[utoipa::path(
    get,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/ddl",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    responses((status = 200, body = MaterializedViewDDL)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn get_materialized_view_ddl(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
) -> ApiResult<Json<MaterializedViewDDL>> {
    let reference = materialized_view_ref(path)?;
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let ddl = adapter.get_materialized_view_ddl(&reference).await?;
    Ok(Json(MaterializedViewDDL { object: reference, ddl }))
}

/// GET /api/clusters/materialized_views/{database}/{name}/{kind}/dependencies
#[utoipa::path(
    get,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/dependencies",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    responses((status = 200, body = MaterializedViewDependencies)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn get_materialized_view_dependencies(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
) -> ApiResult<Json<MaterializedViewDependencies>> {
    let reference = materialized_view_ref(path)?;
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    Ok(Json(
        adapter
            .get_materialized_view_dependencies(&reference)
            .await?,
    ))
}

/// POST /api/clusters/materialized_views
#[utoipa::path(
    post,
    path = "/api/clusters/materialized_views",
    request_body = CreateMaterializedViewRequest,
    responses((status = 201)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn create_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Json(request): Json<CreateMaterializedViewRequest>,
) -> ApiResult<impl IntoResponse> {
    validate_create_materialized_view_request(&request)?;
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter.create_materialized_view(&request.sql).await?;
    audit_materialized_view_write!(
        state,
        org_ctx,
        "materialized_views:create",
        "CREATE MATERIALIZED VIEW"
    );
    Ok((StatusCode::CREATED, Json(json!({ "message": "Materialized view created successfully" }))))
}

/// DELETE /api/clusters/materialized_views/{database}/{name}/{kind}
#[utoipa::path(
    delete,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn delete_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter.drop_materialized_view(&reference).await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:delete", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Materialized view deleted successfully" }))))
}

/// POST /api/clusters/materialized_views/{database}/{name}/{kind}/refresh
#[utoipa::path(
    post,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/refresh",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    request_body = RefreshMaterializedViewRequest,
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn refresh_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
    Json(request): Json<RefreshMaterializedViewRequest>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    request.validate()?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter
        .refresh_materialized_view(&reference, &request)
        .await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:refresh", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Refresh initiated" }))))
}

/// POST /api/clusters/materialized_views/{database}/{name}/{kind}/cancel
#[utoipa::path(
    post,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/cancel",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind"), ("force" = bool, Query, description = "Force cancellation")),
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn cancel_refresh_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
    Query(params): Query<CancelRefreshParams>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter
        .cancel_materialized_view_refresh(&reference, params.force)
        .await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:cancel", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Refresh cancelled" }))))
}

/// PUT /api/clusters/materialized_views/{database}/{name}/{kind}/state
#[utoipa::path(
    put,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/state",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    request_body = UpdateMaterializedViewStateRequest,
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn update_materialized_view_state(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
    Json(request): Json<UpdateMaterializedViewStateRequest>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter
        .set_materialized_view_state(&reference, request.state)
        .await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:state", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Materialized view state updated" }))))
}

/// PUT /api/clusters/materialized_views/{database}/{name}/{kind}/rename
#[utoipa::path(
    put,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/rename",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    request_body = RenameMaterializedViewRequest,
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn rename_materialized_view(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
    Json(request): Json<RenameMaterializedViewRequest>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    request.validate()?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter
        .rename_materialized_view(&reference, &request.new_name)
        .await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:rename", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Materialized view renamed" }))))
}

/// PUT /api/clusters/materialized_views/{database}/{name}/{kind}/refresh-schedule
#[utoipa::path(
    put,
    path = "/api/clusters/materialized_views/{database}/{name}/{kind}/refresh-schedule",
    params(("database" = String, Path, description = "Database name"), ("name" = String, Path, description = "Materialized view name"), ("kind" = MaterializedViewKind, Path, description = "Materialized view kind")),
    request_body = UpdateRefreshScheduleRequest,
    responses((status = 200)),
    security(("bearer_auth" = [])),
    tag = "Materialized Views"
)]
#[app_db]
pub async fn update_materialized_view_refresh_schedule(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(path): Path<MaterializedViewPath>,
    Json(request): Json<UpdateRefreshScheduleRequest>,
) -> ApiResult<impl IntoResponse> {
    let reference = materialized_view_ref(path)?;
    request.validate()?;
    let target = reference.display_name();
    let cluster = active_cluster!(state, org_ctx);
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter
        .update_materialized_view_refresh_schedule(&reference, request.schedule)
        .await?;
    audit_materialized_view_write!(state, org_ctx, "materialized_views:refresh_schedule", &target);
    Ok((StatusCode::OK, Json(json!({ "message": "Materialized view refresh schedule updated" }))))
}
