//! 平台操作审计：记录增删改（不记查询），失败只打日志不阻断主流程。
use crate::db::query as db_query;
use sqlx::Pool;
use stellar_macros::app_db;

/// 一条操作审计记录。
pub struct OpAuditEntry<'a> {
    pub user_id: i64,
    pub username: &'a str,
    pub organization_id: Option<i64>,
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<i64>,
    pub target_name: &'a str,
}

/// 记录一条操作审计。失败时返回 Err，由调用方决定忽略（默认忽略）。
#[app_db]
pub async fn log_op<DB: crate::db::AppDb>(
    pool: &Pool<DB>,
    entry: OpAuditEntry<'_>,
) -> anyhow::Result<()> {
    db_query::query(
        "INSERT INTO op_audit_logs (user_id, username, organization_id, action, target_type, target_id, target_name) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(entry.user_id)
    .bind(entry.username)
    .bind(entry.organization_id)
    .bind(entry.action)
    .bind(entry.target_type)
    .bind(entry.target_id)
    .bind(entry.target_name)
    .execute(pool)
    .await?;
    Ok(())
}

/// fire-and-forget 包装：审计写失败只 warn，不断主流程。
#[app_db]
pub async fn log_op_best_effort<DB: crate::db::AppDb>(pool: &Pool<DB>, entry: OpAuditEntry<'_>) {
    if let Err(e) = log_op(pool, entry).await {
        tracing::warn!("Failed to write op audit log: {}", e);
    }
}
