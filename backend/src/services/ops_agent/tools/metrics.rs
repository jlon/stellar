//! `query_metrics` -- recent metrics snapshots (trend, not just the latest point).

use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::Row;
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::{AppDb, query as db_query};
use crate::services::ops_agent::tool::{AgentTool, ToolContext, limit_arg, truncate};

pub struct QueryMetricsTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

#[app_impl]
impl<DB: AppDb> QueryMetricsTool<DB> {
    async fn fetch(&self, limit: usize) -> Result<String, String> {
        let (storage_kind, storage_key) = if self.ctx.cluster.is_shared_data() {
            ("data_cache", "data_cache_pct")
        } else {
            ("data_disk", "disk_pct")
        };
        let rows = db_query::query(
            "SELECT collected_at, qps, query_latency_p95, query_latency_p99, query_error, query_timeout, \
                    backend_alive, backend_total, frontend_alive, frontend_total, \
                    avg_cpu_usage, avg_memory_usage, disk_usage_pct, max_compaction_score, \
                    txn_running, txn_failed_total, load_running, jvm_heap_usage_pct, io_read_rate, io_write_rate \
             FROM metrics_snapshots WHERE cluster_id = ? ORDER BY collected_at DESC LIMIT ?",
        )
        .bind(self.ctx.cluster.id)
        .bind(limit as i64)
        .fetch_all(&self.ctx.pool)
        .await
        .map_err(|e| format!("查询指标失败: {}", e))?;

        let series: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "at": r.get::<chrono::DateTime<chrono::Utc>, _>("collected_at").to_rfc3339(),
                    "qps": r.get::<f64, _>("qps"),
                    "p95_ms": r.get::<f64, _>("query_latency_p95"),
                    "p99_ms": r.get::<f64, _>("query_latency_p99"),
                    "errors": r.get::<i64, _>("query_error"),
                    "timeouts": r.get::<i64, _>("query_timeout"),
                    "be": format!("{}/{}", r.get::<i32, _>("backend_alive"), r.get::<i32, _>("backend_total")),
                    "fe": format!("{}/{}", r.get::<i32, _>("frontend_alive"), r.get::<i32, _>("frontend_total")),
                    "cpu_pct": r.get::<f64, _>("avg_cpu_usage"),
                    "mem_pct": r.get::<f64, _>("avg_memory_usage"),
                    "storage_kind": storage_kind,
                    (storage_key): r.get::<f64, _>("disk_usage_pct"),
                    "compaction_score": r.get::<f64, _>("max_compaction_score"),
                    "txn_running": r.get::<i32, _>("txn_running"),
                    "txn_failed_total": r.get::<i64, _>("txn_failed_total"),
                    "load_running": r.get::<i32, _>("load_running"),
                    "jvm_heap_pct": r.get::<f64, _>("jvm_heap_usage_pct"),
                    "io_read_rate": r.get::<f64, _>("io_read_rate"),
                    "io_write_rate": r.get::<f64, _>("io_write_rate"),
                })
            })
            .collect();

        if series.is_empty() {
            return Ok("暂无指标快照（采集可能未启用或尚未完成首轮采集）".to_string());
        }
        Ok(truncate(&serde_json::to_string(&series).unwrap_or_default(), 6000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryMetricsTool<DB> {
    fn name(&self) -> &'static str {
        "query_metrics"
    }

    fn description(&self) -> &'static str {
        "查询集群最近的指标快照序列（按时间倒序）。包含 QPS、延迟 p95/p99、错误/超时、BE/FE 存活数、\
         CPU/内存/存储使用率（存算一体为数据盘，存算分离为数据缓存）、compaction score、事务与导入、JVM 堆、磁盘 IO 速率。\
         用于判断负载趋势与异常时间点。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "返回最近多少个采集点（默认 10，最大 50）" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        self.fetch(limit_arg(&args, 10, 50)).await
    }
}
