//! `query_variables` -- key cluster variables (compaction / concurrency / memory knobs).

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::MySQLClient;
use crate::services::cluster_timeout;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, truncate};

/// Default variables the agent cares about when no explicit key list is given.
const DEFAULT_KEYS: &[&str] = &[
    "max_compaction_concurrency",
    "cumulative_compaction_num_threads_per_disk",
    "base_compaction_num_threads_per_disk",
    "max_compaction_score",
    "query_timeout",
    "exec_mem_limit",
    "max_parallel_scan_instance_num",
    "enable_profile",
    "parallel_fragment_exec_instance_num",
    "enable_pipeline_engine",
];

pub struct QueryVariablesTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QueryVariablesTool<DB> {
    async fn fetch(&self, keys: Vec<String>) -> Result<String, String> {
        let pool = self
            .ctx
            .mysql_pool_manager
            .get_pool(&self.ctx.cluster)
            .await
            .map_err(|e| format!("连接集群失败: {}", e))?;
        let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&self.ctx.cluster));
        let (_, rows) = client
            .query_raw("SHOW GLOBAL VARIABLES")
            .await
            .map_err(|e| format!("查询变量失败: {}", e))?;

        let mut out = serde_json::Map::new();
        for row in &rows {
            let name = row.first().cloned().unwrap_or_default();
            let value = row.get(1).cloned().unwrap_or_default();
            if keys.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
                out.insert(name, json!(value));
            }
        }
        if out.is_empty() {
            return Ok("未找到目标变量（可能是变量名不适用于该集群版本）".to_string());
        }
        Ok(truncate(&Value::Object(out).to_string(), 4000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryVariablesTool<DB> {
    fn name(&self) -> &'static str {
        "query_variables"
    }

    fn description(&self) -> &'static str {
        "查询集群全局变量（如 compaction 并发、query_timeout、exec_mem_limit 等关键调优参数）。\
         不传 keys 时返回一组运维关注的默认变量。用于在给出参数调优建议前确认真实当前值。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keys": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "变量名列表（可选）。变量名仅允许字母、数字与下划线。"
                }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let keys: Vec<String> = match args.get("keys").and_then(Value::as_array) {
            Some(list) => list
                .iter()
                .filter_map(Value::as_str)
                .filter(|k| {
                    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                })
                .map(str::to_string)
                .collect(),
            None => DEFAULT_KEYS.iter().map(|s| s.to_string()).collect(),
        };
        self.fetch(keys).await
    }
}
