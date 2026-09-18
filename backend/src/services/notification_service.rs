//! 用户通知（右上角铃铛）：异步任务完成 / Incident 事件等触达。
//! 资源按 user_id 隔离：所有 API 只读写调用者自己的通知。

use serde_json::Value;
use sqlx::Row;

/// 新通知载荷（kind/severity 语义见 product-closure-plan.md 通知矩阵）。
#[derive(Clone)]
pub struct NewNotification {
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub link: Option<String>,
    pub severity: String,
    pub meta: Option<Value>,
}

impl NewNotification {
    pub fn new(kind: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            body: None,
            link: None,
            severity: "info".into(),
            meta: None,
        }
    }
    pub fn severity(mut self, s: impl Into<String>) -> Self {
        self.severity = s.into();
        self
    }
    pub fn body(mut self, b: impl Into<String>) -> Self {
        self.body = Some(b.into());
        self
    }
    pub fn link(mut self, l: impl Into<String>) -> Self {
        self.link = Some(l.into());
        self
    }
    pub fn meta(mut self, m: Value) -> Self {
        self.meta = Some(m);
        self
    }
}
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::db::dialect::RowsAffected;
use crate::db::query as db_query;

pub struct NotificationService<DB: AppDb> {
    pool: sqlx::Pool<DB>,
}

#[app_impl]
impl<DB: AppDb> NotificationService<DB> {
    pub fn new(pool: sqlx::Pool<DB>) -> Self {
        Self { pool }
    }

    /// 创建通知（返回新 id）。severity: info | warning | critical；meta 携带关联对象。
    pub async fn create(&self, user_id: i64, n: NewNotification) -> Result<i64, String> {
        let id = db_query::query(
            "INSERT INTO notifications (user_id, kind, title, body, link, severity, meta_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(n.kind)
        .bind(n.title)
        .bind(n.body)
        .bind(n.link)
        .bind(n.severity)
        .bind(n.meta.as_ref().map(|m| m.to_string()))
        .insert_id(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(id as i64)
    }

    /// 我的通知（unread_only 时只取未读），最新在前。
    pub async fn list(&self, user_id: i64, unread_only: bool, limit: i64) -> Result<Value, String> {
        let rows = db_query::query(
            "SELECT id, kind, title, body, link, severity, meta_json, read, created_at \
             FROM notifications WHERE user_id = ? AND (? = 0 OR read = 0) \
             ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(user_id)
        .bind(if unread_only { 1 } else { 0 })
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<i64, _>("id"),
                    "kind": r.get::<String, _>("kind"),
                    "title": r.get::<String, _>("title"),
                    "body": r.get::<Option<String>, _>("body"),
                    "link": r.get::<Option<String>, _>("link"),
                    "severity": r.get::<String, _>("severity"),
                    "meta": r.get::<Option<String>, _>("meta_json")
                        .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok()),
                    "read": r.get::<i64, _>("read") != 0,
                    "created_at": r.get::<String, _>("created_at"),
                })
            })
            .collect();

        let count =
            db_query::query("SELECT COUNT(*) FROM notifications WHERE user_id = ? AND read = 0")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        let unread_count = count.get::<i64, _>("COUNT(*)");
        Ok(serde_json::json!({ "items": items, "unread_count": unread_count }))
    }

    /// 标记已读。返回 false = 不存在或非本人。
    pub async fn mark_read(&self, user_id: i64, id: i64) -> Result<bool, String> {
        let result =
            db_query::query("UPDATE notifications SET read = 1 WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(result.rows_affected() > 0)
    }
}
