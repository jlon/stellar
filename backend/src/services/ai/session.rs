//! Server-side AI chat sessions persisted in the AppDb (`ai_sessions` /
//! `ai_messages`), shared by the ops agent and the ask-data feature
//! (channel-scoped). Sessions survive page reloads and keep the full
//! tool-call trace for agent turns.

use chrono::Utc;
use serde::Serialize;
use sqlx::Row;
use stellar_macros::app_impl;

use crate::db::dialect::RowsAffected;
use crate::db::{AppDb, query as db_query};

use super::types::{AgentStep, ChatMessage};

/// Session metadata returned to the frontend.
#[derive(Debug, Serialize, Clone)]
pub struct SessionInfo {
    pub id: i64,
    pub channel: String,
    pub cluster_id: i64,
    pub title: String,
    pub created_at: String,
    pub last_active_at: String,
}

/// One persisted turn (user message or assistant answer with its trace).
#[derive(Debug, Serialize, Clone)]
pub struct MessageRecord {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub steps: Vec<AgentStep>,
    /// 当前用户的评价（up/down），加载转录时按需填充。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
}

pub struct AiSessionStore<DB: AppDb> {
    pool: sqlx::Pool<DB>,
}

#[app_impl]
impl<DB: AppDb> AiSessionStore<DB> {
    pub fn new(pool: sqlx::Pool<DB>) -> Self {
        Self { pool }
    }

    /// Create a session (title from the first user message excerpt).
    pub async fn create_session(
        &self,
        channel: &str,
        cluster_id: i64,
        organization_id: Option<i64>,
        user_id: Option<i64>,
        first_message: &str,
    ) -> Result<i64, String> {
        let title: String = first_message.chars().take(60).collect();
        db_query::query(
            "INSERT INTO ai_sessions (channel, cluster_id, organization_id, user_id, title) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(channel)
        .bind(cluster_id)
        .bind(organization_id)
        .bind(user_id)
        .bind(&title)
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())
    }

    /// Load one session's metadata (ownership/authorization checks).
    pub async fn get_session(
        &self,
        session_id: i64,
        user_id: Option<i64>,
    ) -> Result<Option<SessionInfo>, String> {
        let row = db_query::query(
            "SELECT id, channel, cluster_id, title, created_at, last_active_at \
             FROM ai_sessions WHERE id = ? AND (? IS NULL OR user_id = ?)",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.as_ref().map(|r| SessionInfo {
            id: r.get("id"),
            channel: r.get("channel"),
            cluster_id: r.get("cluster_id"),
            title: r.get("title"),
            created_at: r.get("created_at"),
            last_active_at: r.get("last_active_at"),
        }))
    }

    /// Delete sessions of a channel whose last activity is older than
    /// `cutoff_days` (bounded history; Flink default session.ttl=7d).
    pub async fn delete_stale(&self, channel: &str, cutoff_days: i64) -> Result<i64, String> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(cutoff_days);
        let rows =
            db_query::query("DELETE FROM ai_sessions WHERE channel = ? AND last_active_at < ?")
                .bind(channel)
                .bind(cutoff.format("%Y-%m-%d %H:%M:%S").to_string())
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(rows.rows_affected() as i64)
    }

    /// List sessions of a channel+cluster, newest first.
    pub async fn list_sessions(
        &self,
        channel: &str,
        cluster_id: i64,
        user_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SessionInfo>, String> {
        let rows = db_query::query(
            "SELECT id, channel, cluster_id, title, created_at, last_active_at \
             FROM ai_sessions WHERE channel = ? AND cluster_id = ? \
             AND (? IS NULL OR user_id = ?) \
             ORDER BY last_active_at DESC LIMIT ?",
        )
        .bind(channel)
        .bind(cluster_id)
        .bind(user_id)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .iter()
            .map(|r| SessionInfo {
                id: r.get("id"),
                channel: r.get("channel"),
                cluster_id: r.get("cluster_id"),
                title: r.get("title"),
                created_at: r.get::<chrono::DateTime<Utc>, _>("created_at").to_rfc3339(),
                last_active_at: r
                    .get::<chrono::DateTime<Utc>, _>("last_active_at")
                    .to_rfc3339(),
            })
            .collect())
    }

    /// Load the full message history of a session, oldest first.
    pub async fn load_session(&self, session_id: i64) -> Result<Vec<MessageRecord>, String> {
        let rows = db_query::query(
            "SELECT id, role, content, steps_json FROM ai_messages \
             WHERE session_id = ? ORDER BY id",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let steps: Vec<AgentStep> = r
                .get::<Option<String>, _>("steps_json")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            out.push(MessageRecord {
                id: r.get("id"),
                role: r.get("role"),
                content: r.get("content"),
                steps,
                feedback: None,
            });
        }
        Ok(out)
    }

    /// Load the most recent `n` messages of a session, oldest first.
    pub async fn load_recent_messages(
        &self,
        session_id: i64,
        n: usize,
    ) -> Result<Vec<MessageRecord>, String> {
        let rows = db_query::query(
            "SELECT id, role, content, steps_json FROM (
                SELECT id, role, content, steps_json FROM ai_messages
                WHERE session_id = ? ORDER BY id DESC LIMIT ?
             ) t ORDER BY id ASC",
        )
        .bind(session_id)
        .bind(n as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let steps: Vec<AgentStep> = r
                .get::<Option<String>, _>("steps_json")
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            out.push(MessageRecord {
                id: r.get("id"),
                role: r.get("role"),
                content: r.get("content"),
                steps,
                feedback: None,
            });
        }
        Ok(out)
    }

    /// 会话重命名（归属校验由调用方先做）。
    pub async fn rename_session(&self, session_id: i64, title: &str) -> Result<(), String> {
        db_query::query("UPDATE ai_sessions SET title = ? WHERE id = ?")
            .bind(title)
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 消息归属：(session_id, channel, owner_user_id)，评价接口做越权校验用。
    pub async fn get_message_context(
        &self,
        message_id: i64,
    ) -> Result<Option<(i64, String, Option<i64>)>, String> {
        let row = db_query::query(
            "SELECT m.session_id AS sid, s.channel AS ch, s.user_id AS owner FROM ai_messages m \
             JOIN ai_sessions s ON s.id = m.session_id WHERE m.id = ?",
        )
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|r| (r.get("sid"), r.get("ch"), r.get("owner"))))
    }

    /// 设置/清除一条消息的评价（rating 为 None 时清除）。先 UPDATE，零行再 INSERT。
    pub async fn set_feedback(
        &self,
        message_id: i64,
        session_id: i64,
        user_id: i64,
        rating: Option<&str>,
    ) -> Result<(), String> {
        if rating.is_none() {
            db_query::query(
                "DELETE FROM agent_message_feedback WHERE message_id = ? AND user_id = ?",
            )
            .bind(message_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
        let updated = db_query::query(
            "UPDATE agent_message_feedback SET rating = ? \
             WHERE message_id = ? AND user_id = ?",
        )
        .bind(rating)
        .bind(message_id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        if updated.rows_affected() == 0 {
            db_query::query(
                "INSERT INTO agent_message_feedback (message_id, session_id, user_id, rating) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(message_id)
            .bind(session_id)
            .bind(user_id)
            .bind(rating)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 批量取某用户对一批消息的评价（转录回显点赞状态用）。
    pub async fn feedback_map(
        &self,
        message_ids: &[i64],
        user_id: i64,
    ) -> Result<std::collections::HashMap<i64, String>, String> {
        let mut out = std::collections::HashMap::new();
        if message_ids.is_empty() {
            return Ok(out);
        }
        // 方言无关：逐 id 查询（转录消息数小，无需动态 IN）。
        for id in message_ids {
            let row = db_query::query(
                "SELECT rating FROM agent_message_feedback WHERE message_id = ? AND user_id = ?",
            )
            .bind(*id)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            if let Some(r) = row {
                out.insert(*id, r.get("rating"));
            }
        }
        Ok(out)
    }

    /// Persist one turn (role + content + trace steps). Returns the message id.
    pub async fn save_message(
        &self,
        session_id: i64,
        role: &str,
        content: &str,
        steps: &[AgentStep],
    ) -> Result<i64, String> {
        let steps_json = if steps.is_empty() {
            None
        } else {
            Some(serde_json::to_string(steps).map_err(|e| e.to_string())?)
        };
        let row = db_query::query(
            "INSERT INTO ai_messages (session_id, role, content, steps_json) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(steps_json)
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        db_query::query("UPDATE ai_sessions SET last_active_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row)
    }

    pub async fn delete_session(&self, session_id: i64) -> Result<(), String> {
        db_query::query("DELETE FROM ai_sessions WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Map persisted messages to the LLM conversation shape (system excluded).
    pub fn to_chat_messages(records: &[MessageRecord]) -> Vec<ChatMessage> {
        let mut out = Vec::with_capacity(records.len());
        for m in records {
            // Tool results are persisted inside the assistant turn steps; the LLM
            // conversation is rebuilt per turn from the final answers only.
            if m.role == "user" || m.role == "assistant" {
                out.push(ChatMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }
        out
    }
}
