//! Agent runtime data models: events, incidents, evidences, decisions.
//! DB 存字符串枚举值（跨 sqlite/mysql/postgres 兼容）。

use serde::Serialize;
use serde_json::{Value, json};

/// Bump when the output contract in `llm_diagnosis::build_messages` changes.
pub(crate) const LLM_HYPOTHESIS_PROMPT_VERSION: &str = "v1";

/// Runtime metadata for one LLM-backed decision. Credentials and provider URLs
/// are intentionally excluded from the audit trail.
#[derive(Debug, Clone)]
pub(crate) struct LlmDecisionTelemetry {
    pub provider: String,
    pub model: String,
    pub tokens: Option<i64>,
    pub latency_ms: i64,
}

pub(crate) fn with_llm_trace(input: Option<Value>, telemetry: &LlmDecisionTelemetry) -> Value {
    let trace = json!({
        "prompt_version": LLM_HYPOTHESIS_PROMPT_VERSION,
        "latency_ms": telemetry.latency_ms,
    });
    match input {
        Some(Value::Object(mut object)) => {
            object.insert("llm_trace".to_string(), trace);
            Value::Object(object)
        },
        Some(input) => json!({ "input": input, "llm_trace": trace }),
        None => json!({ "llm_trace": trace }),
    }
}

/// 事件种类（DB: agent_events.kind）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    NodeDown,
    DiskPressure,
    Compaction,
    LoadBacklog,
    TxnError,
    MetricBreach,
    SlowQuery,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NodeDown => "node_down",
            Self::DiskPressure => "disk_pressure",
            Self::Compaction => "compaction",
            Self::LoadBacklog => "load_backlog",
            Self::TxnError => "txn_error",
            Self::MetricBreach => "metric_breach",
            Self::SlowQuery => "slow_query",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "node_down" => Some(Self::NodeDown),
            "disk_pressure" => Some(Self::DiskPressure),
            "compaction" => Some(Self::Compaction),
            "load_backlog" => Some(Self::LoadBacklog),
            "txn_error" => Some(Self::TxnError),
            "metric_breach" => Some(Self::MetricBreach),
            "slow_query" => Some(Self::SlowQuery),
            _ => None,
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 事件严重级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Info,
    Warning,
    Critical,
}

impl EventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

/// 事件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventState {
    Open,
    Aggregated,
    Suppressed,
    Resolved,
}

impl EventState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Aggregated => "aggregated",
            Self::Suppressed => "suppressed",
            Self::Resolved => "resolved",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "aggregated" => Some(Self::Aggregated),
            "suppressed" => Some(Self::Suppressed),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
}

/// Incident 状态（M0 使用子集：open / investigating / resolved / closed）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentStatus {
    Open,
    Investigating,
    Resolved,
    Closed,
}

impl IncidentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Investigating => "investigating",
            Self::Resolved => "resolved",
            Self::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "open" => Some(Self::Open),
            "investigating" => Some(Self::Investigating),
            "resolved" => Some(Self::Resolved),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

/// 采集器产生的事件候选（尚未入库/收敛）
#[derive(Debug, Clone)]
pub struct EventCandidate {
    pub fingerprint: String,
    pub kind: EventKind,
    pub severity: EventSeverity,
    pub object_type: Option<String>,
    pub object_id: Option<String>,
    pub title: String,
    pub summary: String,
    pub metrics: Value,
    /// true = 该指纹的持续状态本轮已消退（用于把 open 事件置 resolved）
    pub cleared: bool,
}

/// 已入库的事件行（用于收敛与 Incident 聚合）
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventRow {
    pub id: i64,
    pub cluster_id: i64,
    pub fingerprint: String,
    pub kind: EventKind,
    pub severity: EventSeverity,
    pub object_type: Option<String>,
    pub object_id: Option<String>,
    pub title: String,
    pub state: EventState,
    pub occurrence_count: i64,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}

/// Incident 行
#[derive(Debug, Clone, Serialize)]
pub struct IncidentRow {
    pub id: i64,
    pub cluster_id: i64,
    pub dedupe_key: String,
    pub title: String,
    pub impact_summary: Option<String>,
    pub status: String,
    pub output_json: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub resolved_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 规则诊断产出（存 agent_decisions.output_json）
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisOutcome {
    pub root_cause_type: String,
    pub confidence: f64,
    pub summary: String,
    pub actions: Vec<DiagnosisAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisAction {
    pub title: String,
    pub detail: String,
    pub risk_level: String, // low / medium / high（M0 只给建议，不执行）
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_roundtrip() {
        for k in [
            EventKind::NodeDown,
            EventKind::DiskPressure,
            EventKind::Compaction,
            EventKind::LoadBacklog,
            EventKind::TxnError,
            EventKind::MetricBreach,
            EventKind::SlowQuery,
        ] {
            assert_eq!(EventKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(EventKind::parse("nope"), None);
    }

    #[test]
    fn event_state_roundtrip() {
        for s in ["open", "aggregated", "suppressed", "resolved"] {
            assert_eq!(EventState::parse(s).unwrap().as_str(), s);
        }
        assert_eq!(EventState::parse("?"), None);
    }

    #[test]
    fn severity_orders() {
        assert_eq!(EventSeverity::Info.as_str(), "info");
        assert_eq!(EventSeverity::Warning.as_str(), "warning");
        assert_eq!(EventSeverity::Critical.as_str(), "critical");
    }
}
