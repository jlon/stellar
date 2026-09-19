use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use std::collections::HashMap;

use crate::models::{
    ClusterType, LoadFailureCause, LoadJob, LoadListResponse, LoadQueryParams, LoadStage,
    LoadSummary,
};
use crate::services::mysql_client::MySQLClient;
use crate::utils::{ApiError, ApiResult};

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
const MAX_FILTER_LENGTH: usize = 200;
const MAX_DATABASES_IN_FALLBACK: usize = 50;

/// 查询统一导入任务列表。
///
/// 首选 `information_schema.loads`，兼容旧版本或 Doris 不提供该视图时，
/// 回退到按数据库执行 `SHOW LOAD`。所有字段都来自引擎返回值，缺失字段保持为空。
pub async fn list_loads(
    client: &MySQLClient,
    cluster_type: ClusterType,
    params: &LoadQueryParams,
) -> ApiResult<LoadListResponse> {
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    tracing::debug!(engine = %cluster_type.display_name(), limit, "listing load jobs");

    let information_schema_sql = build_information_schema_query(params, limit)?;
    match client.query_raw(&information_schema_sql).await {
        Ok((columns, rows)) => {
            let items = rows
                .iter()
                .map(|row| parse_load_job(&columns, row, None))
                .collect::<Vec<_>>();
            Ok(build_response(items, limit, "information_schema.loads"))
        },
        Err(information_schema_error) => {
            tracing::debug!(
                engine = %cluster_type.display_name(),
                error = %information_schema_error,
                "information_schema.loads unavailable, falling back to SHOW LOAD"
            );
            match list_from_show_load(client, params, limit).await {
                Ok(items) => Ok(build_response(items, limit, "SHOW LOAD")),
                Err(fallback_error) => {
                    tracing::warn!(
                        error = %fallback_error,
                        "both information_schema.loads and SHOW LOAD failed"
                    );
                    Err(information_schema_error)
                },
            }
        },
    }
}

/// 生成 information_schema.loads 查询，所有过滤值都作为 SQL 字符串字面量处理。
pub(crate) fn build_information_schema_query(
    params: &LoadQueryParams,
    limit: u32,
) -> ApiResult<String> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let mut sql = String::from(
        "SELECT ID AS job_id, LABEL AS label, PROFILE_ID AS profile_id, \
         DB_NAME AS db_name, TABLE_NAME AS table_name, `USER` AS user_name, \
         WAREHOUSE AS warehouse, STATE AS state, PROGRESS AS progress, TYPE AS load_type, \
         PRIORITY AS priority, SCAN_ROWS AS scan_rows, SCAN_BYTES AS scan_bytes, \
         FILTERED_ROWS AS filtered_rows, UNSELECTED_ROWS AS unselected_rows, \
         SINK_ROWS AS sink_rows, CREATE_TIME AS create_time, LOAD_START_TIME AS load_start_time, \
         LOAD_COMMIT_TIME AS load_commit_time, LOAD_FINISH_TIME AS load_finish_time, \
         ERROR_MSG AS error_msg, TRACKING_SQL AS tracking_sql, \
         REJECTED_RECORD_PATH AS rejected_record_path, RUNTIME_DETAILS AS runtime_details, \
         PROPERTIES AS properties FROM information_schema.loads WHERE 1 = 1",
    );

    if let Some(db) = validated_filter(params.db.as_deref(), "db")? {
        sql.push_str(" AND DB_NAME = ");
        sql.push_str(&sql_string(&db));
    }
    if let Some(load_type) = validated_filter(params.load_type.as_deref(), "type")? {
        sql.push_str(" AND UPPER(TYPE) = UPPER(");
        sql.push_str(&sql_string(&load_type));
        sql.push(')');
    }
    if let Some(state) = validated_filter(params.state.as_deref(), "state")? {
        sql.push_str(" AND UPPER(STATE) = UPPER(");
        sql.push_str(&sql_string(&state));
        sql.push(')');
    }
    if let Some(search) = validated_filter(params.search.as_deref(), "search")? {
        let pattern = sql_string(&format!("%{}%", search));
        sql.push_str(" AND (CAST(ID AS CHAR) LIKE ");
        sql.push_str(&pattern);
        sql.push_str(" OR LABEL LIKE ");
        sql.push_str(&pattern);
        sql.push_str(" OR DB_NAME LIKE ");
        sql.push_str(&pattern);
        sql.push_str(" OR TABLE_NAME LIKE ");
        sql.push_str(&pattern);
        sql.push_str(" OR ERROR_MSG LIKE ");
        sql.push_str(&pattern);
        sql.push(')');
    }
    if let Some(start) = range_start(params.range.as_deref())? {
        sql.push_str(" AND CREATE_TIME >= ");
        sql.push_str(&sql_string(&start));
    }

    sql.push_str(" ORDER BY CREATE_TIME DESC LIMIT ");
    sql.push_str(&limit.to_string());
    Ok(sql)
}

fn build_response(mut items: Vec<LoadJob>, limit: u32, source: &str) -> LoadListResponse {
    let has_more = items.len() >= limit as usize;
    items.truncate(limit as usize);
    let summary = summarize(&items);
    LoadListResponse { total: items.len(), items, has_more, source: source.to_string(), summary }
}

async fn list_from_show_load(
    client: &MySQLClient,
    params: &LoadQueryParams,
    limit: u32,
) -> ApiResult<Vec<LoadJob>> {
    let databases = if let Some(db) = validated_filter(params.db.as_deref(), "db")? {
        vec![db]
    } else {
        let (_, rows) = client.query_raw("SHOW DATABASES").await?;
        // ponytail: fallback scans at most 50 databases; add server-side history/cursor before widening this fan-out.
        rows.iter()
            .filter_map(|row| row.first().and_then(|value| clean_value(value)))
            .filter(|db| !is_system_database(db))
            .take(MAX_DATABASES_IN_FALLBACK)
            .collect::<Vec<_>>()
    };

    let range_start = range_start(params.range.as_deref())?;
    let mut items = Vec::new();
    let mut successful_queries = 0usize;
    let mut last_error = None;

    for db in databases {
        let sql = format!("SHOW LOAD FROM {} LIMIT {}", quote_identifier(&db)?, limit);
        match client.query_raw(&sql).await {
            Ok((columns, rows)) => {
                successful_queries += 1;
                items.extend(
                    rows.iter()
                        .map(|row| parse_load_job(&columns, row, Some(&db)))
                        .filter(|job| matches_filters(job, params, range_start.as_deref())),
                );
            },
            Err(error) => last_error = Some(error),
        }
    }

    if successful_queries == 0 {
        return Err(last_error.unwrap_or_else(|| ApiError::not_found("没有可查询的数据库")));
    }

    items.sort_by(|left, right| right.create_time.cmp(&left.create_time));
    items.truncate(limit as usize);
    Ok(items)
}

fn matches_filters(job: &LoadJob, params: &LoadQueryParams, range_start: Option<&str>) -> bool {
    if let Some(db) = params
        .db
        .as_deref()
        .map(str::trim)
        .filter(|db| !db.is_empty())
    {
        if job.database.as_deref() != Some(db) {
            return false;
        }
    }
    if let Some(load_type) = params
        .load_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !job.load_type.eq_ignore_ascii_case(load_type) {
            return false;
        }
    }
    if let Some(state) = params
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !job.state.eq_ignore_ascii_case(state) {
            return false;
        }
    }
    if let Some(start) = range_start {
        if job
            .create_time
            .as_deref()
            .is_some_and(|created| created < start)
        {
            return false;
        }
    }
    if let Some(search) = params
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let search = search.to_ascii_lowercase();
        let haystack = [
            job.job_id.as_deref().unwrap_or_default(),
            job.label.as_deref().unwrap_or_default(),
            job.database.as_deref().unwrap_or_default(),
            job.table_name.as_deref().unwrap_or_default(),
            job.error_msg.as_deref().unwrap_or_default(),
        ]
        .join(" ")
        .to_ascii_lowercase();
        if !haystack.contains(&search) {
            return false;
        }
    }
    true
}

pub(crate) fn parse_load_job(
    columns: &[String],
    row: &[String],
    database_hint: Option<&str>,
) -> LoadJob {
    let values = row_map(columns, row);
    let database = value(&values, &["db_name", "database", "database_name"])
        .or_else(|| database_hint.map(ToOwned::to_owned));
    let state = value(&values, &["state"]).unwrap_or_else(|| "UNKNOWN".to_string());
    let error_msg = value(&values, &["error_msg", "errormsg", "errmsg", "error"]);
    let load_start_time =
        value(&values, &["load_start_time", "loadstarttime", "etl_start_time", "etlstarttime"]);
    let load_commit_time = value(&values, &["load_commit_time", "commit_time"]);
    let load_finish_time =
        value(&values, &["load_finish_time", "loadfinishtime", "etl_finish_time", "etlfinishtime"]);
    let create_time = value(&values, &["create_time", "createtime"]);
    let load_type = value(&values, &["load_type", "type"])
        .map(|value| normalize_load_type(&value))
        .unwrap_or_else(|| "UNKNOWN".to_string());

    let properties = value(&values, &["properties", "task_info", "taskinfo"]);
    let runtime_details =
        value(&values, &["runtime_details", "runtimedetails", "job_details", "jobdetails"]);
    let etl_info = value(&values, &["etl_info", "etlinfo"]);
    let scan_rows = number(&values, &["scan_rows", "scanrows"])
        .or_else(|| json_number(runtime_details.as_deref(), "ScannedRows"));
    let scan_bytes = number(&values, &["scan_bytes", "scanbytes"]);
    let filtered_rows = number(&values, &["filtered_rows", "filteredrows"])
        .or_else(|| delimited_number(etl_info.as_deref(), "dpp.abnorm.ALL"));
    let unselected_rows = number(&values, &["unselected_rows", "unselectedrows"])
        .or_else(|| delimited_number(etl_info.as_deref(), "unselected.rows"));
    let sink_rows =
        number(&values, &["sink_rows", "sinkrows", "load_rows", "loadrows", "loaded_rows"])
            .or_else(|| delimited_number(etl_info.as_deref(), "dpp.norm.ALL"));

    let stage_timeline = if load_type == "ROUTINE_LOAD" {
        Vec::new()
    } else {
        build_stage_timeline(
            create_time.as_deref(),
            load_start_time.as_deref(),
            load_commit_time.as_deref(),
            load_finish_time.as_deref(),
            &state,
        )
    };

    LoadJob {
        job_id: value(&values, &["job_id", "id", "jobid"]),
        label: value(&values, &["label"]),
        profile_id: value(&values, &["profile_id", "profileid"]),
        database,
        table_name: value(&values, &["table_name", "tablename", "table"]),
        user: value(&values, &["user_name", "user", "username"]),
        warehouse: value(&values, &["warehouse", "warehouse_name"]),
        state,
        progress: value(&values, &["progress"]),
        load_type,
        priority: value(&values, &["priority"]),
        scan_rows,
        scan_bytes,
        filtered_rows,
        unselected_rows,
        sink_rows,
        create_time,
        load_start_time,
        load_commit_time,
        load_finish_time,
        failure_cause: error_msg.as_deref().and_then(classify_failure),
        error_msg,
        tracking_sql: value(&values, &["tracking_sql", "trackingsql"]),
        rejected_record_path: value(&values, &["rejected_record_path", "rejectedrecordpath"]),
        runtime_details,
        properties,
        stage_timeline,
    }
}

fn normalize_load_type(value: &str) -> String {
    match value.trim().to_ascii_uppercase().as_str() {
        "BROKER" | "BROKER_LOAD" => "BROKER_LOAD".to_string(),
        "SPARK" | "SPARK_LOAD" => "SPARK_LOAD".to_string(),
        "STREAM" | "STREAM_LOAD" => "STREAM_LOAD".to_string(),
        "ROUTINE" | "ROUTINE_LOAD" => "ROUTINE_LOAD".to_string(),
        "INSERT" => "INSERT".to_string(),
        other => other.to_string(),
    }
}

fn json_number(value: Option<&str>, key: &str) -> Option<u64> {
    let json = serde_json::from_str::<serde_json::Value>(value?).ok()?;
    let value = json.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.replace(',', "").parse::<u64>().ok())
}

fn delimited_number(value: Option<&str>, key: &str) -> Option<u64> {
    value?.split(';').find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case(key) {
            return None;
        }
        value.trim().replace(',', "").parse::<u64>().ok()
    })
}

fn row_map(columns: &[String], row: &[String]) -> HashMap<String, String> {
    columns
        .iter()
        .zip(row.iter())
        .map(|(column, value)| (column.trim().to_ascii_lowercase(), value.clone()))
        .collect()
}

fn value(values: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| values.get(*key).and_then(|value| clean_value(value)))
}

fn clean_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("null") || value == "-" {
        None
    } else {
        Some(value.to_string())
    }
}

fn number(values: &HashMap<String, String>, keys: &[&str]) -> Option<u64> {
    value(values, keys).and_then(|raw| raw.replace(',', "").parse::<u64>().ok())
}

pub(crate) fn build_stage_timeline(
    create_time: Option<&str>,
    load_start_time: Option<&str>,
    load_commit_time: Option<&str>,
    load_finish_time: Option<&str>,
    state: &str,
) -> Vec<LoadStage> {
    let mut stages = Vec::with_capacity(3);
    add_stage(&mut stages, "queue", "排队", create_time, load_start_time, state);
    add_stage(
        &mut stages,
        "execute",
        "执行",
        load_start_time,
        load_commit_time.or(load_finish_time),
        state,
    );
    add_stage(&mut stages, "commit", "提交", load_commit_time, load_finish_time, state);
    stages
}

fn add_stage(
    stages: &mut Vec<LoadStage>,
    key: &str,
    label: &str,
    start: Option<&str>,
    end: Option<&str>,
    state: &str,
) {
    let Some(start_value) = start else { return };
    let Some(start_time) = parse_timestamp(start_value) else { return };
    let end_time = end.and_then(parse_timestamp);
    let effective_end = end_time
        .or_else(|| if is_terminal_state(state) { None } else { Some(Utc::now().naive_utc()) });
    let Some(effective_end) = effective_end else { return };
    let duration_ms = effective_end
        .signed_duration_since(start_time)
        .num_milliseconds();
    if duration_ms <= 0 {
        return;
    }

    let is_running = end_time.is_none() && !is_terminal_state(state);
    stages.push(LoadStage {
        key: key.to_string(),
        label: label.to_string(),
        duration_ms: duration_ms as u64,
        start_time: Some(start_value.to_string()),
        end_time: end.map(ToOwned::to_owned),
        status: if is_running {
            "running".to_string()
        } else if is_failed_state(state) {
            "failed".to_string()
        } else {
            "completed".to_string()
        },
    });
}

fn parse_timestamp(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim();
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(value).map(|date| date.naive_utc()))
        .ok()
}

pub(crate) fn classify_failure(message: &str) -> Option<LoadFailureCause> {
    let normalized = message.to_ascii_lowercase();
    if normalized.trim().is_empty() {
        return None;
    }

    let (code, label, suggestion) =
        if contains_any(&normalized, &["timeout", "timed out", "deadline", "超时"]) {
            ("Timeout", "超时", "检查 FE/BE 负载、网络延迟和导入批次大小")
        } else if contains_any(
            &normalized,
            &[
                "threshold",
                "max_filter_ratio",
                "max-filter-ratio",
                "etl_quality_unsatisfied",
                "quality_unsatisfied",
                "filter ratio",
                "filtered ratio",
                "过滤比例",
            ],
        ) {
            ("ThresholdExceeded", "过滤阈值", "检查数据质量和 max_filter_ratio 配置")
        } else if contains_any(
            &normalized,
            &["permission", "privilege", "access denied", "unauthorized", "权限", "禁止"],
        ) {
            ("PermissionDenied", "权限不足", "检查导入用户对目标库表和对象存储的权限")
        } else if contains_any(
            &normalized,
            &[
                "not found",
                "does not exist",
                "unknown table",
                "unknown database",
                "不存在",
                "找不到",
            ],
        ) {
            ("TargetMissing", "目标不存在", "确认目标 Catalog、数据库、表和分区仍然存在")
        } else if contains_any(
            &normalized,
            &["parse", "format", "delimiter", "json", "csv", "type mismatch", "格式", "解析"],
        ) {
            ("FormatError", "格式错误", "检查文件格式、列顺序、分隔符和字段类型")
        } else if contains_any(
            &normalized,
            &["out of memory", "oom", "resource", "no available", "memory limit", "资源", "内存"],
        ) {
            ("ResourceExhausted", "资源不足", "检查资源组、内存上限和并发导入任务数量")
        } else {
            ("Unknown", "未知原因", "查看完整错误信息和 Profile，必要时重试并联系管理员")
        };

    Some(LoadFailureCause {
        code: code.to_string(),
        label: label.to_string(),
        suggestion: suggestion.to_string(),
    })
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn summarize(items: &[LoadJob]) -> LoadSummary {
    let mut summary = LoadSummary::default();
    for item in items {
        if is_failed_state(&item.state) {
            summary.failed += 1;
        } else if is_terminal_state(&item.state) {
            summary.finished += 1;
        } else if is_queued_state(&item.state) {
            summary.queued += 1;
        } else {
            summary.running += 1;
        }
    }
    summary
}

fn is_queued_state(state: &str) -> bool {
    matches!(state.to_ascii_lowercase().as_str(), "pending" | "queueing" | "before_load" | "queued")
}

fn is_failed_state(state: &str) -> bool {
    let state = state.to_ascii_lowercase();
    state.contains("cancel") || state.contains("fail") || state.contains("error")
}

fn is_terminal_state(state: &str) -> bool {
    let state = state.to_ascii_lowercase();
    is_failed_state(&state)
        || matches!(state.as_str(), "finished" | "committed" | "commited" | "success" | "succeed")
}

fn validated_filter(value: Option<&str>, field: &str) -> ApiResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_FILTER_LENGTH {
        return Err(ApiError::invalid_data(format!("{} 过滤条件过长", field)));
    }
    Ok(Some(value.to_string()))
}

fn range_start(range: Option<&str>) -> ApiResult<Option<String>> {
    let range = range.unwrap_or("24h").trim().to_ascii_lowercase();
    let duration = match range.as_str() {
        "" | "all" => return Ok(None),
        "24h" => ChronoDuration::hours(24),
        "7d" => ChronoDuration::days(7),
        "30d" => ChronoDuration::days(30),
        _ => return Err(ApiError::invalid_data("range 只支持 24h、7d、30d 或 all")),
    };
    Ok(Some(
        (Utc::now() - duration)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    ))
}

fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quote_identifier(value: &str) -> ApiResult<String> {
    if value.is_empty() || value.len() > MAX_FILTER_LENGTH || value.contains('\0') {
        return Err(ApiError::invalid_data("数据库名称无效"));
    }
    Ok(format!("`{}`", value.replace('`', "``")))
}

fn is_system_database(database: &str) -> bool {
    matches!(
        database.to_ascii_lowercase().as_str(),
        "information_schema" | "_statistics_" | "mysql" | "sys" | "__internal_schema"
    )
}
