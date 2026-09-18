//! `query_running_queries` -- currently executing queries (resource competition check).

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::create_adapter;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, limit_arg, truncate};

pub struct QueryRunningQueriesTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QueryRunningQueriesTool<DB> {
    async fn fetch(&self, limit: usize) -> Result<String, String> {
        let adapter =
            create_adapter(self.ctx.cluster.clone(), Arc::clone(&self.ctx.mysql_pool_manager));
        let mut queries = adapter
            .get_queries()
            .await
            .map_err(|e| format!("获取运行中查询失败: {}", e))?;
        queries.truncate(limit);
        if queries.is_empty() {
            return Ok("当前没有运行中的查询".to_string());
        }
        Ok(truncate(&serde_json::to_string(&queries).unwrap_or_default(), 6000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryRunningQueriesTool<DB> {
    fn name(&self) -> &'static str {
        "query_running_queries"
    }

    fn description(&self) -> &'static str {
        "查询当前正在运行的 SQL 查询（含 query_id、耗时、扫描量、状态）。\
         用于判断资源竞争、找出可疑的大查询。注意：运行中的查询没有 Profile，\
         它的 query_id 只能用于 kill_query（走 propose_action 申请）或观察，\
         不能交给 query_profile_diagnostics；要做计划分析请用 query_explain。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "最多返回多少条（默认 20，最大 50）" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        self.fetch(limit_arg(&args, 20, 50)).await
    }
}
