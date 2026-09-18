//! LLM 根因推理（第四阶段，原 M1 子集）。
//!
//! 规则诊断（`diagnoser`）未定论时，把事件 + 证据摘要交给 LLM 生成结构化假设，
//! 并执行**后置校验**（设计 §6.3）：
//! - 引用的证据 ID 必须存在且属于本 Incident；
//! - 无证据引用的假设直接丢弃；
//! - 全部非法 → `agent_decisions.status = rejected`，回退规则诊断摘要。
//!
//! LLM 仅解释根因，不生成动作参数（动作建议一律来自确定性剧本）。

use serde_json::{Value, json};
use std::collections::HashSet;

use super::models::EventRow;
use super::service::EvidenceSummary;

/// 结构化假设（LLM 输出契约）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Hypothesis {
    pub title: String,
    pub root_cause_type: String,
    pub confidence: f64,
    pub supporting_evidence_ids: Vec<i64>,
    #[serde(default)]
    pub counter_evidence_ids: Vec<i64>,
    #[serde(default)]
    pub missing_evidence: Vec<String>,
    #[serde(default)]
    pub recommended_next_checks: Vec<String>,
}

/// 构建 LLM 调用消息（system + user）。
pub fn build_messages(
    incident_title: &str,
    impact_summary: Option<&str>,
    events: &[EventRow],
    evidences: &[EvidenceSummary],
) -> Vec<crate::services::ai::types::ChatMessage> {
    let system = crate::services::ai::types::ChatMessage {
        role: "system".into(),
        content: "你是 StarRocks / Apache Doris OLAP 集群的根因分析专家。\
                  你只能基于给定证据推理，禁止编造事实。\
                  输出严格的 JSON 数组（不要 markdown 代码块、不要额外文字），\
                  每个元素格式：\
                  {\"title\": string, \"root_cause_type\": string, \"confidence\": number 0-1, \
                   \"supporting_evidence_ids\": [int], \"counter_evidence_ids\": [int], \
                   \"missing_evidence\": [string], \"recommended_next_checks\": [string]}.\
                  root_cause_type 取值：node_failure | resource_saturation | data_skew | \
                  config_regression | workload_spike | capacity | write_anomaly | \
                  compaction_backlog | load_backlog | query_performance | unknown。"
            .into(),
        tool_calls: None,
        tool_call_id: None,
    };

    let events_txt: Vec<String> = events
        .iter()
        .map(|e| {
            format!(
                "- event#{} [{}] severity={} object={}:{} count={} title={}",
                e.id,
                e.kind.as_str(),
                e.severity.as_str(),
                e.object_type.as_deref().unwrap_or("-"),
                e.object_id.as_deref().unwrap_or("-"),
                e.occurrence_count,
                e.title
            )
        })
        .collect();
    let evidence_txt: Vec<String> = evidences
        .iter()
        .map(|e| {
            format!(
                "- evidence#{} stage={} collector={} quality={}: {}",
                e.id, e.stage, e.collector, e.quality, e.summary
            )
        })
        .collect();

    let user = crate::services::ai::types::ChatMessage {
        role: "user".into(),
        content: format!(
            "Incident: {}\n影响: {}\n\n关联事件:\n{}\n\n证据:\n{}\n\n请给出最多 3 个根因假设，\
             每个假设必须引用至少一个 existed evidence id。",
            incident_title,
            impact_summary.unwrap_or("无"),
            events_txt.join("\n"),
            evidence_txt.join("\n")
        ),
        tool_calls: None,
        tool_call_id: None,
    };
    vec![system, user]
}

/// 解析 LLM 输出为假设列表（容忍 markdown 代码块包裹与前后噪音）。
pub fn parse_hypotheses(text: &str) -> Vec<Hypothesis> {
    let cleaned = strip_markdown_fence(text);
    // 取第一个 JSON 数组片段（LLM 可能加说明文字）
    let start = cleaned.find('[');
    let end = cleaned.rfind(']');
    let slice = match (start, end) {
        (Some(s), Some(e)) if e > s => &cleaned[s..=e],
        _ => return Vec::new(),
    };
    serde_json::from_str::<Vec<Hypothesis>>(slice).unwrap_or_default()
}

/// 后置校验：证据引用必须存在；无引用的假设丢弃。返回 (通过, 丢弃数, 错误)。
pub fn validate_hypotheses(
    hypotheses: Vec<Hypothesis>,
    valid_evidence_ids: &HashSet<i64>,
) -> (Vec<Hypothesis>, usize, Vec<String>) {
    let mut kept = Vec::new();
    let mut dropped = 0usize;
    let mut errors = Vec::new();
    for (i, h) in hypotheses.into_iter().enumerate() {
        // 引用必须全部存在（counter 引用缺失可容忍，只校验 supporting）
        let missing: Vec<i64> = h
            .supporting_evidence_ids
            .iter()
            .filter(|id| !valid_evidence_ids.contains(id))
            .copied()
            .collect();
        if h.supporting_evidence_ids.is_empty() {
            dropped += 1;
            errors.push(format!("假设 #{}「{}」没有引用任何证据", i + 1, h.title));
            continue;
        }
        if !missing.is_empty() {
            dropped += 1;
            errors.push(format!("假设 #{}「{}」引用了不存在的证据 {:?}", i + 1, h.title, missing));
            continue;
        }
        let confidence = h.confidence.clamp(0.0, 1.0);
        kept.push(Hypothesis { confidence, ..h });
    }
    (kept, dropped, errors)
}

fn strip_markdown_fence(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        rest.strip_suffix("```")
            .map(str::to_string)
            .unwrap_or_else(|| rest.to_string())
    } else if let Some(rest) = s.strip_prefix("```") {
        rest.strip_suffix("```")
            .map(str::to_string)
            .unwrap_or_else(|| rest.to_string())
    } else {
        s.to_string()
    }
}

/// 回退输出：规则诊断摘要（LLM 不可用或全部假设被拒时返回前端）。
pub fn fallback_outcome(rule_summary: Option<&str>) -> Value {
    json!({
        "hypotheses": [],
        "fallback": "llm_unavailable_or_rejected",
        "rule_diagnosis": rule_summary.unwrap_or("无规则诊断结论"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence_ids(ids: &[i64]) -> HashSet<i64> {
        ids.iter().copied().collect()
    }

    #[test]
    fn parse_plain_json_array() {
        let text = r#"[{"title":"磁盘饱和","root_cause_type":"capacity","confidence":0.8,"supporting_evidence_ids":[1,2]}]"#;
        let hs = parse_hypotheses(text);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].title, "磁盘饱和");
        assert_eq!(hs[0].supporting_evidence_ids, vec![1, 2]);
    }

    #[test]
    fn parse_markdown_fenced_json() {
        let text = "```json\n[{\"title\":\"a\",\"root_cause_type\":\"unknown\",\"confidence\":0.5,\"supporting_evidence_ids\":[1]}]\n```";
        let hs = parse_hypotheses(text);
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].title, "a");
    }

    #[test]
    fn parse_garbage_returns_empty() {
        assert!(parse_hypotheses("抱歉，我无法分析。").is_empty());
        assert!(parse_hypotheses("").is_empty());
    }

    #[test]
    fn validate_drops_no_evidence_hypothesis() {
        let (kept, dropped, errors) = validate_hypotheses(
            vec![Hypothesis {
                title: "无依据".into(),
                root_cause_type: "unknown".into(),
                confidence: 0.9,
                supporting_evidence_ids: vec![],
                counter_evidence_ids: vec![],
                missing_evidence: vec![],
                recommended_next_checks: vec![],
            }],
            &evidence_ids(&[1]),
        );
        assert_eq!(kept.len(), 0);
        assert_eq!(dropped, 1);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn validate_drops_bogus_evidence_ref() {
        let (kept, dropped, _) = validate_hypotheses(
            vec![Hypothesis {
                title: "引用不存在证据".into(),
                root_cause_type: "capacity".into(),
                confidence: 0.7,
                supporting_evidence_ids: vec![99],
                counter_evidence_ids: vec![],
                missing_evidence: vec![],
                recommended_next_checks: vec![],
            }],
            &evidence_ids(&[1]),
        );
        assert_eq!(kept.len(), 0);
        assert_eq!(dropped, 1);
    }

    #[test]
    fn validate_keeps_valid_and_clamps_confidence() {
        let (kept, dropped, _) = validate_hypotheses(
            vec![Hypothesis {
                title: "合理假设".into(),
                root_cause_type: "capacity".into(),
                confidence: 1.7,
                supporting_evidence_ids: vec![1],
                counter_evidence_ids: vec![],
                missing_evidence: vec![],
                recommended_next_checks: vec![],
            }],
            &evidence_ids(&[1]),
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(dropped, 0);
        assert!((kept[0].confidence - 1.0).abs() < 1e-9, "confidence 应钳制到 1.0");
    }

    #[test]
    fn build_messages_contains_evidence_ids() {
        let msgs = build_messages(
            "t",
            Some("s"),
            &[],
            &[EvidenceSummary {
                id: 7,
                stage: "metrics".into(),
                collector: "x".into(),
                quality: "strong".into(),
                summary: "payload".into(),
            }],
        );
        assert_eq!(msgs.len(), 2);
        assert!(msgs[1].content.contains("evidence#7"));
        assert!(msgs[0].content.contains("supporting_evidence_ids"));
    }
}
