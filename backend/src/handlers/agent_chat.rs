//! AI Agent chat & session REST endpoints.

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use stellar_macros::app_db;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::db::AppDb;
use crate::middleware::OrgContext;
use crate::utils::{ApiError, ApiResult, check_org_access};

struct CancelOnDrop(CancellationToken);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[derive(Deserialize)]
pub struct ChatRequest {
    /// Reuse an existing session; when absent a new one is created.
    pub session_id: Option<i64>,
    /// Explicit cluster id; when absent the org's active cluster is used.
    pub cluster_id: Option<i64>,
    pub message: String,
    /// 受限页面上下文，仅支持已定义的页面类型和参数。
    #[serde(default)]
    pub context: Option<crate::services::ops_agent::context::PageContext>,
}

#[derive(Deserialize)]
pub struct SessionQuery {
    pub cluster_id: i64,
}

/// Resolve the chat target cluster following the org scoping rules.
#[app_db]
async fn resolve_chat_cluster<DB: AppDb>(
    state: &Arc<AppState<DB>>,
    org_ctx: &OrgContext,
    req: &ChatRequest,
) -> ApiResult<crate::models::cluster::Cluster> {
    if let Some(context) = req.context.as_ref() {
        context.validate().map_err(ApiError::validation_error)?;
    }
    // Non-super-admin resolves strictly within their organization.
    let cluster = if org_ctx.is_super_admin {
        state
            .ops_agent_service
            .resolve_cluster(req.cluster_id)
            .await?
    } else {
        match req.cluster_id {
            Some(id) => state.cluster_service.get_cluster(id).await?,
            None => {
                state
                    .cluster_service
                    .get_active_cluster_by_org(org_ctx.organization_id)
                    .await?
            },
        }
    };
    validate_chat_cluster_access(&cluster, org_ctx)?;
    Ok(cluster)
}

pub(crate) fn validate_chat_cluster_access(
    cluster: &crate::models::cluster::Cluster,
    org_ctx: &OrgContext,
) -> ApiResult<()> {
    check_org_access(org_ctx, cluster.organization_id, "use the AI assistant")
}

/// POST /api/agent/chat
#[app_db]
pub async fn chat<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(req): Json<ChatRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = resolve_chat_cluster(&state, &org_ctx, &req).await?;
    let turn = crate::services::ops_agent::ChatTurnRequest {
        session_id: req.session_id,
        user_id: org_ctx.user_id,
        created_by: &org_ctx.username,
        user_message: &req.message,
        page_context: req.context.as_ref(),
    };

    let outcome = state.ops_agent_service.chat(&cluster, &turn).await?;
    Ok(Json(json!({
        "session_id": outcome.session_id,
        "cluster_id": outcome.cluster_id,
        "steps": outcome.steps,
        "final_answer": outcome.final_answer,
    })))
}

/// POST /api/agent/chat/stream -- 步骤级 SSE 流式（事件：step/answer/done/error）。
/// 心跳由 axum KeepAlive 每 30s 发注释行；keep-alive 由响应流自身驱动，
/// 连接关闭即自动终止，不存在独立心跳任务泄漏（Flink hotfix 77f03445a97/76594c573b8 教训）。
#[app_db]
pub async fn stream_chat<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(req): Json<ChatRequest>,
) -> ApiResult<Response> {
    let cluster = resolve_chat_cluster(&state, &org_ctx, &req).await?;
    let cancellation = CancellationToken::new();
    let cancel_on_drop = CancelOnDrop(cancellation.clone());
    let turn = crate::services::ops_agent::ChatTurnRequest {
        session_id: req.session_id,
        user_id: org_ctx.user_id,
        created_by: &org_ctx.username,
        user_message: &req.message,
        page_context: req.context.as_ref(),
    };
    let mut rx = state
        .ops_agent_service
        .chat_stream(&cluster, &turn, cancellation.clone())
        .await?;

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
    let forwarder_cancellation = cancellation.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let (name, data) = match ev {
                crate::services::ops_agent::ChatStreamEvent::Step { step } => {
                    ("step", serde_json::to_string(&step).unwrap_or_default())
                },
                crate::services::ops_agent::ChatStreamEvent::Delta { text } => ("delta", text),
                crate::services::ops_agent::ChatStreamEvent::Phase { phase } => {
                    ("phase", json!({ "phase": phase }).to_string())
                },
                crate::services::ops_agent::ChatStreamEvent::ActionRequest { action } => {
                    ("action_request", action.to_string())
                },
                crate::services::ops_agent::ChatStreamEvent::Answer { final_answer } => {
                    ("answer", final_answer)
                },
                crate::services::ops_agent::ChatStreamEvent::Done { session_id, usage_tokens } => (
                    "done",
                    json!({ "session_id": session_id, "usage_tokens": usage_tokens }).to_string(),
                ),
                crate::services::ops_agent::ChatStreamEvent::Error { message } => {
                    ("error", message)
                },
            };
            if event_tx
                .send(Ok(Event::default().event(name).data(data)))
                .is_err()
            {
                forwarder_cancellation.cancel();
                break;
            }
        }
    });

    // Flink's AiChatStreamHandler explicitly sets this header. Without it,
    // Nginx/Ingress may buffer token frames until the whole response completes,
    // making a correct SSE backend appear non-streaming in the browser.
    let event_stream = futures::stream::unfold(
        (event_rx, cancel_on_drop),
        |(mut rx, cancel_on_drop)| async move {
            rx.recv().await.map(|event| (event, (rx, cancel_on_drop)))
        },
    );
    let mut response = Sse::new(event_stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(30))
                .text("ping"),
        )
        .into_response();
    response
        .headers_mut()
        .insert(HeaderName::from_static("x-accel-buffering"), HeaderValue::from_static("no"));
    Ok(response)
}

/// GET /api/agent/sessions?cluster_id=..
#[app_db]
pub async fn list_sessions<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Query(params): Query<SessionQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let sessions = state
        .ops_agent_service
        .list_sessions(params.cluster_id, org_ctx.user_id)
        .await?;
    Ok(Json(json!({ "items": sessions })))
}

/// GET /api/agent/sessions/:id -- full transcript with tool-call steps.
#[app_db]
pub async fn get_session<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(session_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if session_id <= 0 {
        return Err(ApiError::validation_error("Invalid session id"));
    }
    let owner = if org_ctx.is_super_admin { None } else { Some(org_ctx.user_id) };
    let messages = state
        .ops_agent_service
        .get_session(session_id, owner, Some(org_ctx.user_id))
        .await?;
    Ok(Json(json!({ "items": messages })))
}

/// POST /api/agent/messages/:id/feedback -- 回答点赞/点踩（GPT 同款）
#[derive(Deserialize)]
pub struct FeedbackRequest {
    /// up / down；null/缺省 = 取消评价（切换式点赞）。
    pub rating: Option<String>,
}

#[app_db]
pub async fn set_message_feedback<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(message_id): Path<i64>,
    Json(req): Json<FeedbackRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if message_id <= 0 {
        return Err(ApiError::validation_error("Invalid message id"));
    }
    let owner = if org_ctx.is_super_admin { None } else { Some(org_ctx.user_id) };
    let feedback = state
        .ops_agent_service
        .set_message_feedback(message_id, owner, org_ctx.user_id, req.rating)
        .await?;
    Ok(Json(json!({ "message_id": message_id, "feedback": feedback })))
}

/// PATCH /api/agent/sessions/:id -- 会话重命名（GPT 同款）
#[derive(Deserialize)]
pub struct RenameRequest {
    pub title: String,
}

#[app_db]
pub async fn rename_session<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(session_id): Path<i64>,
    Json(req): Json<RenameRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if session_id <= 0 {
        return Err(ApiError::validation_error("Invalid session id"));
    }
    let owner = if org_ctx.is_super_admin { None } else { Some(org_ctx.user_id) };
    let title = state
        .ops_agent_service
        .rename_session(session_id, owner, &req.title)
        .await?;
    Ok(Json(json!({ "id": session_id, "title": title })))
}

/// DELETE /api/agent/sessions/:id
#[app_db]
pub async fn delete_session<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(session_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    if session_id <= 0 {
        return Err(ApiError::validation_error("Invalid session id"));
    }
    let owner = if org_ctx.is_super_admin { None } else { Some(org_ctx.user_id) };
    state
        .ops_agent_service
        .delete_session(session_id, owner)
        .await?;
    Ok(Json(json!({ "message": "Session deleted" })))
}
