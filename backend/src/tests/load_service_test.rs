use crate::middleware::permission_extractor::extract_permission;
use crate::models::LoadQueryParams;
use crate::services::load_service::{
    build_information_schema_query, build_stage_timeline, classify_failure, parse_load_job,
};

#[test]
fn classifies_common_failure_causes() {
    assert_eq!(classify_failure("load timeout").unwrap().code, "Timeout");
    assert_eq!(
        classify_failure("filtered ratio exceeds max_filter_ratio")
            .unwrap()
            .code,
        "ThresholdExceeded"
    );
    assert_eq!(
        classify_failure("ETL_QUALITY_UNSATISFIED: filtered rows too high")
            .unwrap()
            .code,
        "ThresholdExceeded"
    );
    assert_eq!(classify_failure("unknown table t").unwrap().code, "TargetMissing");
}

#[test]
fn builds_only_real_timestamp_stages() {
    let stages = build_stage_timeline(
        Some("2026-01-01 00:00:00"),
        Some("2026-01-01 00:00:02"),
        Some("2026-01-01 00:00:05"),
        Some("2026-01-01 00:00:06"),
        "FINISHED",
    );
    assert_eq!(stages.len(), 3);
    assert_eq!(stages[0].duration_ms, 2_000);
    assert_eq!(stages[1].duration_ms, 3_000);
    assert_eq!(stages[2].duration_ms, 1_000);
}

#[test]
fn terminal_jobs_without_timestamps_have_no_fake_stages() {
    assert!(build_stage_timeline(None, None, None, None, "FINISHED").is_empty());
    assert!(
        build_stage_timeline(Some("2026-01-01 00:00:00"), None, None, None, "FINISHED",).is_empty()
    );
}

#[test]
fn load_routes_reuse_query_permission() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/loads"),
        Some(("clusters".to_string(), "queries".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/clusters/loads/42"),
        Some(("clusters".to_string(), "queries".to_string()))
    );
}

#[test]
fn show_load_columns_are_normalized_into_the_shared_dto() {
    let columns = vec![
        "JobId".to_string(),
        "Label".to_string(),
        "Type".to_string(),
        "EtlInfo".to_string(),
        "TaskInfo".to_string(),
        "JobDetails".to_string(),
        "State".to_string(),
        "CreateTime".to_string(),
        "LoadStartTime".to_string(),
        "LoadFinishTime".to_string(),
        "ErrorMsg".to_string(),
    ];
    let row = vec![
        "42".to_string(),
        "daily".to_string(),
        "BROKER".to_string(),
        "unselected.rows=2; dpp.abnorm.ALL=3; dpp.norm.ALL=95".to_string(),
        "resource:N/A; timeout(s):300; max_filter_ratio:0.0".to_string(),
        r#"{"ScannedRows":100,"FileSize":2048}"#.to_string(),
        "CANCELLED".to_string(),
        "2026-01-01 00:00:00".to_string(),
        "2026-01-01 00:00:02".to_string(),
        "2026-01-01 00:00:03".to_string(),
        "load timeout".to_string(),
    ];
    let job = parse_load_job(&columns, &row, Some("sales"));
    assert_eq!(job.job_id.as_deref(), Some("42"));
    assert_eq!(job.database.as_deref(), Some("sales"));
    assert_eq!(job.load_type, "BROKER_LOAD");
    assert_eq!(job.scan_rows, Some(100));
    assert_eq!(job.scan_bytes, None);
    assert_eq!(
        job.properties.as_deref(),
        Some("resource:N/A; timeout(s):300; max_filter_ratio:0.0")
    );
    assert_eq!(job.runtime_details.as_deref(), Some(r#"{"ScannedRows":100,"FileSize":2048}"#));
    assert_eq!(job.filtered_rows, Some(3));
    assert_eq!(job.sink_rows, Some(95));
    assert_eq!(job.failure_cause.as_ref().unwrap().code, "Timeout");
    assert_eq!(job.stage_timeline.len(), 2);
}

#[test]
fn routine_loads_do_not_get_batch_timeline() {
    let columns = vec![
        "Type".to_string(),
        "CreateTime".to_string(),
        "LoadStartTime".to_string(),
        "LoadFinishTime".to_string(),
    ];
    let row = vec![
        "ROUTINE_LOAD".to_string(),
        "2026-01-01 00:00:00".to_string(),
        "2026-01-01 00:00:01".to_string(),
        "2026-01-01 00:00:02".to_string(),
    ];

    let job = parse_load_job(&columns, &row, Some("sales"));
    assert!(job.stage_timeline.is_empty());
}

#[test]
fn load_query_escapes_literals_and_limits_rows() {
    let query = build_information_schema_query(
        &LoadQueryParams {
            db: Some("sales'o".to_string()),
            search: Some("batch'1".to_string()),
            range: Some("all".to_string()),
            ..Default::default()
        },
        9999,
    )
    .unwrap();

    assert!(query.contains("DB_NAME = 'sales''o'"));
    assert!(query.contains("LIKE '%batch''1%'"));
    assert!(query.ends_with("LIMIT 500"));
}
