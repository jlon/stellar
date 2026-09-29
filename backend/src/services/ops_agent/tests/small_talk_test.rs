use super::super::context::{
    build_non_diagnostic_system_message, is_small_talk, requires_cluster_evidence,
};

#[test]
fn recognizes_pure_social_messages_without_matching_operational_questions() {
    for message in ["你好", "您好！", "Hello.", "thanks", "再见"] {
        assert!(is_small_talk(message), "{message} should be a social turn");
    }

    for message in ["你好，集群现在健康吗？", "查询 p99 延迟", "谢谢，继续检查节点"]
    {
        assert!(!is_small_talk(message), "{message} needs normal intent handling");
    }
}

#[test]
fn social_turn_system_message_has_no_cluster_or_diagnostic_context() {
    let message = build_non_diagnostic_system_message();

    assert!(!message.content.contains("当前集群快照"));
    assert!(!message.content.contains("query_metrics"));
    assert!(!message.content.contains("## 结论"));
    assert!(message.content.contains("不得调用工具"));
}

#[test]
fn only_explicit_live_cluster_questions_enable_evidence_collection() {
    for message in [
        "你好，集群现在健康吗？",
        "最近查询变慢了，帮我诊断",
        "当前 p99 延迟是多少？",
        "终止正在运行的查询",
        "kill query 123",
        "设置集群变量 query_timeout",
    ] {
        assert!(requires_cluster_evidence(message), "{message} should enable evidence collection");
    }

    for message in
        ["你好", "还", "你能做什么？", "什么是 Compaction？", "SQL 是什么？", "如何查看集群状态？"]
    {
        assert!(!requires_cluster_evidence(message), "{message} should not read cluster data");
    }
}
