use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::AppState;
use crate::middleware::OrgContext;
use crate::models::QueryExecutionHistoryResponse;
use crate::services::QueryExecutionHistoryService;
use crate::utils::ApiResult;

#[derive(Debug, Deserialize)]
pub struct HistoryQueryParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

#[utoipa::path(
    get,
    path = "/api/clusters/queries/execution-history",
    params(
        ("limit" = Option<i64>, Query, description = "Number of records to return (default: 50)"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination (default: 0)")
    ),
    responses(
        (status = 200, description = "Query execution history", body = QueryExecutionHistoryResponse),
        (status = 404, description = "No active cluster found")
    ),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
pub async fn list_execution_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    axum::extract::Query(params): axum::extract::Query<HistoryQueryParams>,
) -> ApiResult<Json<QueryExecutionHistoryResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let service = QueryExecutionHistoryService::new(state.db.clone());
    let (data, total) = service
        .list_history(org_ctx.user_id, cluster.id, params.limit, params.offset)
        .await?;

    Ok(Json(QueryExecutionHistoryResponse { data, total }))
}

#[utoipa::path(
    delete,
    path = "/api/clusters/queries/execution-history/{id}",
    params(
        ("id" = i64, Path, description = "History record ID")
    ),
    responses(
        (status = 200, description = "History record deleted"),
        (status = 404, description = "Record not found")
    ),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
pub async fn delete_execution_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Path(id): Path<i64>,
) -> ApiResult<impl IntoResponse> {
    let service = QueryExecutionHistoryService::new(state.db.clone());
    let deleted = service.delete_history(org_ctx.user_id, id).await?;

    if deleted {
        Ok((StatusCode::OK, Json(json!({ "message": "History record deleted" }))))
    } else {
        Ok((StatusCode::NOT_FOUND, Json(json!({ "message": "Record not found" }))))
    }
}

#[utoipa::path(
    delete,
    path = "/api/clusters/queries/execution-history",
    responses(
        (status = 200, description = "All history cleared"),
        (status = 404, description = "No active cluster found")
    ),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
pub async fn clear_execution_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let service = QueryExecutionHistoryService::new(state.db.clone());
    let count = service.clear_history(org_ctx.user_id, cluster.id).await?;

    Ok((StatusCode::OK, Json(json!({ "message": format!("Cleared {} history records", count) }))))
}
