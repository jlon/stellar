//! Context builder: static system prompt template + per-turn cluster snapshot.
//! The dynamic layer (`build_snapshot`) is refreshed at the start of every agent
//! turn so stale facts never leak across turns (Flink assistant design).

use serde_json::json;

use crate::models::cluster::Cluster;
use crate::services::metrics_collector_service::MetricsSnapshot;

use super::tool::{AgentTool, tool_specs};

/// System prompt template with `{{snapshot}}`, `{{tools}}`, `{{skills}}` placeholders.
const SYSTEM_TEMPLATE: &str = r#"你是 Stellar AI 运维助手，一名 StarRocks / Apache Doris OLAP 集群的资深运维专家。
你通过只读工具获取真实数据后回答用户问题。

## 当前集群快照（每轮新鲜采集，禁止假设过期值）
{{snapshot}}

## 可用工具
{{tools}}

## 诊断技能
{{skills}}

## 回答要求
1. 证据先行：先调用工具取数，再下结论；结论引用真实指标。
2. 无数据时明说 "暂无数据"，绝不编造。
3. 输出简洁的中文结论：现状 → 根因判断 → 可执行建议（命令/SQL/参数）。
4. **回答必须使用三段式模板**：
   ## 结论
   （一句话结论，直接回答提问）
   ## 依据
   （多指标证据必须用 Markdown 表格呈现：指标/当前值/判定三列；每个结论数字都能在依据中找到来源）
   ## 建议
   （可执行的建议：命令 / SQL / 参数调整；需要调参或终止查询时必须用 propose_action 提交申请走用户确认，禁止只给“可自行执行”的提示）
5. **调用纪律**：先用 query_metrics（1 次）定方向，再按技能深入；同一工具相同参数不重复调用；
   一轮诊断工具调用控制在 8 次以内，证据够了就收敛结论。
6. **证据不足时**：明确列出缺什么证据 + 下一步取证计划，禁止用猜测填补；
   Profile 拿不到就走 query_explain（执行计划）或基于扫描量下结论，不要反复重试同一个失败调用；
   同一工具失败 2 次就换路，禁止换参穷举。
"#;

/// Per-turn cluster snapshot (freshly collected).
pub struct ClusterSnapshot {
    pub text: String,
    pub collected_at_ms: i64,
}

/// Build the system message for a turn.
pub fn build_system_message(
    snapshot: &ClusterSnapshot,
    tools: &[Box<dyn AgentTool>],
) -> crate::services::ai::types::ChatMessage {
    build_system_message_with_context(snapshot, tools, None)
}

/// 带页面上下文的 system 构建：`page_context` 为前端上报的当前页面信息
/// （如：用户在「查询管理」页附带 query_id），让模型感知提问场景。
pub fn build_system_message_with_context(
    snapshot: &ClusterSnapshot,
    tools: &[Box<dyn AgentTool>],
    page_context: Option<&serde_json::Value>,
) -> crate::services::ai::types::ChatMessage {
    let tools_text =
        serde_json::to_string_pretty(&tool_specs(tools)).unwrap_or_else(|_| "[]".to_string());
    let mut prompt = SYSTEM_TEMPLATE
        .replace("{{snapshot}}", &snapshot.text)
        .replace("{{tools}}", &tools_text)
        .replace("{{skills}}", super::skills::SKILLS);
    if let Some(ctx) = page_context {
        let page = ctx.get("page").and_then(|v| v.as_str()).unwrap_or("");
        let params = ctx.get("params").cloned().unwrap_or_else(|| json!({}));
        prompt.push_str(&format!(
            "
## 用户当前页面上下文
用户正在「{}」页面提问，页面附带参数：{}。
             若问题与页面内容相关，优先围绕这些参数取证；无关则忽略。",
            page, params
        ));
    }
    crate::services::ai::types::ChatMessage {
        role: "system".to_string(),
        content: prompt,
        tool_calls: None,
        tool_call_id: None,
    }
}

/// Render a `MetricsSnapshot` (plus cluster metadata) into compact text.
pub fn render_snapshot(cluster: &Cluster, latest: Option<&MetricsSnapshot>) -> ClusterSnapshot {
    let head = format!(
        "cluster: name={}, type={:?} (deployment: {:?}), fe_host={}, active={}",
        cluster.name,
        cluster.cluster_type,
        cluster.deployment_mode,
        cluster.fe_host,
        cluster.is_active
    );
    let body = match latest {
        Some(m) => json!({
            "collected_at": m.collected_at.to_rfc3339(),
            "be": {"alive": m.backend_alive, "total": m.backend_total},
            "fe": {"alive": m.frontend_alive, "total": m.frontend_total},
            "query_perf": {
                "qps": m.qps, "p50_ms": m.query_latency_p50, "p95_ms": m.query_latency_p95,
                "p99_ms": m.query_latency_p99, "error": m.query_error, "timeout": m.query_timeout
            },
            "resource": {
                "cpu_pct": m.avg_cpu_usage, "mem_pct": m.avg_memory_usage,
                "disk_usage_pct": m.disk_usage_pct, "disk_used_bytes": m.disk_used_bytes,
                "disk_total_bytes": m.disk_total_bytes
            },
            "storage": {"tablets": m.tablet_count, "max_compaction_score": m.max_compaction_score},
            "txn": {"running": m.txn_running, "failed_total": m.txn_failed_total},
            "load": {"running": m.load_running, "finished_total": m.load_finished_total},
            "jvm": {"heap_usage_pct": m.jvm_heap_usage_pct, "threads": m.jvm_thread_count},
            "io": {"read_rate": m.io_read_rate, "write_rate": m.io_write_rate},
        })
        .to_string(),
        None => "no metrics snapshot collected yet".to_string(),
    };
    ClusterSnapshot {
        text: format!("{}\nmetrics: {}", head, body),
        collected_at_ms: chrono::Utc::now().timestamp_millis(),
    }
}
