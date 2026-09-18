//! Agent Tool abstraction -- aligned with pi-from-scratch / Flink AI assistant tool shape:
//! `name + description + JSON Schema parameters + execute(args) => string`.

use async_trait::async_trait;
use serde_json::{Value, json};

/// Context shared by all tools (assembled once per chat endpoint call).
pub struct ToolContext<DB: crate::db::AppDb> {
    pub cluster: crate::models::cluster::Cluster,
    pub pool: sqlx::Pool<DB>,
    pub mysql_pool_manager: std::sync::Arc<crate::services::mysql_pool_manager::MySQLPoolManager>,
    pub audit_config: crate::config::AuditLogConfig,
    /// 当前会话/用户（propose_action 申请审计用）。
    pub session_id: i64,
    pub user_id: i64,
    pub username: String,
}

/// A read-only diagnostic tool callable by the agent loop.
#[async_trait]
pub trait AgentTool: Send + Sync {
    /// Tool name, e.g. `query_metrics`.
    fn name(&self) -> &'static str;
    /// Short description shown to the LLM.
    fn description(&self) -> &'static str;
    /// JSON Schema of the parameters object.
    fn parameters(&self) -> Value;
    /// Execute the tool; returns a text result (already truncated if needed).
    async fn execute(&self, args: Value) -> Result<String, String>;
}

/// 共享文本截断（见 utils::string_ext::truncate），经此处 re-export 保持既有调用面。
pub(crate) use crate::utils::string_ext::truncate;

/// Common parameter helper: object with an optional `limit` field.
pub(crate) fn limit_arg(args: &Value, default: usize, max: usize) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|v| (v as usize).clamp(1, max))
        .unwrap_or(default)
}

/// Build the tools list sent to the LLM (name + description + parameters).
pub(crate) fn tool_specs(tools: &[Box<dyn AgentTool>]) -> Value {
    json!(
        tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters(),
                }
            }))
            .collect::<Vec<_>>()
    )
}
