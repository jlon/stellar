use crate::middleware::permission_extractor::extract_permission;
use crate::models::{LoadFailureCause, LoadJob, LoadQueryParams, RoutineLoadDetails};
use crate::services::load_service::{
    available_actions, build_doris_load_failure_query, build_information_schema_query,
    build_stage_timeline, build_starrocks_load_query, classify_failure, encode_load_cursor,
    find_doris_load_failure_details, parse_load_job, parse_routine_load_task, routine_load_name,
};

#[test]
fn classifies_common_failure_causes() {
    let timeout = classify_failure("load timeout").unwrap();
    assert_eq!(timeout.code, "Timeout");
    assert!(timeout.suggestion.contains("计算节点"));
    assert!(!timeout.suggestion.contains("FE/BE"));
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
fn load_actions_reuse_the_sql_execute_permission() {
    assert_eq!(
        extract_permission("POST", "/api/clusters/loads/42/actions"),
        Some(("clusters".to_string(), "queries:execute".to_string()))
    );
    // 读取仍走独立的 loads 权限
    assert_eq!(
        extract_permission("GET", "/api/clusters/loads"),
        Some(("clusters".to_string(), "loads".to_string()))
    );
}

#[test]
fn every_failure_cause_carries_executable_steps() {
    let samples = [
        ("load timeout", "Timeout"),
        ("filtered ratio exceeds max_filter_ratio", "ThresholdExceeded"),
        ("Access denied for user", "PermissionDenied"),
        ("unknown table t", "TargetMissing"),
        ("parse error while reading csv", "FormatError"),
        ("out of memory in resource group", "ResourceExhausted"),
        ("something nobody classified", "Unknown"),
    ];

    for (message, expected_code) in samples {
        let cause = classify_failure(message).expect("cause");
        assert_eq!(cause.code, expected_code);
        assert!(!cause.steps.is_empty(), "{expected_code} 缺少修复步骤");
        assert!(
            cause.steps.iter().all(|step| step.trim().len() > 8),
            "{expected_code} 的步骤过于笼统"
        );
    }
}

/// 处置动作只按引擎返回的父作业状态给出：PAUSED 可恢复、运行中可暂停、终态不给。
#[test]
fn offers_routine_actions_only_for_known_parent_states() {
    let build = |state: Option<&str>| LoadJob {
        job_id: Some("42".to_string()),
        label: Some("events_topic".to_string()),
        database: Some("analytics".to_string()),
        state: "RUNNING".to_string(),
        load_type: "ROUTINE_LOAD".to_string(),
        properties: Some(r#"{"job_name":"events_topic_load"}"#.to_string()),
        routine_load: Some(RoutineLoadDetails {
            state: state.map(ToOwned::to_owned),
            current_task_num: None,
            statistics: None,
            progress: None,
            timestamp_progress: None,
            latest_source_position: None,
            offset_lag: None,
            reason_of_state_changed: None,
            error_log_urls: None,
            tracking_sql: None,
            other_msg: None,
            tasks: Vec::new(),
        }),
        ..Default::default()
    };

    let paused = available_actions(&build(Some("PAUSED"))).unwrap();
    assert_eq!(paused.len(), 1);
    assert_eq!(paused[0].action, "resume_routine");
    assert_eq!(paused[0].statement, "RESUME ROUTINE LOAD FOR `analytics`.`events_topic_load`");

    let running = available_actions(&build(Some("RUNNING"))).unwrap();
    assert_eq!(running[0].action, "pause_routine");
    assert_eq!(running[0].statement, "PAUSE ROUTINE LOAD FOR `analytics`.`events_topic_load`");

    // 终态与未知状态不猜测动作
    assert!(
        available_actions(&build(Some("STOPPED")))
            .unwrap()
            .is_empty()
    );
    assert!(available_actions(&build(None)).unwrap().is_empty());

    // 非 Routine Load 没有父作业名，不给动作
    let stream = LoadJob {
        job_id: Some("7".to_string()),
        load_type: "STREAM_LOAD".to_string(),
        ..Default::default()
    };
    assert!(available_actions(&stream).unwrap().is_empty());
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
fn load_routes_use_dedicated_load_permission() {
    assert_eq!(
        extract_permission("GET", "/api/clusters/loads"),
        Some(("clusters".to_string(), "loads".to_string()))
    );
    assert_eq!(
        extract_permission("GET", "/api/clusters/loads/42"),
        Some(("clusters".to_string(), "loads".to_string()))
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
fn routine_load_uses_its_confirmed_parent_name_and_task_columns() {
    let job = parse_load_job(
        &["Type".to_string(), "Properties".to_string()],
        &["ROUTINE".to_string(), r#"{"job_name":"kafka_ingest"}"#.to_string()],
        Some("sales"),
    );
    let task = parse_routine_load_task(
        &[
            "TaskId".to_string(),
            "TxnId".to_string(),
            "TxnStatus".to_string(),
            "BeId".to_string(),
            "DataSourceProperties".to_string(),
            "Message".to_string(),
        ],
        &[
            "task-1".to_string(),
            "99".to_string(),
            "COMMITTED".to_string(),
            "10001".to_string(),
            r#"Progress:{"0":42}"#.to_string(),
            "running".to_string(),
        ],
    );

    assert_eq!(routine_load_name(&job).as_deref(), Some("kafka_ingest"));
    assert_eq!(task.txn_status.as_deref(), Some("COMMITTED"));
    assert_eq!(task.be_id.as_deref(), Some("10001"));
    assert_eq!(task.data_source_properties.as_deref(), Some(r#"Progress:{"0":42}"#));
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
    assert!(query.ends_with("LIMIT 501"));
}

#[test]
fn starrocks_query_unions_history_and_uses_a_stable_cursor() {
    let cursor = encode_load_cursor("2026-01-01 00:00:00", "42").unwrap();
    let query = build_starrocks_load_query(
        &LoadQueryParams {
            cursor: Some(cursor),
            range: Some("all".to_string()),
            ..Default::default()
        },
        101,
    )
    .unwrap();

    assert!(query.contains("FROM information_schema.loads"));
    assert!(query.contains("FROM `_statistics_`.loads_history"));
    assert!(query.contains(" UNION "));
    assert!(query.contains("CREATE_TIME < '2026-01-01 00:00:00'"));
    assert!(query.contains("CREATE_TIME = '2026-01-01 00:00:00' AND ID < 42"));
    assert!(query.ends_with("ORDER BY create_time DESC, job_id DESC LIMIT 101"));
}

#[test]
fn detail_query_uses_an_exact_numeric_job_id() {
    let query = build_information_schema_query(
        &LoadQueryParams { job_id: Some("42".to_string()), ..Default::default() },
        501,
    )
    .unwrap();

    assert!(query.contains(" AND ID = 42"));
    assert!(!query.contains("CAST(ID AS CHAR) LIKE"));
    assert!(
        build_information_schema_query(
            &LoadQueryParams { job_id: Some("42 OR 1=1".to_string()), ..Default::default() },
            1,
        )
        .is_err()
    );
}

#[test]
fn doris_failure_details_are_label_scoped_and_job_id_matched() {
    let query = build_doris_load_failure_query("sales", "batch'o").unwrap();
    let columns = vec![
        "JobId".to_string(),
        "URL".to_string(),
        "ErrorMsg".to_string(),
        "JobDetails".to_string(),
    ];
    let rows = vec![
        vec![
            "41".to_string(),
            "https://errors/41".to_string(),
            "other failure".to_string(),
            "{\"id\":41}".to_string(),
        ],
        vec![
            "42".to_string(),
            "https://errors/42".to_string(),
            "selected failure".to_string(),
            "{\"id\":42}".to_string(),
        ],
    ];

    let details = find_doris_load_failure_details(&columns, &rows, "42").unwrap();
    assert_eq!(query, "SHOW LOAD FROM `sales` WHERE LABEL = 'batch''o'");
    assert_eq!(details.url.as_deref(), Some("https://errors/42"));
    assert_eq!(details.error_msg.as_deref(), Some("selected failure"));
    assert_eq!(details.job_details.as_deref(), Some("{\"id\":42}"));
    assert!(find_doris_load_failure_details(&columns, &rows, "404").is_none());
    assert!(
        find_doris_load_failure_details(
            &columns,
            &[vec!["42".to_string(), "-".to_string(), "null".to_string(), "".to_string(),]],
            "42",
        )
        .is_none()
    );
}
