use crate::{
    middleware::permission_extractor::extract_permission,
    services::{audit_log_service::AuditQuerySample, mv_opportunity_service::build_opportunities},
};

fn sample(sql: &str, duration_ms: u64, timestamp: &str) -> AuditQuerySample {
    AuditQuerySample {
        database: "analytics".to_string(),
        timestamp: timestamp.to_string(),
        duration_ms,
        stmt: sql.to_string(),
    }
}

#[test]
fn mv_opportunities_group_literal_variants_without_exposing_values() {
    let (candidates, truncated) = build_opportunities(vec![
        sample(
            "SELECT day, SUM(amount) FROM orders WHERE store_id = 12 AND country = 'CN' GROUP BY day",
            100,
            "2026-09-28 08:00:00",
        ),
        sample(
            "SELECT day, SUM(amount) FROM orders WHERE store_id = 24 AND country = 'US' GROUP BY day",
            200,
            "2026-09-28 09:00:00",
        ),
        sample(
            "/* tenant */ SELECT day, SUM(amount) FROM orders WHERE store_id = 36 AND country = 'JP' GROUP BY day",
            300,
            "2026-09-28 10:00:00",
        ),
    ]);

    assert!(!truncated);
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate.source_database, "analytics");
    assert_eq!(candidate.source_table, "orders");
    assert_eq!(candidate.execution_count, 3);
    assert_eq!(candidate.total_duration_ms, 600);
    assert_eq!(candidate.average_duration_ms, 200);
    assert_eq!(candidate.p95_duration_ms, 300);
    assert!(candidate.sql_pattern.contains("store_id = ?"));
    assert!(candidate.sql_pattern.contains("country = ?"));
    assert!(!candidate.sql_pattern.contains("12"));
    assert!(!candidate.sql_pattern.contains("CN"));
    assert!(!candidate.sql_pattern.contains("tenant"));
}

#[test]
fn mv_opportunities_reject_unsafe_or_ambiguous_shapes() {
    let (candidates, _) = build_opportunities(vec![
        sample(
            "SELECT day, SUM(amount) FROM orders JOIN stores ON orders.store_id = stores.id GROUP BY day",
            100,
            "2026-09-28 08:00:00",
        ),
        sample(
            "WITH daily AS (SELECT day, SUM(amount) AS amount FROM orders GROUP BY day) SELECT day, SUM(amount) FROM daily GROUP BY day",
            100,
            "2026-09-28 09:00:00",
        ),
        sample(
            "SELECT day, SUM(amount) FROM orders, stores WHERE orders.store_id = stores.id GROUP BY day",
            100,
            "2026-09-28 09:30:00",
        ),
        sample("SELECT day, amount FROM orders GROUP BY day, amount", 100, "2026-09-28 10:00:00"),
    ]);

    assert!(candidates.is_empty());
}

#[test]
fn mv_opportunities_keep_unqualified_queries_in_their_source_database() {
    let mut analytics =
        sample("SELECT day, SUM(amount) FROM orders GROUP BY day", 100, "2026-09-28 08:00:00");
    let mut reporting = analytics.clone();
    reporting.database = "reporting".to_string();
    analytics.timestamp = "2026-09-28 09:00:00".to_string();

    let (candidates, _) = build_opportunities(vec![
        analytics.clone(),
        analytics.clone(),
        analytics,
        reporting.clone(),
        reporting.clone(),
        reporting,
    ]);

    assert_eq!(candidates.len(), 2);
    let databases = candidates
        .iter()
        .map(|candidate| candidate.source_database.as_str())
        .collect::<Vec<_>>();
    assert!(databases.contains(&"analytics"));
    assert!(databases.contains(&"reporting"));
}

#[test]
fn mv_opportunity_endpoint_requires_existing_mv_detail_permission() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/materialized_views/opportunities"),
        Some(("clusters".to_string(), "materialized_views:get".to_string()))
    );
}
