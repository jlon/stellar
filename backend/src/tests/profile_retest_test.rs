use crate::handlers::profile::{
    build_retest_runs, extract_query_sql, merge_runs, normalize_time, parse_profile_time,
};
use crate::models::ProfileListItem;
use crate::models::ProfileRetestRun;
use crate::services::audit_log_service::like_hint;

fn item(query_id: &str, start: &str, time: &str, statement: &str) -> ProfileListItem {
    ProfileListItem {
        query_id: query_id.to_string(),
        start_time: start.to_string(),
        time: time.to_string(),
        state: "Finished".to_string(),
        statement: statement.to_string(),
    }
}

#[test]
fn parses_engine_profile_durations() {
    assert_eq!(parse_profile_time("9ms"), Some(9));
    assert_eq!(parse_profile_time("1s234ms"), Some(1_234));
    assert_eq!(parse_profile_time("2m3s"), Some(123_000));
    assert_eq!(parse_profile_time("1m"), Some(60_000));
    assert_eq!(parse_profile_time("2h"), Some(7_200_000));
    assert_eq!(parse_profile_time(" 16ms "), Some(16));
}

#[test]
fn rejects_unparseable_durations_instead_of_guessing() {
    assert_eq!(parse_profile_time(""), None);
    assert_eq!(parse_profile_time("16"), None); // 缺单位
    assert_eq!(parse_profile_time("ms"), None); // 缺数字
    assert_eq!(parse_profile_time("1x"), None); // 未知单位
    assert_eq!(parse_profile_time("1s!!"), None); // 非法字符
}

/// 复测只聚合同一指纹的执行，并把无法解析耗时的记录排除。
#[test]
fn groups_only_same_fingerprint_runs() {
    let profiles = vec![
        item("q-new", "2026-09-21 16:10:00", "8ms", "SELECT   count(*) FROM t WHERE id = 1"),
        item("q-other", "2026-09-21 16:05:00", "5ms", "SELECT * FROM other_table"),
        item("q-base", "2026-09-21 16:00:00", "1s200ms", "SELECT count(*)   FROM t WHERE id = 1"),
        item("q-bad", "2026-09-21 15:59:00", "unknown", "SELECT count(*) FROM t WHERE id = 1"),
    ];

    let baseline_sql = "SELECT count(*) FROM t WHERE id = 1";
    let (fingerprint, runs) = build_retest_runs(&profiles, "q-base", baseline_sql);

    assert!(fingerprint.starts_with("SELECT count(*) FROM t WHERE id = 1"));
    // q-other 指纹不同被排除；q-bad 耗时无法解析被排除
    assert_eq!(runs.len(), 2);
    // 时间升序，便于展示处置前后
    assert_eq!(runs[0].query_id, "q-base");
    assert!(runs[0].is_baseline);
    assert_eq!(runs[0].time_ms, 1_200);
    assert_eq!(runs[1].query_id, "q-new");
    assert!(!runs[1].is_baseline);
    assert_eq!(runs[1].time_ms, 8);
}

/// 审计表的 stmt 保留换行，预筛片段必须是单个标识符而不是多词片段。
#[test]
fn like_hint_takes_a_single_identifier_token() {
    let sql = "select dayno, imei, log_map['duplexInitRecordId']\n  from t\n  where id = 1";
    let hint = like_hint(sql).expect("hint");
    assert_eq!(hint, "duplexInitRecordId");
    assert!(!hint.contains(' '));
    assert!(!hint.contains('\n'));
}

#[test]
fn like_hint_escapes_like_wildcards_and_returns_none_without_token() {
    assert_eq!(like_hint("select a_bcdefgh from t").unwrap(), "a\\_bcdefgh");
    assert_eq!(like_hint("select 1"), None);
    assert_eq!(like_hint(""), None);
}

/// 两种来源的时间戳格式不同（审计无时区后缀），需归一化后才能正确排序。
#[test]
fn normalizes_timestamps_from_both_sources() {
    assert_eq!(normalize_time("2026-09-21 15:55:07 (+08:00)"), "2026-09-21 15:55:07");
    assert_eq!(normalize_time(" 2026-09-21 16:10:00 "), "2026-09-21 16:10:00");
}

/// 合并以审计为主：重复 query_id 保留审计那条，顺序按时间升序。
#[test]
fn merges_audit_and_profile_runs_without_duplicates() {
    let run = |id: &str, start: &str, ms: u64, state: Option<&str>| ProfileRetestRun {
        query_id: id.to_string(),
        start_time: start.to_string(),
        time_ms: ms,
        state: state.map(ToOwned::to_owned),
        is_baseline: false,
    };

    let audit = vec![
        run("q-audit", "2026-09-21 16:00:00", 900, None),
        run("q-both", "2026-09-21 16:05:00", 500, None),
    ];
    let profile = vec![
        run("q-both", "2026-09-21 16:05:00", 500, Some("Finished")),
        run("q-profile", "2026-09-21 15:00:00", 1_200, Some("Finished")),
    ];

    let merged = merge_runs(audit, profile);

    assert_eq!(merged.len(), 3);
    // 时间升序
    assert_eq!(merged[0].query_id, "q-profile");
    assert_eq!(merged[1].query_id, "q-audit");
    assert_eq!(merged[2].query_id, "q-both");
    // 重复项保留审计来源（无状态），不混入 profile 的状态
    assert_eq!(merged[2].state, None);
}

/// 高频集群的窗口期回退：profile 原文的 `Query:` 段是最后一条稳定来源。
#[test]
fn extracts_query_from_profile_text() {
    let content = "Query:\n            SELECT a\n              FROM t\n\nQuery ID: abc-123\n";
    let sql = extract_query_sql(content).expect("sql");
    assert!(sql.starts_with("SELECT a"));
    assert!(sql.contains("FROM t"));
    assert!(!sql.contains("Query ID"));

    // 没有 Query 段时返回 None，不猜测
    assert_eq!(extract_query_sql("Summary:\n total: 1\n"), None);
    assert_eq!(extract_query_sql("Query:\n\nSummary:\n"), None);
}
