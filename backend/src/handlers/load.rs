use axum::{
    extract::{Path, Query, State},
    Json,
};
use std::sync::Arc;
use stellar_macros::app_db;

use crate::models::{LoadJob, LoadListResponse, LoadQueryParams};
use crate::services::{cluster_timeout, load_service, MySQLClient};
use crate::utils::{get_active_cluster_for_org, ApiResult};
use crate::AppState;

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
        ("limit" = Option<u32>, Query, description = "返回条数，最多 500"),
        ("cursor" = Option<String>, Query, description = "上一页末尾位置，由响应 next_cursor 返回")
    ),
    responses(
        (status = 200, description = "导入任务列表", body = LoadListResponse),
        (status = 404, description = "没有可用的活跃集群")
    ),
    security(("bearer_auth" = [])),
    tag = "Load Management"
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
    tag = "Load Management"
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

    params.job_id = Some(job_id.clone());
    params.search = None;
    params.range = Some("all".to_string());
    params.limit = Some(500);
    params.cursor = None;
    let response = load_service::list_loads(&client, cluster.cluster_type, &params).await?;
    let mut job = response
        .items
        .into_iter()
        .find(|item| item.job_id.as_deref() == Some(job_id.as_str()))
        .ok_or_else(|| crate::utils::ApiError::not_found(format!("导入任务 {} 不存在", job_id)))?;
    if cluster.cluster_type == crate::models::ClusterType::StarRocks
        && job.load_type == "ROUTINE_LOAD"
    {
        match load_service::load_routine_details(&client, &job).await {
            Ok(Some(details)) => {
                if job.tracking_sql.is_none() {
                    job.tracking_sql = details.tracking_sql.clone();
                }
                job.routine_load = Some(details);
            },
            Ok(None) => {},
            Err(error) => {
                tracing::debug!(error = %error, job_id = %job_id, "routine load details unavailable");
            },
        }
    }
    if cluster.cluster_type == crate::models::ClusterType::Doris {
        match load_service::load_doris_failure_details(&client, &job).await {
            Ok(details) => job.doris_failure = details,
            Err(error) => {
                tracing::debug!(error = %error, job_id = %job_id, "Doris load failure details unavailable");
            },
        }
    }
    Ok(Json(job))
}
