//! 对话内动作确认 REST（授权执行的核心交互）。

use axum::Json;
use axum::extract::{Extension, Path, State};
use serde_json::json;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::db::AppDb;
use crate::middleware::OrgContext;
use crate::utils::{ApiError, ApiResult};

/// POST /api/agent/chat-actions/:id/confirm -- 用户确认 → 执行（单次）
/// session 归属从动作行读取校验（不信任客户端传入）。
#[app_db]
pub async fn confirm_chat_action<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = crate::services::ops_agent::chat_actions::ChatActionStore::new(
        state.ops_agent_service_pool(),
    );
    let action = store
        .get_by_id(id)
        .await
        .map_err(ApiError::not_found)?
        .ok_or_else(|| ApiError::not_found("动作不存在"))?;
    // 会话归属校验（owner-only）
    let session_store = crate::services::ai::AiSessionStore::new(state.ops_agent_service_pool());
    let meta = session_store
        .get_session(action.session_id, Some(org_ctx.user_id))
        .await
        .map_err(ApiError::not_found)?
        .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    let cluster = state.cluster_service.get_cluster(meta.cluster_id).await?;
    let view = store
        .confirm(&cluster, &state.mysql_pool_manager, id, action.session_id, &org_ctx.username)
        .await
        .map_err(ApiError::invalid_data)?;
    Ok(Json(json!({ "action": view })))
}

/// POST /api/agent/chat-actions/:id/cancel -- 用户拒绝
#[app_db]
pub async fn cancel_chat_action<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let store = crate::services::ops_agent::chat_actions::ChatActionStore::new(
        state.ops_agent_service_pool(),
    );
    let action = store
        .get_by_id(id)
        .await
        .map_err(ApiError::not_found)?
        .ok_or_else(|| ApiError::not_found("动作不存在"))?;
    let session_store = crate::services::ai::AiSessionStore::new(state.ops_agent_service_pool());
    session_store
        .get_session(action.session_id, Some(org_ctx.user_id))
        .await
        .map_err(ApiError::not_found)?
        .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    let view = store
        .cancel(id, action.session_id)
        .await
        .map_err(ApiError::invalid_data)?;
    Ok(Json(json!({ "action": view })))
}

/// GET /api/agent/chat-actions?session_id= -- 会话动作申请列表（审计/回放）
#[app_db]
pub async fn list_chat_actions<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    axum::extract::Query(params): axum::extract::Query<SessionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    // 归属校验：非本人会话拒绝
    let store = crate::services::ai::AiSessionStore::new(state.ops_agent_service_pool());
    let meta = store
        .get_session(params.session_id, Some(org_ctx.user_id))
        .await
        .map_err(ApiError::not_found)?
        .ok_or_else(|| ApiError::not_found("会话不存在"))?;
    let store = crate::services::ops_agent::chat_actions::ChatActionStore::new(
        state.ops_agent_service_pool(),
    );
    let items = store
        .list(meta.id)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(json!({ "items": items })))
}

#[derive(serde::Deserialize)]
pub struct SessionQuery {
    pub session_id: i64,
}
