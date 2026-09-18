//! `propose_action` -- 受控写动作申请（对话内授权执行）：
//! LLM 只能通过本工具**提交申请**（不执行），用户确认后由后端走 MySQLClient 通道执行。
//! 返回 `ACTION_PENDING:{json}` 前缀文本，agent 循环截获并推 SSE 确认卡片。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::ops_agent::chat_actions::ChatActionStore;
use crate::services::ops_agent::tool::{AgentTool, ToolContext};

pub struct ProposeActionTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for ProposeActionTool<DB> {
    fn name(&self) -> &'static str {
        "propose_action"
    }

    fn description(&self) -> &'static str {
        "提交一个运维动作申请（不执行！）：当诊断发现需要调整集群参数（update_variable）或终止异常查询（kill_query）时，         用本工具提交申请并说明理由。用户确认后动作才会执行，执行结果会返回。         禁止绕过本工具直接建议执行（如给出可自行执行的提示），一律通过本工具走确认流程。         kind 取值: kill_query（query_id 为运行中查询的 UUID）、update_variable（key=变量名，value=新值，scope=global/session）。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["kill_query", "update_variable"], "description": "动作类型" },
                "params": {
                    "type": "object",
                    "description": "动作参数：kill_query 需 query_id；update_variable 需 key/value/scope"
                },
                "reason": { "type": "string", "description": "提出该动作的理由（基于哪些证据），将记录进审计" }
            },
            "required": ["kind", "params", "reason"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
        let reason = args
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let store = ChatActionStore::new(self.ctx.pool.clone());
        let (pending_text, _view) = store
            .propose(
                self.ctx.session_id,
                self.ctx.user_id,
                &self.ctx.username,
                &kind,
                &params,
                if reason.is_empty() { None } else { Some(&reason) },
            )
            .await
            .map_err(|e| format!("动作申请失败: {}", e))?;

        // 返回值必须是纯 `ACTION_PENDING:{json}`（agent 循环按前缀截获并解析，
        // 尾随文本会导致解析失败）。后续指引由 awaiting 行为完成。
        Ok(pending_text)
    }
}
