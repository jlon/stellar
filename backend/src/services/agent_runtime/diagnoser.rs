//! 规则诊断器：事件症状模式 → 确定性根因剧本（不调用 LLM，置信度 ≥0.8 直接产出）。
//! M0 产出写入 `agent_decisions`（stage=rule_diagnosis）；LLM 假设/建议留第四阶段。

use serde_json::{Value, json};

use super::models::{DiagnosisAction, DiagnosisOutcome, EventKind, EventRow};

/// 依据 Incident 关联的活跃事件集合产出确定性诊断。
pub fn diagnose(events: &[EventRow]) -> DiagnosisOutcome {
    let has = |k: EventKind| events.iter().any(|e| e.kind == k);
    let critical = events.iter().any(|e| e.severity.as_str() == "critical");

    // 剧本：症状模式 → 根因（按优先级：节点故障 > 容量 > compaction > 导入 > 事务 > 性能）
    let mut outcome = if has(EventKind::NodeDown) {
        DiagnosisOutcome {
            root_cause_type: "node_failure".to_string(),
            confidence: 0.9,
            summary: "计算节点离线，优先处理节点故障：检查 BE 进程、网络与磁盘状态。"
                .to_string(),
            actions: vec![
                DiagnosisAction {
                    title: "检查离线 BE 节点".to_string(),
                    detail: "登录对应 BE 主机确认进程存活（ps/be 日志），检查磁盘剩余空间与网络连通性。"
                        .to_string(),
                    risk_level: "low".to_string(),
                },
                DiagnosisAction {
                    title: "观察恢复与负载分配".to_string(),
                    detail: "节点恢复后核对存活数（be_alive/be_total）回到 1.0，确认 tablet 均衡与查询无异常。"
                        .to_string(),
                    risk_level: "low".to_string(),
                },
            ],
        }
    } else if has(EventKind::DiskPressure) {
        DiagnosisOutcome {
            root_cause_type: "capacity".to_string(),
            confidence: if critical { 0.95 } else { 0.85 },
            summary: "磁盘水位超警戒，容量压力是当前主要风险：清理数据、检查副本分配与扩容。"
                .to_string(),
            actions: vec![
                DiagnosisAction {
                    title: "清理过期数据与垃圾文件".to_string(),
                    detail:
                        "检查 BE 数据目录 trash 清理策略、过期分区/TTL，必要时下线低热度表数据。"
                            .to_string(),
                    risk_level: "medium".to_string(),
                },
                DiagnosisAction {
                    title: "评估扩容或副本均衡".to_string(),
                    detail: "磁盘使用率 ≥90% 时优先扩容；确认 tablet 在 BE 间均衡分布。"
                        .to_string(),
                    risk_level: "medium".to_string(),
                },
            ],
        }
    } else if has(EventKind::Compaction) {
        DiagnosisOutcome {
            root_cause_type: "compaction_backlog".to_string(),
            confidence: 0.85,
            summary: "Compaction Score 偏高，compaction 积压拖慢合并与查询。".to_string(),
            actions: vec![
                DiagnosisAction {
                    title: "检查磁盘 IO 与 compaction 并发".to_string(),
                    detail: "确认 BE 磁盘 IO 未饱和；compaction score 持续高位时评估调整 max_compaction_concurrency。"
                        .to_string(),
                    risk_level: "low".to_string(),
                },
                DiagnosisAction {
                    title: "观察 score 趋势".to_string(),
                    detail: "compaction score 应随积压处理逐步回落，连续多轮高位需人工介入。".to_string(),
                    risk_level: "low".to_string(),
                },
            ],
        }
    } else if has(EventKind::LoadBacklog) {
        DiagnosisOutcome {
            root_cause_type: "load_backlog".to_string(),
            confidence: 0.8,
            summary: "导入任务积压，写入链路存在瓶颈（磁盘/compaction/节点负载）。".to_string(),
            actions: vec![DiagnosisAction {
                title: "定位积压瓶颈".to_string(),
                detail: "核对 BE 磁盘水位、compaction score 与负载；确认导入并发配置是否合理。"
                    .to_string(),
                risk_level: "low".to_string(),
            }],
        }
    } else if has(EventKind::TxnError) {
        DiagnosisOutcome {
            root_cause_type: "write_anomaly".to_string(),
            confidence: 0.75,
            summary: "事务失败激增，写入链路异常（节点/网络/并发冲突）。".to_string(),
            actions: vec![DiagnosisAction {
                title: "检查写入链路与节点健康".to_string(),
                detail: "查看事务失败明细、BE 存活与磁盘状态，确认是否存在节点级故障。".to_string(),
                risk_level: "medium".to_string(),
            }],
        }
    } else if has(EventKind::MetricBreach) || has(EventKind::SlowQuery) {
        DiagnosisOutcome {
            root_cause_type: "query_performance".to_string(),
            confidence: 0.7,
            summary: "查询错误/超时或慢查询增多，访问性能问题：结合慢查询取证与 Profile 分析定位。"
                .to_string(),
            actions: vec![DiagnosisAction {
                title: "分析慢查询与 Profile".to_string(),
                detail: "对照取证阶段的审计/Profile 证据，定位热点 SQL 与资源瓶颈。".to_string(),
                risk_level: "low".to_string(),
            }],
        }
    } else {
        DiagnosisOutcome {
            root_cause_type: "unknown".to_string(),
            confidence: 0.5,
            summary: "当前事件组合未命中已知剧本，建议扩大取证范围或人工介入。".to_string(),
            actions: vec![],
        }
    };

    // 置信度按严重度微调（有 critical 事件 +0.05，封顶 0.98）
    outcome.confidence = (outcome.confidence + if critical { 0.05 } else { 0.0 }).min(0.98);
    outcome
}

/// 输入摘要：事件集合 → input_digest（证据/事件 ID 集合 + 关键指标），用于决策审计比对。
pub fn digest_of(events: &[EventRow]) -> String {
    let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    json!({ "event_ids": ids, "kinds": kinds }).to_string()
}

/// 输出 JSON（决策审计留档用）
pub fn outcome_json(o: &DiagnosisOutcome) -> Value {
    json!(o)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_runtime::models::{EventSeverity, EventState};

    fn events_of(kinds: &[(EventKind, EventSeverity)]) -> Vec<EventRow> {
        kinds
            .iter()
            .enumerate()
            .map(|(i, (k, s))| EventRow {
                id: i as i64 + 1,
                cluster_id: 1,
                fingerprint: format!("{}:cluster:1", k.as_str()),
                kind: *k,
                severity: *s,
                object_type: Some("cluster".into()),
                object_id: Some("1".into()),
                title: String::new(),
                state: EventState::Open,
                occurrence_count: 1,
                first_seen_at: chrono::Utc::now(),
                last_seen_at: chrono::Utc::now(),
            })
            .collect()
    }

    fn ev(k: EventKind, s: EventSeverity) -> Vec<(EventKind, EventSeverity)> {
        vec![(k, s)]
    }

    #[test]
    fn node_down_beats_capacity_playbook() {
        // 节点故障优先于容量剧本
        let out = diagnose(&events_of(&ev(EventKind::NodeDown, EventSeverity::Critical)));
        assert_eq!(out.root_cause_type, "node_failure");
        assert!(out.confidence >= 0.9);
        assert!(!out.actions.is_empty());
    }

    #[test]
    fn disk_critical_maps_to_capacity_with_high_confidence() {
        let out = diagnose(&events_of(&ev(EventKind::DiskPressure, EventSeverity::Critical)));
        assert_eq!(out.root_cause_type, "capacity");
        assert!(out.confidence >= 0.95, "critical 应显著高于基础值");
    }

    #[test]
    fn disk_warning_lower_confidence_than_critical() {
        let w = diagnose(&events_of(&ev(EventKind::DiskPressure, EventSeverity::Warning)));
        let c = diagnose(&events_of(&ev(EventKind::DiskPressure, EventSeverity::Critical)));
        assert!(w.confidence < c.confidence);
    }

    #[test]
    fn compaction_playbook() {
        let out = diagnose(&events_of(&ev(EventKind::Compaction, EventSeverity::Warning)));
        assert_eq!(out.root_cause_type, "compaction_backlog");
        assert!(out.confidence >= 0.8);
    }

    #[test]
    fn load_backlog_playbook() {
        let out = diagnose(&events_of(&ev(EventKind::LoadBacklog, EventSeverity::Warning)));
        assert_eq!(out.root_cause_type, "load_backlog");
    }

    #[test]
    fn txn_error_playbook() {
        let out = diagnose(&events_of(&ev(EventKind::TxnError, EventSeverity::Warning)));
        assert_eq!(out.root_cause_type, "write_anomaly");
    }

    #[test]
    fn slow_query_performance_playbook() {
        let out = diagnose(&events_of(&ev(EventKind::SlowQuery, EventSeverity::Info)));
        assert_eq!(out.root_cause_type, "query_performance");
    }

    #[test]
    fn confidence_capped_at_0_98() {
        let out = diagnose(&events_of(&ev(EventKind::NodeDown, EventSeverity::Critical)));
        assert!(out.confidence <= 0.98);
    }

    #[test]
    fn unknown_when_no_pattern() {
        let out = diagnose(&[]);
        assert_eq!(out.root_cause_type, "unknown");
        assert!(out.confidence < 0.8);
    }

    #[test]
    fn combined_events_pick_highest_priority() {
        // compaction + txn：无 node_down/disk 时 compaction 优先
        let out = diagnose(&events_of(&[
            (EventKind::Compaction, EventSeverity::Warning),
            (EventKind::TxnError, EventSeverity::Warning),
        ]));
        assert_eq!(out.root_cause_type, "compaction_backlog");
    }

    #[test]
    fn digest_contains_event_ids_and_kinds() {
        let events = events_of(&ev(EventKind::DiskPressure, EventSeverity::Critical));
        let d = digest_of(&events);
        assert!(d.contains("\"event_ids\""));
        assert!(d.contains("\"disk_pressure\""));
    }
}
