//! `query_slow_queries` -- slow query list from the audit log.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::audit_log_service::AuditLogService;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, limit_arg, truncate};

pub struct QuerySlowQueriesTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QuerySlowQueriesTool<DB> {
    async fn fetch(
        &self,
        hours: i64,
        min_duration_ms: i64,
        limit: usize,
    ) -> Result<String, String> {
        let service = AuditLogService::new(
            Arc::clone(&self.ctx.mysql_pool_manager),
            self.ctx.audit_config.clone(),
        );
        let rows = service
            .get_slow_queries(&self.ctx.cluster, hours as i32, min_duration_ms, limit)
            .await
            .map_err(|e| format!("查询审计慢查询失败: {}", e))?;
        if rows.is_empty() {
            return Ok(format!(
                "最近 {} 小时内没有超过 {}ms 的慢查询（或审计日志未配置）",
                hours, min_duration_ms
            ));
        }
        Ok(truncate(&serde_json::to_string(&rows).unwrap_or_default(), 6000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QuerySlowQueriesTool<DB> {
    fn name(&self) -> &'static str {
        "query_slow_queries"
    }

    fn description(&self) -> &'static str {
        "从审计日志查询慢查询清单（按耗时倒序，仅已完成查询）。含 query_id、用户、库、耗时、扫描行数/字节、\
         返回行数、CPU/内存开销、SQL 预览（前 200 字）。是性能问题的第一手证据；已完成查询的 query_id 可交给 \
         query_profile_diagnostics 做规则诊断，或用 query_explain 做执行计划分析。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hours": { "type": "integer", "description": "回溯小时数（默认 1，最大 24）" },
                "min_duration_ms": { "type": "integer", "description": "慢查询阈值毫秒（默认 1000）" },
                "limit": { "type": "integer", "description": "最多返回条数（默认 10，最大 50）" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let hours = args
            .get("hours")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .clamp(1, 24);
        let min_ms = args
            .get("min_duration_ms")
            .and_then(Value::as_i64)
            .unwrap_or(1000)
            .max(1);
        self.fetch(hours, min_ms, limit_arg(&args, 10, 50)).await
    }
}
