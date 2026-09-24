use crate::services::ai::llm::truncate;

#[test]
fn truncation_preserves_utf8_boundaries() {
    assert_eq!(truncate("服务不可用", 3), "服务不...");
}
