//! Agent 事件闭环 REST endpoints（事件 / Incident 只读 + 手动调查/关闭）。

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::db::AppDb;
use crate::middleware::OrgContext;
use crate::utils::{ApiError, ApiResult};

#[derive(Deserialize)]
pub struct IncidentQuery {
    pub cluster_id: i64,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct EventQuery {
    pub cluster_id: i64,
    pub state: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/agent/incidents?cluster_id=&status=&limit=
#[app_db]
pub async fn list_incidents<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Query(params): Query<IncidentQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let items = state
        .agent_runtime_service
        .list_incidents(params.cluster_id, params.status.as_deref(), limit)
        .await?;
    Ok(Json(json!({ "items": items })))
}

/// GET /api/agent/incidents/:id -- incident + events + evidences + decisions
#[app_db]
pub async fn get_incident<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path(incident_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if incident_id <= 0 {
        return Err(ApiError::validation_error("Invalid incident id"));
    }
    Ok(Json(
        state
            .agent_runtime_service
            .get_incident_detail(incident_id)
            .await?,
    ))
}

/// POST /api/agent/incidents/:id/investigate -- 手动取证 + 规则诊断
#[app_db]
pub async fn investigate<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path(incident_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if incident_id <= 0 {
        return Err(ApiError::validation_error("Invalid incident id"));
    }
    let outcome = state
        .agent_runtime_service
        .investigate_by_id(incident_id)
        .await?;
    Ok(Json(json!({ "diagnosis": outcome })))
}

/// POST /api/agent/incidents/:id/analyze -- LLM 根因推理（含证据校验与回退）
#[app_db]
pub async fn analyze<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path(incident_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if incident_id <= 0 {
        return Err(ApiError::validation_error("Invalid incident id"));
    }
    let analysis = state
        .agent_runtime_service
        .llm_analyze_incident(incident_id)
        .await?;
    Ok(Json(json!({ "analysis": analysis })))
}

/// POST /api/agent/incidents/:id/close
#[app_db]
pub async fn close_incident<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path(incident_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if incident_id <= 0 {
        return Err(ApiError::validation_error("Invalid incident id"));
    }
    state
        .agent_runtime_service
        .close_incident(incident_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(json!({ "message": "Incident closed" })))
}

/// POST /api/agent/incidents/:id/actions -- 创建待确认动作（两阶段确认第一步）
#[app_db]
pub async fn create_action<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(incident_id): Path<i64>,
    Json(req): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    let kind = req
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let params = req
        .get("params")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let cluster = resolve_incident_cluster(&state, &org_ctx, incident_id).await?;
    let action = state
        .agent_runtime_service
        .create_action(&cluster, incident_id, &kind, &params, &org_ctx.username)
        .await
        .map_err(ApiError::invalid_data)?;
    Ok(Json(json!({ "action": action })))
}

/// GET /api/agent/incidents/:id/actions -- 动作列表
#[app_db]
pub async fn list_actions<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path(incident_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let items = state
        .agent_runtime_service
        .list_actions(incident_id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(json!({ "items": items })))
}

/// POST /api/agent/incidents/:id/actions/:aid/confirm -- 确认并执行
#[app_db]
pub async fn confirm_action<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path((incident_id, action_id)): Path<(i64, i64)>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = resolve_incident_cluster(&state, &org_ctx, incident_id).await?;
    let action = state
        .agent_runtime_service
        .confirm_action(&cluster, incident_id, action_id, &org_ctx.username)
        .await
        .map_err(ApiError::invalid_data)?;
    Ok(Json(json!({ "action": action })))
}

/// POST /api/agent/incidents/:id/actions/:aid/cancel -- 取消待处理动作
#[app_db]
pub async fn cancel_action<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Path((incident_id, action_id)): Path<(i64, i64)>,
) -> ApiResult<Json<serde_json::Value>> {
    let action = state
        .agent_runtime_service
        .cancel_action(incident_id, action_id)
        .await
        .map_err(ApiError::invalid_data)?;
    Ok(Json(json!({ "action": action })))
}

/// 解析 incident 归属集群（动作执行需要真实集群连接）。
#[app_db]
async fn resolve_incident_cluster<DB: AppDb>(
    state: &Arc<AppState<DB>>,
    org_ctx: &OrgContext,
    incident_id: i64,
) -> ApiResult<crate::models::cluster::Cluster> {
    let detail = state
        .agent_runtime_service
        .get_incident_detail(incident_id)
        .await?;
    // detail = { incident: {...}, decisions, events, evidences }
    let cluster_id = detail
        .get("incident")
        .and_then(|v| v.get("cluster_id"))
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ApiError::not_found("Incident 不存在"))?;
    let cluster = state.cluster_service.get_cluster(cluster_id).await?;
    if !org_ctx.is_super_admin && cluster.organization_id != org_ctx.organization_id {
        return Err(ApiError::forbidden("无权访问该 Incident"));
    }
    Ok(cluster)
}

/// GET /api/agent/events?cluster_id=&state=&limit=
#[app_db]
pub async fn list_events<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Query(params): Query<EventQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    let items = state
        .agent_runtime_service
        .list_events(params.cluster_id, params.state.as_deref(), limit)
        .await?;
    Ok(Json(json!({ "items": items })))
}
