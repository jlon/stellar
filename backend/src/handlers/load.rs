use axum::{
    Json,
    extract::{Path, Query, State},
};
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::models::{LoadJob, LoadListResponse, LoadQueryParams};
use crate::services::{MySQLClient, cluster_timeout, load_service};
use crate::utils::{ApiResult, get_active_cluster_for_org};

/// 查询当前组织活跃集群的导入任务。
#[utoipa::path(
    get,
    path = "/api/clusters/loads",
    params(
        ("db" = Option<String>, Query, description = "数据库名称"),
        ("type" = Option<String>, Query, description = "导入类型"),
        ("state" = Option<String>, Query, description = "任务状态"),
        ("search" = Option<String>, Query, description = "按作业 ID、Label、表名或错误信息搜索"),
        ("range" = Option<String>, Query, description = "时间范围：24h、7d、30d 或 all"),
        ("limit" = Option<u32>, Query, description = "返回条数，最多 500")
    ),
    responses(
        (status = 200, description = "导入任务列表", body = LoadListResponse),
        (status = 404, description = "没有可用的活跃集群")
    ),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
#[app_db]
pub async fn list_loads(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Query(params): Query<LoadQueryParams>,
) -> ApiResult<Json<LoadListResponse>> {
    let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));
    let response = load_service::list_loads(&client, cluster.cluster_type, &params).await?;
    Ok(Json(response))
}

/// 查询单个导入任务的完整详情。
#[utoipa::path(
    get,
    path = "/api/clusters/loads/{job_id}",
    params(
        ("job_id" = String, Path, description = "作业 ID"),
        ("db" = Option<String>, Query, description = "数据库名称")
    ),
    responses(
        (status = 200, description = "导入任务详情", body = LoadJob),
        (status = 404, description = "导入任务不存在")
    ),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
#[app_db]
pub async fn get_load(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(job_id): Path<String>,
    Query(mut params): Query<LoadQueryParams>,
) -> ApiResult<Json<LoadJob>> {
    let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));

    params.search = Some(job_id.clone());
    params.range = Some("all".to_string());
    params.limit = Some(500);
    let response = load_service::list_loads(&client, cluster.cluster_type, &params).await?;
    let job = response
        .items
        .into_iter()
        .find(|item| item.job_id.as_deref() == Some(job_id.as_str()))
        .ok_or_else(|| crate::utils::ApiError::not_found(format!("导入任务 {} 不存在", job_id)))?;
    Ok(Json(job))
}
