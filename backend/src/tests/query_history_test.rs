use crate::handlers::query_history::{
    normalize_audit_timestamp, normalize_history_pagination, quote_sql_literal,
};

#[test]
fn audit_filters_accept_only_normalized_timestamps_and_escaped_literals() {
    assert_eq!(
        normalize_audit_timestamp(Some("2026-09-24T10:08")).expect("valid timestamp"),
        Some("2026-09-24 10:08:00".to_string())
    );
    assert!(normalize_audit_timestamp(Some("2026-09-24' OR 1=1 --")).is_err());
    assert_eq!(quote_sql_literal(r"x\'y"), r"'x\\''y'");
}

#[test]
fn audit_history_pagination_is_bounded_and_nonnegative() {
    assert_eq!(normalize_history_pagination(0, -1), (1, 0));
    assert_eq!(normalize_history_pagination(500, 30), (100, 30));
}
