use serde_json::json;

use crate::services::agent_runtime::models::{LlmDecisionTelemetry, with_llm_trace};

#[test]
fn llm_trace_keeps_existing_evidence_input_and_excludes_credentials() {
    let input = with_llm_trace(
        Some(json!({ "evidence_ids": [7, 9] })),
        &LlmDecisionTelemetry {
            provider: "primary".into(),
            model: "model-a".into(),
            tokens: Some(42),
            latency_ms: 123,
        },
    );

    assert_eq!(input["evidence_ids"], json!([7, 9]));
    assert_eq!(input["llm_trace"]["prompt_version"], "v1");
    assert_eq!(input["llm_trace"]["latency_ms"], 123);
    assert!(input.get("api_key").is_none());
    assert!(input.get("api_base").is_none());
}
