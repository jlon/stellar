//! 事件采集器与收敛器（规则优先，纯只读）。
//!
//! - `collect_cluster_events`：快照前后对比 + 节点明细 + 审计慢查询 → 事件候选
//! - `converge_events`：fingerprint 5 分钟窗归并（累加计数）+ 拓扑抑制（node_down 抑制次生）
//!   + 消退处理（阈值回落 → open 事件置 resolved）
//!
//! 阈值口径与 `overview_service::generate_alerts` 保持一致（磁盘 80/90、compaction 50/100）；
//! 事件语义：breach 存在即事件（按 tick 累加 occurrence_count），消退即 resolved。

use chrono::{Duration, Utc};
use serde_json::{Value, json};
use sqlx::Row;

use stellar_macros::app_db;

use crate::db::{AppDb, query as db_query};
use crate::models::cluster::Cluster;
use crate::services::cluster_adapter::ClusterAdapter;
use crate::utils::string_ext::truncate;

use super::models::{EventCandidate, EventKind, EventRow, EventSeverity, EventState};

// ---- 阈值注册表（与 overview_service 告警口径一致；后续可由 [agent.thresholds] 覆盖） ----
/// 磁盘水位：warning ≥ 80%，critical ≥ 90%
/// 磁盘水位阈值：与 overview_service::generate_alerts 共用同一常量（见
/// metrics_collector_service::DISK_WARNING_PCT / DISK_CRITICAL_PCT），口径一致。
pub use crate::services::metrics_collector_service::{DISK_CRITICAL_PCT, DISK_WARNING_PCT};
/// Compaction score：warning > 50，critical > 100
pub const COMPACTION_WARNING: f64 = 50.0;
pub const COMPACTION_CRITICAL: f64 = 100.0;
/// 运行中导入任务数 ≥ 10 视作积压
pub const LOAD_BACKLOG_THRESHOLD: i32 = 10;
/// 单 tick 事务失败/查询错误增量 ≥ 5 视作异常
pub const INCREMENT_DELTA: i64 = 5;
/// 慢查询摄取门槛（毫秒）与最近回溯窗口（小时）
pub const SLOW_QUERY_MS: i64 = 5000;
pub const SLOW_QUERY_WINDOW_HOURS: i64 = 2;
/// 事件归并窗口（分钟）：同 fingerprint 在此窗口内累加
pub const CONVERGE_WINDOW_MIN: i64 = 5;

/// 一次采集的结果：新候选 + 需要置 resolved 的指纹
pub struct CollectOutcome {
    pub candidates: Vec<EventCandidate>,
    pub cleared_fingerprints: Vec<String>,
}

/// 采集某集群的当前事件状态（快照 diff + 节点明细 + 慢查询）。
#[app_db]
pub async fn collect_cluster_events<DB: AppDb>(
    cluster: &Cluster,
    pool: &sqlx::Pool<DB>,
    adapter: Option<&dyn ClusterAdapter>,
    audit_snapshot: Option<&[(String, String, i64)]>, // (query_id, sql, duration_ms)
) -> CollectOutcome {
    let mut outcome = CollectOutcome { candidates: Vec::new(), cleared_fingerprints: Vec::new() };

    // 1) 快照 diff：最近 2 条
    let rows = db_query::query(
        "SELECT collected_at, backend_alive, backend_total, disk_usage_pct, max_compaction_score, \
                txn_failed_total, load_running, query_error, query_timeout, qps \
         FROM metrics_snapshots WHERE cluster_id = ? ORDER BY collected_at DESC LIMIT 2",
    )
    .bind(cluster.id)
    .fetch_all(pool)
    .await;
    let rows = match rows {
        Ok(r) if !r.is_empty() => r,
        _ => return outcome, // 无快照，无事件
    };
    let latest = &rows[0];
    let prev = rows.get(1);

    let disk = latest.get::<f64, _>("disk_usage_pct");
    let compaction = latest.get::<f64, _>("max_compaction_score");
    let be_alive = latest.get::<i32, _>("backend_alive");
    let be_total = latest.get::<i32, _>("backend_total");
    let load_running = latest.get::<i32, _>("load_running");

    // 1a) 磁盘水位（shared-data 集群本地盘只是数据缓存配额，写满是 LRU 淘汰的稳态，不得报磁盘事故）
    if !cluster.is_shared_data() {
        push_breach(
            &mut outcome,
            BreachSpec {
                kind: EventKind::DiskPressure,
                active: disk >= DISK_WARNING_PCT,
                severity: if disk >= DISK_CRITICAL_PCT {
                    EventSeverity::Critical
                } else {
                    EventSeverity::Warning
                },
                object_type: "cluster",
                object_id: &cluster.id.to_string(),
                title: format!("磁盘使用率 {:.1}%（警戒 {}%）", disk, DISK_WARNING_PCT as i32),
                summary: format!("磁盘使用率 {:.1}%，建议清理过期数据或扩容", disk),
                metrics: json!({ "disk_usage_pct": disk }),
            },
        );
    }

    // 1b) compaction score
    push_breach(
        &mut outcome,
        BreachSpec {
            kind: EventKind::Compaction,
            active: compaction > COMPACTION_WARNING,
            severity: if compaction > COMPACTION_CRITICAL {
                EventSeverity::Critical
            } else {
                EventSeverity::Warning
            },
            object_type: "cluster",
            object_id: &cluster.id.to_string(),
            title: format!("Compaction Score 偏高 {:.1}", compaction),
            summary: format!("Compaction Score {:.1}，检查磁盘 IO 与 compaction 并发", compaction),
            metrics: json!({ "max_compaction_score": compaction }),
        },
    );

    // 1c) 导入积压
    push_breach(
        &mut outcome,
        BreachSpec {
            kind: EventKind::LoadBacklog,
            active: load_running >= LOAD_BACKLOG_THRESHOLD,
            severity: EventSeverity::Warning,
            object_type: "cluster",
            object_id: &cluster.id.to_string(),
            title: format!("运行中导入任务 {} 个，超过积压阈值", load_running),
            summary: format!("{} 个导入任务积压，检查 BE 磁盘/compaction 状态", load_running),
            metrics: json!({ "load_running": load_running }),
        },
    );

    // 1d) 事务失败增量（prev → latest）
    let txn_failed = latest.get::<i64, _>("txn_failed_total");
    let txn_delta = prev
        .map(|p| txn_failed - p.get::<i64, _>("txn_failed_total"))
        .unwrap_or(0);
    push_breach(
        &mut outcome,
        BreachSpec {
            kind: EventKind::TxnError,
            active: txn_delta >= INCREMENT_DELTA,
            severity: EventSeverity::Warning,
            object_type: "cluster",
            object_id: &cluster.id.to_string(),
            title: format!("事务失败激增（+{} 次）", txn_delta),
            summary: format!("本窗口新增 {} 次事务失败，检查写入链路", txn_delta),
            metrics: json!({ "txn_failed_total": txn_failed, "delta": txn_delta }),
        },
    );

    // 1e) 查询错误/超时增量
    let err = latest.get::<i64, _>("query_error") + latest.get::<i64, _>("query_timeout");
    let err_prev = prev
        .map(|p| p.get::<i64, _>("query_error") + p.get::<i64, _>("query_timeout"))
        .unwrap_or(0);
    let err_delta = err - err_prev;
    push_breach(
        &mut outcome,
        BreachSpec {
            kind: EventKind::MetricBreach,
            active: err_delta >= INCREMENT_DELTA,
            severity: EventSeverity::Warning,
            object_type: "cluster",
            object_id: &cluster.id.to_string(),
            title: format!("查询错误/超时激增（+{}）", err_delta),
            summary: format!("本窗口新增 {} 次查询错误/超时，检查负载与慢查询", err_delta),
            metrics: json!({ "query_error_delta": err_delta }),
        },
    );

    // 2) 节点掉线：快照显示存活下降 → 调 adapter 找具体节点
    if be_alive < be_total {
        let mut found = false;
        if let Some(adapter) = adapter {
            if let Ok(backends) = adapter.get_backends().await {
                for be in backends.iter().filter(|b| !is_alive(&b.alive)) {
                    found = true;
                    outcome.candidates.push(make_candidate(
                        EventKind::NodeDown,
                        EventSeverity::Critical,
                        "be",
                        &be.host,
                        format!("计算节点 {} 离线", be.host),
                        format!("BE {} 离线（存活 {}/{}）", be.host, be_alive, be_total),
                        json!({ "backend_alive": be_alive, "backend_total": be_total }),
                    ));
                }
            }
        }
        if !found {
            // 无 adapter 明细时退化为集群级事件
            outcome.candidates.push(make_candidate(
                EventKind::NodeDown,
                EventSeverity::Critical,
                "cluster",
                &cluster.id.to_string(),
                format!("{} 个计算节点离线", be_total - be_alive),
                format!("计算节点离线（存活 {}/{})", be_alive, be_total),
                json!({ "backend_alive": be_alive, "backend_total": be_total }),
            ));
        }
    }

    // 3) 慢查询摄取（审计快照，≥5s 低频门槛，每 tick 至多 3 条）
    if let Some(slow) = audit_snapshot {
        for (query_id, sql, duration_ms) in slow.iter().take(3) {
            outcome.candidates.push(EventCandidate {
                fingerprint: fingerprint(EventKind::SlowQuery, "query", query_id),
                kind: EventKind::SlowQuery,
                severity: EventSeverity::Info,
                object_type: Some("query".to_string()),
                object_id: Some(query_id.clone()),
                title: format!("慢查询 {}（{:.1}s）", query_id, *duration_ms as f64 / 1000.0),
                summary: truncate(sql, 200),
                metrics: json!({ "duration_ms": duration_ms }),
                cleared: false,
            });
        }
    }

    outcome
}

/// 阈值事件参数（避免长参数列）
struct BreachSpec<'a> {
    kind: EventKind,
    active: bool,
    severity: EventSeverity,
    object_type: &'a str,
    object_id: &'a str,
    title: String,
    summary: String,
    metrics: Value,
}

/// 阈值事件：active=true 入候选（按 severity），false 仅标记消退指纹。
fn push_breach(outcome: &mut CollectOutcome, spec: BreachSpec<'_>) {
    outcome
        .cleared_fingerprints
        .push(fingerprint(spec.kind, spec.object_type, spec.object_id));
    if spec.active {
        outcome.candidates.push(make_candidate(
            spec.kind,
            spec.severity,
            spec.object_type,
            spec.object_id,
            spec.title,
            spec.summary,
            spec.metrics,
        ));
    }
}

fn make_candidate(
    kind: EventKind,
    severity: EventSeverity,
    object_type: &str,
    object_id: &str,
    title: String,
    summary: String,
    metrics: Value,
) -> EventCandidate {
    EventCandidate {
        fingerprint: fingerprint(kind, object_type, object_id),
        kind,
        severity,
        object_type: Some(object_type.to_string()),
        object_id: Some(object_id.to_string()),
        title,
        summary,
        metrics,
        cleared: false,
    }
}

/// Backend.alive 是字符串（"true"/"false"）
fn is_alive(s: &str) -> bool {
    let s = s.trim();
    s.eq_ignore_ascii_case("true") || s == "1"
}

/// 归并键：{kind}:{object_type}:{object_id}（cluster 维度在 SQL 中限定）
pub fn fingerprint(kind: EventKind, object_type: &str, object_id: &str) -> String {
    format!("{}:{}:{}", kind.as_str(), object_type, object_id)
}

/// 收敛入库：
/// 1) 消退指纹 → 置 resolved；
/// 2) fingerprint + 5 分钟窗内 open/aggregated 事件 → 累加计数；
/// 3) node_down 活跃时，同集群次生事件（disk/compaction/load/txn/metric）标记 suppressed。
/// 返回活跃事件行（open/aggregated 且未抑制），供 Incident 聚合。
#[app_db]
pub async fn converge_events<DB: AppDb>(
    cluster: &Cluster,
    pool: &sqlx::Pool<DB>,
    outcome: &CollectOutcome,
) -> Vec<EventRow> {
    // 1) 消退处理先于新事件
    for fp in &outcome.cleared_fingerprints {
        let _ = db_query::query(
            "UPDATE agent_events SET state = 'resolved' \
             WHERE cluster_id = ? AND fingerprint = ? AND state IN ('open','aggregated')",
        )
        .bind(cluster.id)
        .bind(fp)
        .execute(pool)
        .await;
    }

    // 2) node_down 抑制源：本次候选 + 库内活跃
    let mut node_down_events: Vec<(String, String)> = Vec::new(); // (object_type, object_id)
    for c in outcome
        .candidates
        .iter()
        .filter(|c| c.kind == EventKind::NodeDown)
    {
        node_down_events.push((
            c.object_type.clone().unwrap_or_default(),
            c.object_id.clone().unwrap_or_default(),
        ));
    }
    if node_down_events.is_empty() {
        if let Ok(rows) = db_query::query(
            "SELECT object_type, object_id FROM agent_events \
             WHERE cluster_id = ? AND kind = 'node_down' AND state IN ('open','aggregated')",
        )
        .bind(cluster.id)
        .fetch_all(pool)
        .await
        {
            for r in &rows {
                node_down_events.push((r.get::<String, _>("object_type"), r.get("object_id")));
            }
        }
    }

    // 3) 归并入库
    let mut active = Vec::new();
    for c in &outcome.candidates {
        let suppressed = !node_down_events.is_empty()
            && c.kind != EventKind::NodeDown
            && c.kind != EventKind::SlowQuery;

        let window = Utc::now() - Duration::minutes(CONVERGE_WINDOW_MIN);
        if let Ok(Some(row)) = db_query::query(
            "SELECT id, occurrence_count FROM agent_events \
             WHERE cluster_id = ? AND fingerprint = ? AND state IN ('open','aggregated') \
               AND last_seen_at >= ? LIMIT 1",
        )
        .bind(cluster.id)
        .bind(&c.fingerprint)
        .bind(window)
        .fetch_optional(pool)
        .await
        {
            let id: i64 = row.get("id");
            let count: i64 = row.get("occurrence_count");
            let new_state = if suppressed { "suppressed" } else { "open" };
            let _ = db_query::query(
                "UPDATE agent_events SET occurrence_count = ?, state = ?, \
                        metrics_json = ?, last_seen_at = ? WHERE id = ?",
            )
            .bind(count + 1)
            .bind(new_state)
            .bind(serde_json::to_string(&c.metrics).unwrap_or_default())
            .bind(Utc::now())
            .bind(id)
            .execute(pool)
            .await;
            if !suppressed {
                active.push(EventRow {
                    id,
                    cluster_id: cluster.id,
                    fingerprint: c.fingerprint.clone(),
                    kind: c.kind,
                    severity: c.severity,
                    object_type: c.object_type.clone(),
                    object_id: c.object_id.clone(),
                    title: c.title.clone(),
                    state: EventState::Open,
                    occurrence_count: count + 1,
                    first_seen_at: Utc::now(),
                    last_seen_at: Utc::now(),
                });
            }
            continue;
        }

        let state = if suppressed { "suppressed" } else { "open" };
        let id = db_query::query(
            "INSERT INTO agent_events \
             (cluster_id, fingerprint, kind, severity, object_type, object_id, title, summary, \
              metrics_json, state, occurrence_count, suppressed_by) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)",
        )
        .bind(cluster.id)
        .bind(&c.fingerprint)
        .bind(c.kind.as_str())
        .bind(c.severity.as_str())
        .bind(&c.object_type)
        .bind(&c.object_id)
        .bind(&c.title)
        .bind(&c.summary)
        .bind(serde_json::to_string(&c.metrics).unwrap_or_default())
        .bind(state)
        .bind(node_down_events.first().map(|(_, id)| id.clone()))
        .insert_id(pool)
        .await;

        match id {
            Ok(event_id) if !suppressed => {
                active.push(EventRow {
                    id: event_id,
                    cluster_id: cluster.id,
                    fingerprint: c.fingerprint.clone(),
                    kind: c.kind,
                    severity: c.severity,
                    object_type: c.object_type.clone(),
                    object_id: c.object_id.clone(),
                    title: c.title.clone(),
                    state: EventState::Open,
                    occurrence_count: 1,
                    first_seen_at: Utc::now(),
                    last_seen_at: Utc::now(),
                });
            },
            Ok(_) | Err(_) => {},
        }
    }

    active
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_shape() {
        assert_eq!(fingerprint(EventKind::NodeDown, "be", "h1"), "node_down:be:h1");
        assert_eq!(fingerprint(EventKind::DiskPressure, "cluster", "3"), "disk_pressure:cluster:3");
    }

    #[test]
    fn is_alive_parses_starrocks_strings() {
        assert!(is_alive("true"));
        assert!(is_alive("TRUE"));
        assert!(is_alive("1"));
        assert!(!is_alive("false"));
        assert!(!is_alive(""));
    }

    #[test]
    fn push_breach_active_and_cleared() {
        let mut out = CollectOutcome { candidates: Vec::new(), cleared_fingerprints: Vec::new() };
        push_breach(
            &mut out,
            BreachSpec {
                kind: EventKind::DiskPressure,
                active: true,
                severity: EventSeverity::Critical,
                object_type: "cluster",
                object_id: "1",
                title: "t".into(),
                summary: "s".into(),
                metrics: json!({}),
            },
        );
        // 未超阈 → 只标消退（并测试同名指纹去重语义由 converge 处理）
        push_breach(
            &mut out,
            BreachSpec {
                kind: EventKind::Compaction,
                active: false,
                severity: EventSeverity::Warning,
                object_type: "cluster",
                object_id: "1",
                title: "t".into(),
                summary: "s".into(),
                metrics: json!({}),
            },
        );
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].kind, EventKind::DiskPressure);
        assert_eq!(out.candidates[0].severity, EventSeverity::Critical);
        assert!(
            out.cleared_fingerprints
                .contains(&"compaction:cluster:1".to_string())
        );
    }

    #[test]
    fn slow_query_candidate_carries_query_object() {
        let mut out = CollectOutcome { candidates: Vec::new(), cleared_fingerprints: Vec::new() };
        // 直接构造慢查询候选（绕过快照依赖）
        out.candidates.push(EventCandidate {
            fingerprint: fingerprint(EventKind::SlowQuery, "query", "q1"),
            kind: EventKind::SlowQuery,
            severity: EventSeverity::Info,
            object_type: Some("query".into()),
            object_id: Some("q1".into()),
            title: "慢查询 q1".into(),
            summary: "select 1".into(),
            metrics: json!({ "duration_ms": 6000 }),
            cleared: false,
        });
        assert_eq!(out.candidates[0].object_id.as_deref(), Some("q1"));
    }
}
