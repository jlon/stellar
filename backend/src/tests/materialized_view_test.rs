use crate::{
    handlers::materialized_view::validate_create_materialized_view_request,
    middleware::permission_extractor::extract_permission,
    models::{
        MaterializedView, MaterializedViewKind, MaterializedViewRef, PartitionValue,
        RefreshIntervalUnit, RefreshMaterializedViewRequest, RefreshSchedule,
        UpdateRefreshScheduleRequest,
    },
    services::{MaterializedViewService, cluster_adapter::doris::DorisAdapter},
};
use serde_json::json;

#[test]
fn materialized_view_reference_rejects_ambiguous_identifiers() {
    let reference = MaterializedViewRef {
        database: "analytics".to_string(),
        name: "daily_sales".to_string(),
        kind: MaterializedViewKind::Async,
    };
    assert!(reference.validate().is_ok());

    let unsafe_reference = MaterializedViewRef {
        database: "analytics` DROP DATABASE mysql".to_string(),
        name: "daily_sales".to_string(),
        kind: MaterializedViewKind::Async,
    };
    assert!(unsafe_reference.validate().is_err());
}

#[test]
fn typed_refresh_request_rejects_arbitrary_alter_input() {
    let request = serde_json::from_value::<RefreshMaterializedViewRequest>(json!({
        "mode": "async",
        "alter_clause": "INACTIVE"
    }));
    assert!(request.is_err());

    let schedule = serde_json::from_value::<UpdateRefreshScheduleRequest>(json!({
        "schedule": { "kind": "scheduled", "interval": 8761, "unit": "hour" }
    }))
    .expect("typed schedule should deserialize");
    assert!(schedule.validate().is_err());

    let obsolete_schedule = serde_json::from_value::<UpdateRefreshScheduleRequest>(json!({
        "schedule": { "kind": "async", "interval": 1, "unit": "week" }
    }));
    assert!(obsolete_schedule.is_err());
}

#[test]
fn scheduled_refresh_uses_starrocks_schedule_syntax() {
    assert_eq!(
        MaterializedViewService::refresh_schedule_clause(RefreshSchedule::Scheduled {
            interval: 1,
            unit: RefreshIntervalUnit::Day,
        }),
        "REFRESH SCHEDULE EVERY (INTERVAL 1 DAY)"
    );
}

#[test]
fn starrocks_dependency_query_uses_complete_async_mv_identity() {
    let reference = MaterializedViewRef {
        database: "analytics".to_string(),
        name: "daily_sales".to_string(),
        kind: MaterializedViewKind::Async,
    };

    let query = MaterializedViewService::direct_dependencies_query(&reference);

    assert!(query.contains("object_database = 'analytics'"));
    assert!(query.contains("object_name = 'daily_sales'"));
    assert!(query.contains("object_type = 'MATERIALIZED_VIEW'"));
}

#[test]
fn starrocks_current_metadata_distinguishes_sync_rollups() {
    assert_eq!(
        MaterializedViewService::kind_from_refresh_type("SYNC"),
        MaterializedViewKind::Rollup
    );
    assert_eq!(
        MaterializedViewService::kind_from_refresh_type("ASYNC"),
        MaterializedViewKind::Async
    );
    assert_eq!(
        MaterializedViewService::rollup_parent_from_definition(
            "CREATE MATERIALIZED VIEW `orders_by_day` AS SELECT `day` FROM `analytics`.`orders` GROUP BY `day`"
        ),
        Some("orders".to_string())
    );
    assert_eq!(
        MaterializedViewService::rollup_parent_from_definition(
            "CREATE MATERIALIZED VIEW `orders_by_day` AS SELECT * FROM `analytics`.`orders.v1`"
        ),
        Some("orders.v1".to_string())
    );
}

#[test]
fn materialized_view_routes_require_existing_specific_permissions() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/materialized_views"),
        Some(("clusters".to_string(), "materialized_views".to_string()))
    );
    assert_eq!(
        extract_permission("POST", "/api/clusters/materialized_views"),
        Some(("clusters".to_string(), "materialized_views:create".to_string()))
    );
    assert_eq!(
        extract_permission(
            "PUT",
            "/api/clusters/materialized_views/analytics/daily_sales/async/refresh-schedule"
        ),
        Some(("clusters".to_string(), "materialized_views:alter".to_string()))
    );
}

#[test]
fn create_request_accepts_one_materialized_view_statement_only() {
    let valid = serde_json::from_value(json!({
        "sql": "CREATE MATERIALIZED VIEW daily_sales AS SELECT 1"
    }))
    .expect("a materialized view request should deserialize");
    assert!(validate_create_materialized_view_request(&valid).is_ok());

    let multiple = serde_json::from_value(json!({
        "sql": "CREATE MATERIALIZED VIEW daily_sales AS SELECT 1; DROP DATABASE analytics"
    }))
    .expect("request shape should deserialize before statement validation");
    assert!(validate_create_materialized_view_request(&multiple).is_err());

    let arbitrary = serde_json::from_value(json!({ "sql": "DROP DATABASE analytics" }))
        .expect("request shape should deserialize before statement validation");
    assert!(validate_create_materialized_view_request(&arbitrary).is_err());
}

#[test]
fn partition_values_are_escaped_as_literals() {
    let value = PartitionValue::String("east' OR 1=1".to_string());
    assert_eq!(value.sql_literal().unwrap(), "'east'' OR 1=1'");
}

#[test]
fn doris_dependency_extractor_skips_ctes_and_comments() {
    let definition = r#"
        /* FROM imaginary_table */
        WITH recent_orders AS (
          SELECT * FROM warehouse.orders
        )
        SELECT * FROM recent_orders JOIN reporting.customers ON 1 = 1
    "#;

    let (dependencies, warnings) =
        DorisAdapter::extract_doris_dependencies(definition, "analytics");

    assert_eq!(dependencies.len(), 2);
    assert!(dependencies.iter().any(|dependency| {
        dependency.object.database.as_deref() == Some("warehouse")
            && dependency.object.name == "orders"
    }));
    assert!(dependencies.iter().any(|dependency| {
        dependency.object.database.as_deref() == Some("reporting")
            && dependency.object.name == "customers"
    }));
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.object.name == "recent_orders")
    );
    assert!(
        !dependencies
            .iter()
            .any(|dependency| dependency.object.name == "imaginary_table")
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("Comments were ignored"))
    );
}

#[test]
fn doris_rollup_discovery_keeps_only_unambiguous_completed_jobs() {
    assert!(DorisAdapter::is_finished_rollup_state(Some("finished")));
    assert!(!DorisAdapter::is_finished_rollup_state(Some("CANCELLED")));

    let rollups = DorisAdapter::unique_rollups(vec![
        test_rollup("daily_rollup"),
        test_rollup("daily_rollup"),
        test_rollup("weekly_rollup"),
    ]);

    assert_eq!(rollups.len(), 1);
    assert_eq!(rollups[0].name, "weekly_rollup");
}

#[test]
fn doris_rollup_actions_require_current_table_metadata() {
    assert!(DorisAdapter::has_current_rollup(
        &[json!({ "IndexName": "daily_rollup" })],
        "daily_rollup"
    ));
    assert!(!DorisAdapter::has_current_rollup(
        &[json!({ "IndexName": "old_rollup" })],
        "daily_rollup"
    ));
}

fn test_rollup(name: &str) -> MaterializedView {
    MaterializedView {
        id: format!("rollup:analytics:orders:{name}"),
        name: name.to_string(),
        database_name: "analytics".to_string(),
        kind: MaterializedViewKind::Rollup,
        refresh_type: "ROLLUP".to_string(),
        is_active: true,
        partition_type: None,
        task_id: None,
        task_name: None,
        last_refresh_start_time: None,
        last_refresh_finished_time: None,
        last_refresh_duration: None,
        last_refresh_state: Some("FINISHED".to_string()),
        rows: None,
        definition: "-- fixture".to_string(),
    }
}
