//! 对话内授权执行（Chat Actions）：LLM 通过受控工具 `propose_action` 提交动作申请，
//! 用户确认后才经 MySQLClient 通道执行（复用 actions.rs 的参数校验与执行器）。
//!
//! 审计链（高危动作强制）：申请用户/LLM 理由/确认人/参数/结果/时间 全量留档。
//!
//! 协议（对齐 Flink PENDING_ACTION）：工具返回 `ACTION_PENDING:{json}` 前缀文本，
//! agent 循环截获后推 SSE `action_request` 事件，前端渲染确认卡片。

use serde::Serialize;
use sqlx::Row;
use stellar_macros::app_impl;

use crate::db::dialect::RowsAffected;
use crate::db::{AppDb, query as db_query};
use crate::models::cluster::Cluster;
use crate::services::agent_runtime::actions::validate_params;
use crate::services::mysql_pool_manager::MySQLPoolManager;

/// 工具返回前缀（agent 循环识别并转 SSE 事件）。
pub const ACTION_PENDING_PREFIX: &str = "ACTION_PENDING:";

/// 对话内动作申请概览（API/SSE 用）。
#[derive(Debug, Clone, Serialize)]
pub struct ChatActionView {
    pub id: i64,
    pub session_id: i64,
    pub kind: String,
    pub title: String,
    pub params: serde_json::Value,
    pub reason: Option<String>,
    pub status: String,
    pub action_uuid: String,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: String,
    pub confirmed_by: Option<String>,
    pub confirmed_at: Option<String>,
    pub executed_at: Option<String>,
    pub result_json: Option<String>,
}

#[stellar_macros::app_db]
fn row_to_chat_action<DB: AppDb>(r: &<DB as sqlx::Database>::Row) -> ChatActionView {
    ChatActionView {
        id: r.get("id"),
        session_id: r.get("session_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        params: serde_json::from_str(&r.get::<String, _>("params_json")).unwrap_or_default(),
        reason: r.get("reason"),
        status: r.get("status"),
        action_uuid: r.get("action_uuid"),
        created_by: r.get("created_by"),
        created_at: r.get("created_at"),
        expires_at: r.get("expires_at"),
        confirmed_by: r.get("confirmed_by"),
        confirmed_at: r.get("confirmed_at"),
        executed_at: r.get("executed_at"),
        result_json: r.get("result_json"),
    }
}

/// TTL：15 分钟（对齐动作闭环）。
pub const ACTION_TTL_MINUTES: i64 = 15;

pub struct ChatActionStore<DB: AppDb> {
    pool: sqlx::Pool<DB>,
}

#[app_impl]
impl<DB: AppDb> ChatActionStore<DB> {
    pub fn new(pool: sqlx::Pool<DB>) -> Self {
        Self { pool }
    }

    /// LLM 申请（propose_action 工具调用）：参数先校验，通过才落 pending。
    /// 返回 ACTION_PENDING 前缀文本 + 完整视图。
    pub async fn propose(
        &self,
        session_id: i64,
        user_id: i64,
        created_by: &str,
        kind: &str,
        params: &serde_json::Value,
        reason: Option<&str>,
    ) -> Result<(String, ChatActionView), String> {
        let title = validate_params(kind, params)?;
        let uuid = uuid::Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(ACTION_TTL_MINUTES);
        let id = db_query::query(
            "INSERT INTO agent_chat_actions \
                (session_id, user_id, kind, title, params_json, reason, status, action_uuid, \
                 created_by, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(kind)
        .bind(&title)
        .bind(params.to_string())
        .bind(reason)
        .bind(&uuid)
        .bind(created_by)
        .bind(expires_at.format("%Y-%m-%d %H:%M:%S").to_string())
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let view = self
            .get(id, session_id)
            .await?
            .ok_or("申请创建后读取失败")?;
        let payload = serde_json::json!({
            "id": view.id,
            "kind": view.kind,
            "title": view.title,
            "params": view.params,
            "reason": view.reason,
            "expires_at": view.expires_at,
        })
        .to_string();
        Ok((format!("{}{}", ACTION_PENDING_PREFIX, payload), view))
    }

    /// 按 id 取（归属校验由调用方用返回的 session_id 完成）。
    pub async fn get_by_id(&self, id: i64) -> Result<Option<ChatActionView>, String> {
        let row = db_query::query(
            "SELECT id, session_id, kind, title, params_json, reason, status, action_uuid, \
                    created_by, created_at, expires_at, confirmed_by, confirmed_at, executed_at, result_json \
             FROM agent_chat_actions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.as_ref().map(|r| row_to_chat_action::<DB>(r)))
    }

    pub async fn get(&self, id: i64, session_id: i64) -> Result<Option<ChatActionView>, String> {
        let row = db_query::query(
            "SELECT id, session_id, kind, title, params_json, reason, status, action_uuid, \
                    created_by, created_at, expires_at, confirmed_by, confirmed_at, executed_at, result_json \
             FROM agent_chat_actions WHERE id = ? AND session_id = ?",
        )
        .bind(id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.as_ref().map(|r| row_to_chat_action::<DB>(r)))
    }

    pub async fn list(&self, session_id: i64) -> Result<Vec<ChatActionView>, String> {
        let rows = db_query::query(
            "SELECT id, session_id, kind, title, params_json, reason, status, action_uuid, \
                    created_by, created_at, expires_at, confirmed_by, confirmed_at, executed_at, result_json \
             FROM agent_chat_actions WHERE session_id = ? ORDER BY id DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.iter().map(|r| row_to_chat_action::<DB>(r)).collect())
    }

    /// 用户确认 → 原子翻转（单次执行）→ MySQLClient 通道执行 → 结果留档。
    pub async fn confirm(
        &self,
        cluster: &Cluster,
        pool_mgr: &MySQLPoolManager,
        id: i64,
        session_id: i64,
        confirmed_by: &str,
    ) -> Result<ChatActionView, String> {
        let view = self
            .get(id, session_id)
            .await?
            .ok_or_else(|| format!("动作 {} 不存在", id))?;
        if view.status != "pending" {
            return Err(format!("动作已处于 {} 状态，不可重复确认（单次执行）", view.status));
        }
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if view.expires_at.as_str() < now.as_str() {
            return Err("动作已过期，不可执行".to_string());
        }
        let updated = db_query::query(
            "UPDATE agent_chat_actions SET status = 'executing', confirmed_at = ?, confirmed_by = ? \
             WHERE id = ? AND session_id = ? AND status = 'pending'",
        )
        .bind(&now)
        .bind(confirmed_by)
        .bind(id)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if updated.rows_affected() == 0 {
            return Err("动作已被其他会话处理".to_string());
        }

        let result = crate::services::agent_runtime::actions::execute_action(
            cluster,
            pool_mgr,
            &view.kind,
            &view.params,
        )
        .await;
        let (status, result_json) = match &result {
            Ok(text) => ("executed".to_string(), text.clone()),
            Err(e) => ("failed".to_string(), e.clone()),
        };
        // 执行结果回写会话消息（审计链 + 前端刷新即可见）
        {
            let emoji = if status == "executed" { "✅" } else { "❌" };
            let note = format!(
                "{} 动作「{}」执行结果：{}（确认人：{}，{}）",
                emoji, view.title, result_json, confirmed_by, status
            );
            let _ = crate::services::ai::AiSessionStore::new(self.pool.clone())
                .save_message(session_id, "assistant", &note, &[])
                .await;
        }
        let executed_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rows = db_query::query(
            "UPDATE agent_chat_actions SET status = ?, executed_at = ?, result_json = ? WHERE id = ?",
        )
        .bind(&status)
        .bind(&executed_at)
        .bind(&result_json)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if rows.rows_affected() == 0 {
            return Err("动作状态更新失败".to_string());
        }
        self.get(id, session_id)
            .await?
            .ok_or("读取执行结果失败".to_string())
    }

    /// 用户拒绝。
    pub async fn cancel(&self, id: i64, session_id: i64) -> Result<ChatActionView, String> {
        let view = self.get(id, session_id).await?.ok_or("动作不存在")?;
        if view.status != "pending" {
            return Err(format!("动作已处于 {} 状态，不可取消", view.status));
        }
        db_query::query(
            "UPDATE agent_chat_actions SET status = 'cancelled' \
             WHERE id = ? AND session_id = ? AND status = 'pending'",
        )
        .bind(id)
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        self.get(id, session_id)
            .await?
            .ok_or("读取失败".to_string())
    }
}
