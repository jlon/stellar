//! `query_explain` -- 执行计划分析（无 Profile 时的确定性 fallback）。
//!
//! Profile 只对"已完成且开启 profile"的查询可用；运行中查询、profile 已过期、
//! `enable_profile=false` 的场景下，用只读 `EXPLAIN` 看执行计划是唯一的确定性手段。
//! SQL 只接受 SELECT/WITH 开头、单条语句，`EXPLAIN` 本身不执行数据查询。

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::db::AppDb;
use crate::services::audit_log_service::AuditLogService;
use crate::services::cluster_timeout;
use crate::services::mysql_client::MySQLClient;
use crate::services::ops_agent::tool::{AgentTool, ToolContext, truncate};

pub struct QueryExplainTool<DB: AppDb> {
    pub(crate) ctx: Arc<ToolContext<DB>>,
}

/// 只读语句守卫：SELECT/WITH/VALUES/TABLE 开头，单条（禁分号防堆叠）。
fn sanitize_readonly_sql(sql: &str) -> Result<String, String> {
    let t = sql.trim();
    if t.is_empty() {
        return Err("SQL 为空".to_string());
    }
    if t.len() > 20000 {
        return Err("SQL 过长（>20000 字符），请缩小范围".to_string());
    }
    let upper = t.to_ascii_uppercase();
    let ok = ["SELECT", "WITH", "VALUES", "TABLE"].iter().any(|k| {
        upper.starts_with(k)
            && upper[k.len()..].starts_with(|c: char| c.is_whitespace() || c == '(')
    });
    if !ok {
        return Err("只支持 SELECT/WITH 开头的只读查询做执行计划分析".to_string());
    }
    if t.contains(';') {
        return Err("只支持单条语句（不含分号）".to_string());
    }
    Ok(t.to_string())
}

#[app_impl]
impl<DB: AppDb> QueryExplainTool<DB> {
    async fn fetch(&self, query_id: &str, sql: &str) -> Result<String, String> {
        // 1. 确定待分析 SQL 与所属库：直接给的优先（库未知），否则按 query_id 去审计日志取全文+库
        //    （慢查询清单里的 query_preview 只有 200 字，做不了计划分析）。
        let (stmt, db) = if !sql.trim().is_empty() {
            (sanitize_readonly_sql(sql)?, String::new())
        } else if !query_id.trim().is_empty() {
            let service = AuditLogService::new(
                Arc::clone(&self.ctx.mysql_pool_manager),
                self.ctx.audit_config.clone(),
            );
            let (full, db) = service
                .get_query_sql(&self.ctx.cluster, query_id.trim())
                .await
                .map_err(|e| e.to_string())?;
            let stmt = sanitize_readonly_sql(&full).map_err(|e| {
                format!("审计日志中的 SQL 不可做计划分析（{}），请让用户直接给出 SQL", e)
            })?;
            (stmt, db)
        } else {
            return Err("query_id 和 sql 至少提供一个".to_string());
        };
        // 库名白名单（防 USE 注入；含反引号直接拒绝）。
        if !db.is_empty()
            && (db.contains('`') || !db.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        {
            return Err(format!("数据库名非法，无法切换上下文: {}", db));
        }
        // 2. 同一会话内 USE 库 + 只读 EXPLAIN（不执行数据查询）。
        //    会话式连接不受 with_timeout 约束，这里整体加一层与集群超时对齐的上限。
        let timeout = cluster_timeout(&self.ctx.cluster);
        let pool = self
            .ctx
            .mysql_pool_manager
            .get_pool(&self.ctx.cluster)
            .await
            .map_err(|e| format!("连接集群失败: {}", e))?;
        let client = MySQLClient::from_pool(pool);
        let explain_sql = format!("EXPLAIN {}", stmt);
        let flow = async {
            let mut session = client.create_session().await?;
            if !db.is_empty() {
                session.use_database(&db).await.map_err(|e| {
                    crate::utils::ApiError::invalid_data(format!(
                        "切换到查询原属库 `{}` 失败（{}）：该库可能已被删除，\
                         EXPLAIN 无法在正确上下文执行",
                        db, e
                    ))
                })?;
            }
            session.execute(&explain_sql).await
        };
        let (columns, rows, _) = tokio::time::timeout(timeout, flow)
            .await
            .map_err(|_| {
                format!("EXPLAIN 执行超时（{}s）：FE 可能过载，请稍后重试", timeout.as_secs())
            })?
            .map_err(|e| format!("EXPLAIN 执行失败: {}", e))?;
        let plan: Vec<Vec<String>> = rows.into_iter().take(120).collect();
        let out = json!({
            "database": db,
            "columns": columns,
            "plan_rows": plan,
            "hint": "重点看 join 顺序与方式、扫描行数估计、是否用到物化视图/分区裁剪",
        });
        Ok(truncate(&out.to_string(), 6000))
    }
}

#[app_impl]
#[async_trait]
impl<DB: AppDb> AgentTool for QueryExplainTool<DB> {
    fn name(&self) -> &'static str {
        "query_explain"
    }

    fn description(&self) -> &'static str {
        "对一条 SQL 做只读 EXPLAIN 执行计划分析。query_id（从 query_slow_queries 或 \
         query_running_queries 拿）与 sql 二选一：有 query_id 时自动去审计日志取全文。\
         这是拿不到 Profile 时的确定性 fallback（运行中查询、profile 未开启或已过期都走这里）。"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query_id": { "type": "string", "description": "审计/运行中的 query_id（自动取全文，可选）" },
                "sql": { "type": "string", "description": "完整 SQL（只读 SELECT/WITH，可选）" }
            }
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let query_id = args
            .get("query_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sql = args.get("sql").and_then(Value::as_str).unwrap_or_default();
        self.fetch(query_id, sql).await
    }
}
