//! `propose_action` -- 受控写动作申请（对话内授权执行）：
//! LLM 只能通过本工具**提交申请**（不执行），用户确认后由后端走 MySQLClient 通道执行。
//! 返回 `ACTION_PENDING:{json}` 前缀文本，agent 循环截获并推 SSE 确认卡片。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::models::CreateMaterializedViewRequest;
use crate::services::create_adapter;
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
        "提交一个运维动作申请（不执行！）：当诊断发现需要调整集群参数（update_variable）、终止异常查询（kill_query）或创建异步物化视图（create_materialized_view）时，用本工具提交申请并说明理由。用户确认后动作才会执行，执行结果会返回。禁止绕过本工具直接建议执行，一律走确认流程。create_materialized_view 的 params 只能提供结构化自定义 SELECT 草案（database、name、query_sql、可选 partition_by/distribution/build_immediate/sort_columns/replication_num、schedule），不能传 cluster_id 或 DDL；服务端会在申请时校验并生成待审阅 DDL。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["kill_query", "update_variable", "create_materialized_view"], "description": "动作类型" },
                "params": {
                    "type": "object",
                    "description": "动作参数：kill_query 需 query_id；update_variable 需 key/value/scope；create_materialized_view 需 database/name/query_sql/schedule，不得传 cluster_id 或 DDL"
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

        let params = if kind == "create_materialized_view" {
            self.prepare_materialized_view_action(params).await?
        } else {
            params
        };
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

#[app_impl]
impl<DB: AppDb> ProposeActionTool<DB> {
    async fn prepare_materialized_view_action(&self, params: Value) -> Result<Value, String> {
        let mut draft = params
            .as_object()
            .cloned()
            .ok_or("物化视图草案必须是对象")?;
        if draft.contains_key("cluster_id") || draft.contains_key("confirmed_ddl") {
            return Err("物化视图草案不能指定集群或 DDL；由服务端生成并绑定当前集群".to_string());
        }
        draft.insert("cluster_id".to_string(), json!(self.ctx.cluster.id));
        let request: CreateMaterializedViewRequest =
            serde_json::from_value(Value::Object(draft)).map_err(|_| "物化视图草案字段不合法")?;
        request.validate().map_err(|error| error.to_string())?;
        if request.query_sql.is_none() {
            return Err("智能运维只能提交自定义 SELECT 物化视图草案".to_string());
        }
        let adapter = create_adapter(
            self.ctx.cluster.clone(),
            std::sync::Arc::clone(&self.ctx.mysql_pool_manager),
        );
        let ddl = adapter
            .preview_materialized_view(&request)
            .await
            .map_err(|error| format!("物化视图草案无法预览: {error}"))?;
        Ok(json!({ "request": request, "confirmed_ddl": ddl }))
    }
}
