use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::IntoParams;

use crate::{
    utils::ApiResult,
    middleware::OrgContext,
    models::{
        CreateResourceGroupRequest, ResourceGroup, ResourceGroupUsage, ResourceUsageAnalysis,
        UpdateResourceGroupRequest,
    },
    services::ResourceGroupService,
    AppState,
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct AnalysisQuery {
    #[serde(default = "default_days")]
    days: u32,
}

fn default_days() -> u32 {
    30
}

#[utoipa::path(
    get,
    path = "/api/clusters/resource-groups",
    tag = "Resource Groups",
    responses(
        (status = 200, description = "Resource groups list", body = Vec<ResourceGroup>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_resource_groups(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
) -> ApiResult<Json<Vec<ResourceGroup>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let groups = ResourceGroupService::list_resource_groups(&pool).await?;
    Ok(Json(groups))
}

#[utoipa::path(
    get,
    path = "/api/clusters/resource-groups/{name}",
    tag = "Resource Groups",
    params(
        ("name" = String, Path, description = "Resource group name")
    ),
    responses(
        (status = 200, description = "Resource group details", body = ResourceGroup),
        (status = 404, description = "Resource group not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_resource_group(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(name): Path<String>,
) -> ApiResult<Json<ResourceGroup>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let group = ResourceGroupService::get_resource_group(&pool, &name).await?;
    Ok(Json(group))
}

#[utoipa::path(
    post,
    path = "/api/clusters/resource-groups",
    tag = "Resource Groups",
    request_body = CreateResourceGroupRequest,
    responses(
        (status = 201, description = "Resource group created"),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_resource_group(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(req): Json<CreateResourceGroupRequest>,
) -> ApiResult<StatusCode> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    ResourceGroupService::create_resource_group(&pool, req).await?;
    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    put,
    path = "/api/clusters/resource-groups/{name}",
    tag = "Resource Groups",
    params(
        ("name" = String, Path, description = "Resource group name")
    ),
    request_body = UpdateResourceGroupRequest,
    responses(
        (status = 200, description = "Resource group updated"),
        (status = 404, description = "Resource group not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn update_resource_group(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(name): Path<String>,
    Json(req): Json<UpdateResourceGroupRequest>,
) -> ApiResult<StatusCode> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    ResourceGroupService::update_resource_group(&pool, &name, req).await?;
    Ok(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/api/clusters/resource-groups/{name}",
    tag = "Resource Groups",
    params(
        ("name" = String, Path, description = "Resource group name")
    ),
    responses(
        (status = 204, description = "Resource group deleted"),
        (status = 404, description = "Resource group not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn delete_resource_group(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    ResourceGroupService::delete_resource_group(&pool, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/clusters/resource-groups/usage",
    tag = "Resource Groups",
    responses(
        (status = 200, description = "Resource group usage", body = Vec<ResourceGroupUsage>),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_resource_group_usage(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
) -> ApiResult<Json<Vec<ResourceGroupUsage>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let usage = ResourceGroupService::get_resource_group_usage(&pool).await?;
    Ok(Json(usage))
}

#[utoipa::path(
    get,
    path = "/api/clusters/resource-groups/analysis",
    tag = "Resource Groups",
    params(AnalysisQuery),
    responses(
        (status = 200, description = "Resource usage analysis", body = ResourceUsageAnalysis),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn analyze_resource_usage(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Query(query): Query<AnalysisQuery>,
) -> ApiResult<Json<ResourceUsageAnalysis>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let analysis = ResourceGroupService::analyze_resource_usage(&pool, query.days).await?;
    Ok(Json(analysis))
}
