use serde_json::json;

use super::super::agent::tool_call_key;

#[test]
fn identical_tool_calls_share_a_deduplication_key() {
    assert_eq!(
        tool_call_key("query_metrics", &json!({"end": 2, "start": 1})),
        tool_call_key("query_metrics", &json!({"start": 1, "end": 2})),
    );
}

#[test]
fn deduplication_key_includes_the_tool_name() {
    let args = json!({"limit": 10});
    assert_ne!(tool_call_key("query_metrics", &args), tool_call_key("query_nodes", &args),);
}
