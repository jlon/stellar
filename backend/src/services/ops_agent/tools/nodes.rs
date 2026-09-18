//! `query_nodes` -- FE/BE node status via the cluster adapter.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::create_adapter;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, truncate};

pub struct QueryNodesTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QueryNodesTool<DB> {
    async fn fetch(&self, kind: &str) -> Result<String, String> {
        let adapter =
            create_adapter(self.ctx.cluster.clone(), Arc::clone(&self.ctx.mysql_pool_manager));
        match kind {
            "fe" => {
                let nodes = adapter
                    .get_frontends()
                    .await
                    .map_err(|e| format!("获取 FE 节点失败: {}", e))?;
                Ok(truncate(&serde_json::to_string(&nodes).unwrap_or_default(), 6000))
            },
            _ => {
                let nodes = adapter
                    .get_backends()
                    .await
                    .map_err(|e| format!("获取 BE 节点失败: {}", e))?;
                Ok(truncate(&serde_json::to_string(&nodes).unwrap_or_default(), 6000))
            },
        }
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryNodesTool<DB> {
    fn name(&self) -> &'static str {
        "query_nodes"
    }

    fn description(&self) -> &'static str {
        "查询集群节点的实时状态。kind=be 返回所有 BE（Backend）节点的存活状态、地址、\
         磁盘与 tablet 信息；kind=fe 返回 FE（Frontend）节点。用于定位掉线/异常节点。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["be", "fe"], "description": "节点类型，默认 be" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let kind = args.get("kind").and_then(Value::as_str).unwrap_or("be");
        self.fetch(kind).await
    }
}
