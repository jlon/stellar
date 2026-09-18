//! `query_profile_diagnostics` -- fetch a query profile and run the existing
//! rule engine + root cause analyzer (deterministic, no LLM involved).

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::create_adapter;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, truncate};
use crate::services::profile_analyzer::analyze_profile;

pub struct QueryProfileDiagnosticsTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QueryProfileDiagnosticsTool<DB> {
    async fn fetch(&self, query_id: &str) -> Result<String, String> {
        if query_id.is_empty() || !query_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            return Err("query_id 格式非法".to_string());
        }
        let adapter =
            create_adapter(self.ctx.cluster.clone(), Arc::clone(&self.ctx.mysql_pool_manager));
        let profile_text = adapter.get_profile(query_id).await.map_err(|e| {
            format!(
                "获取 Profile 失败: {}。下一步：该查询可能仍在运行（运行中无 Profile）、\
                 Profile 未开启（用 query_variables 查 enable_profile，或申请 propose_action 打开）、\
                 或 Profile 已过期；改用 query_explain 做执行计划分析，或基于扫描量直接给结论。",
                e
            )
        })?;

        let analysis =
            analyze_profile(&profile_text).map_err(|e| format!("Profile 规则分析失败: {}", e))?;

        let root_cause = analysis
            .root_cause_analysis
            .as_ref()
            .map(|r| r.summary.clone())
            .unwrap_or_default();
        let diagnostics: Vec<Value> = analysis
            .diagnostics
            .iter()
            .take(10)
            .map(|d| {
                json!({
                    "rule_id": d.rule_id,
                    "severity": d.severity,
                    "node_path": d.node_path,
                    "message": d.message,
                    "reason": d.reason,
                    "suggestions": d.suggestions,
                })
            })
            .collect();

        let out = json!({
            "query_id": query_id,
            "performance_score": analysis.performance_score,
            "conclusion": analysis.conclusion,
            "suggestions": analysis.suggestions.iter().take(8).collect::<Vec<_>>(),
            "diagnostics": diagnostics,
            "root_cause": root_cause,
        });
        Ok(truncate(&out.to_string(), 6000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryProfileDiagnosticsTool<DB> {
    fn name(&self) -> &'static str {
        "query_profile_diagnostics"
    }

    fn description(&self) -> &'static str {
        "对指定 query_id 拉取 Query Profile 并运行内置规则引擎诊断（纯规则、无需外部模型）。\
         只支持已完成的查询（query_id 来自 query_slow_queries）；运行中的查询没有 Profile。\
         返回性能评分、结论、诊断规则命中（含原因与建议）与根因分析。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query_id": { "type": "string", "description": "目标查询的 query_id（十六进制字符串）" }
            },
            "required": ["query_id"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let query_id = args
            .get("query_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.fetch(query_id).await
    }
}
