//! 用户通知 REST（铃铛）：列表 / 创建 / 标记已读。资源按 user_id 隔离。

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::db::AppDb;
use crate::middleware::OrgContext;
use crate::utils::{ApiError, ApiResult};
use axum::extract::Extension;

#[derive(Deserialize)]
pub struct NotificationQuery {
    pub unread_only: Option<bool>,
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateNotification {
    pub kind: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub severity: Option<String>,
    pub meta: Option<serde_json::Value>,
}

/// GET /api/notifications?unread_only=&limit=
#[app_db]
pub async fn list<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Query(params): Query<NotificationQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    let data = state
        .notification_service
        .list(org_ctx.user_id, params.unread_only.unwrap_or(false), limit)
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(data))
}

/// POST /api/notifications -- 客户端在异步任务完成时创建通知
/// （如：用户离开会话页后 LLM 诊断完成）。
#[app_db]
pub async fn create<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(req): Json<CreateNotification>,
) -> ApiResult<Json<serde_json::Value>> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(ApiError::invalid_data("通知标题不能为空"));
    }
    let severity = match req.severity.as_deref() {
        Some("warning" | "critical" | "info") => req.severity.as_deref().unwrap(),
        _ => "info",
    };
    let id = state
        .notification_service
        .create(
            org_ctx.user_id,
            crate::services::notification_service::NewNotification {
                kind: req.kind.clone().unwrap_or_else(|| "system".to_string()),
                title: title.to_string(),
                body: req.body.clone(),
                link: req.link.clone(),
                severity: severity.to_string(),
                meta: req.meta.clone(),
            },
        )
        .await
        .map_err(ApiError::internal_error)?;
    Ok(Json(json!({ "id": id })))
}

/// POST /api/notifications/:id/read
#[app_db]
pub async fn mark_read<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let ok = state
        .notification_service
        .mark_read(org_ctx.user_id, id)
        .await
        .map_err(ApiError::internal_error)?;
    if !ok {
        return Err(ApiError::not_found("通知不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}
