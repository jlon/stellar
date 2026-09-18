//! 平台操作审计查询：谁、何时、做了什么（只记增删改）。
use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Deserialize;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::db::AppDb;
use crate::db::query as db_query;
use crate::middleware::OrgContext;
use crate::utils::ApiResult;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct OpAuditLog {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub organization_id: Option<i64>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<i64>,
    pub target_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct OpAuditQuery {
    pub target_type: Option<String>,
    pub action: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/op-audit-logs?target_type=&action=&limit=
#[app_db]
pub async fn list<DB: AppDb>(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Query(params): Query<OpAuditQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    let limit = params.limit.unwrap_or(50).clamp(1, 200);
    // 超管看全部，普通用户看本组织的
    let rows: Vec<OpAuditLog> = if org_ctx.is_super_admin {
        db_query::query_as(
            "SELECT id, user_id, username, organization_id, action, target_type, target_id, target_name, created_at \
             FROM op_audit_logs \
             WHERE (? IS NULL OR target_type = ?) AND (? IS NULL OR action = ?) \
             ORDER BY id DESC LIMIT ?",
        )
        .bind(&params.target_type)
        .bind(&params.target_type)
        .bind(&params.action)
        .bind(&params.action)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        db_query::query_as(
            "SELECT id, user_id, username, organization_id, action, target_type, target_id, target_name, created_at \
             FROM op_audit_logs \
             WHERE organization_id = ? AND (? IS NULL OR target_type = ?) AND (? IS NULL OR action = ?) \
             ORDER BY id DESC LIMIT ?",
        )
        .bind(org_ctx.organization_id)
        .bind(&params.target_type)
        .bind(&params.target_type)
        .bind(&params.action)
        .bind(&params.action)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    };
    Ok(Json(serde_json::json!({ "items": rows })))
}
