use chrono::Utc;

use crate::services::agent_runtime::collectors::{compute_node_context, load_backlog_summary};
use crate::services::agent_runtime::diagnoser::diagnose;
use crate::services::agent_runtime::models::{EventKind, EventRow, EventSeverity, EventState};

fn node_down_event() -> EventRow {
    EventRow {
        id: 1,
        cluster_id: 1,
        fingerprint: "node_down:cn:cn-1".to_string(),
        kind: EventKind::NodeDown,
        severity: EventSeverity::Critical,
        object_type: Some("cn".to_string()),
        object_id: Some("cn-1".to_string()),
        title: "计算节点 cn-1 离线".to_string(),
        state: EventState::Open,
        occurrence_count: 1,
        first_seen_at: Utc::now(),
        last_seen_at: Utc::now(),
    }
}

#[test]
fn shared_data_events_use_cn_and_object_storage_context() {
    assert_eq!(compute_node_context(true), ("cn", "CN"));
    let summary = load_backlog_summary(true, 10);
    assert!(summary.contains("CN"));
    assert!(summary.contains("对象存储"));
    assert!(!summary.contains("BE 磁盘"));
}

#[test]
fn node_diagnosis_is_valid_for_compute_nodes_in_both_modes() {
    let outcome = diagnose(&[node_down_event()]);
    assert_eq!(outcome.root_cause_type, "node_failure");
    assert!(!outcome.summary.contains("BE"));
    assert!(
        outcome
            .actions
            .iter()
            .all(|action| !action.detail.contains("tablet"))
    );
}
