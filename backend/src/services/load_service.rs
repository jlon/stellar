use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration as ChronoDuration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::models::{
    ClusterType, DorisLoadFailureDetails, LoadAction, LoadFailureCause, LoadJob, LoadListResponse,
    LoadQueryParams, LoadStage, LoadSummary, RoutineLoadDetails, RoutineLoadTask,
};
use crate::services::mysql_client::MySQLClient;
use crate::utils::{ApiError, ApiResult};

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 500;
const MAX_FETCH_LIMIT: u32 = MAX_LIMIT + 1;
const MAX_FILTER_LENGTH: usize = 200;
const MAX_DATABASES_IN_FALLBACK: usize = 50;

#[derive(Debug, Deserialize, Serialize)]
struct LoadCursor {
    create_time: String,
    job_id: i64,
}

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
    let fetch_limit = limit + 1;
    tracing::debug!(engine = %cluster_type.display_name(), limit, "listing load jobs");

    let primary_sql = match cluster_type {
        ClusterType::StarRocks => build_starrocks_load_query(params, fetch_limit)?,
        ClusterType::Doris => build_information_schema_query(params, fetch_limit)?,
    };
    match client.query_raw(&primary_sql).await {
        Ok((columns, rows)) => {
            let items = rows
                .iter()
                .map(|row| parse_load_job(&columns, row, None))
                .collect::<Vec<_>>();
            let source = match cluster_type {
                ClusterType::StarRocks => "information_schema.loads + _statistics_.loads_history",
                ClusterType::Doris => "information_schema.loads",
            };
            Ok(build_response(items, limit, source, true))
        },
        Err(information_schema_error) => {
            if cluster_type == ClusterType::StarRocks {
                tracing::debug!(
                    error = %information_schema_error,
                    "loads_history unavailable, retrying information_schema.loads"
                );
                let information_schema_sql = build_information_schema_query(params, fetch_limit)?;
                if let Ok((columns, rows)) = client.query_raw(&information_schema_sql).await {
                    let items = rows
                        .iter()
                        .map(|row| parse_load_job(&columns, row, None))
                        .collect::<Vec<_>>();
                    return Ok(build_response(items, limit, "information_schema.loads", true));
                }
            }
            if params
                .cursor
                .as_deref()
                .is_some_and(|cursor| !cursor.trim().is_empty())
            {
                return Err(information_schema_error);
            }
            tracing::debug!(
                engine = %cluster_type.display_name(),
                error = %information_schema_error,
                "information_schema.loads unavailable, falling back to SHOW LOAD"
            );
            match list_from_show_load(client, params, limit).await {
                Ok(items) => Ok(build_response(items, limit, "SHOW LOAD", false)),
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

/// 生成 StarRocks 运行视图和持久历史的合并查询。
///
/// `_statistics_.loads_history` 由运行视图同步而来，二者使用 `UNION` 去重，
/// 以免同步窗口内的已完成任务重复出现在列表中。
pub(crate) fn build_starrocks_load_query(
    params: &LoadQueryParams,
    limit: u32,
) -> ApiResult<String> {
    let limit = limit.clamp(1, MAX_FETCH_LIMIT);
    let cursor = decode_load_cursor(params.cursor.as_deref())?;
    let current = build_load_source_query(params, "information_schema.loads", cursor.as_ref())?;
    let history = build_load_source_query(params, "`_statistics_`.loads_history", cursor.as_ref())?;
    Ok(format!("{current} UNION {history} ORDER BY create_time DESC, job_id DESC LIMIT {limit}"))
}

/// 生成 information_schema.loads 查询，所有过滤值都作为 SQL 字符串字面量处理。
pub(crate) fn build_information_schema_query(
    params: &LoadQueryParams,
    limit: u32,
) -> ApiResult<String> {
    let limit = limit.clamp(1, MAX_FETCH_LIMIT);
    let cursor = decode_load_cursor(params.cursor.as_deref())?;
    let mut sql = build_load_source_query(params, "information_schema.loads", cursor.as_ref())?;
    sql.push_str(" ORDER BY create_time DESC, job_id DESC LIMIT ");
    sql.push_str(&limit.to_string());
    Ok(sql)
}

fn build_load_source_query(
    params: &LoadQueryParams,
    source: &str,
    cursor: Option<&LoadCursor>,
) -> ApiResult<String> {
    let mut sql = format!(
        "SELECT ID AS job_id, LABEL AS label, PROFILE_ID AS profile_id, \
         DB_NAME AS db_name, TABLE_NAME AS table_name, `USER` AS user_name, \
         WAREHOUSE AS warehouse, STATE AS state, PROGRESS AS progress, TYPE AS load_type, \
         PRIORITY AS priority, SCAN_ROWS AS scan_rows, SCAN_BYTES AS scan_bytes, \
         FILTERED_ROWS AS filtered_rows, UNSELECTED_ROWS AS unselected_rows, \
         SINK_ROWS AS sink_rows, CREATE_TIME AS create_time, LOAD_START_TIME AS load_start_time, \
         LOAD_COMMIT_TIME AS load_commit_time, LOAD_FINISH_TIME AS load_finish_time, \
         ERROR_MSG AS error_msg, TRACKING_SQL AS tracking_sql, \
         REJECTED_RECORD_PATH AS rejected_record_path, RUNTIME_DETAILS AS runtime_details, \
         PROPERTIES AS properties FROM {source} WHERE 1 = 1",
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
    if let Some(job_id) = params.job_id.as_deref() {
        sql.push_str(" AND ID = ");
        sql.push_str(&validated_job_id(job_id)?);
    } else if let Some(search) = validated_filter(params.search.as_deref(), "search")? {
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
    if let Some(cursor) = cursor {
        let create_time = sql_string(&cursor.create_time);
        sql.push_str(" AND (CREATE_TIME < ");
        sql.push_str(&create_time);
        sql.push_str(" OR (CREATE_TIME = ");
        sql.push_str(&create_time);
        sql.push_str(" AND ID < ");
        sql.push_str(&cursor.job_id.to_string());
        sql.push_str("))");
    }

    Ok(sql)
}

fn build_response(
    mut items: Vec<LoadJob>,
    limit: u32,
    source: &str,
    supports_cursor: bool,
) -> LoadListResponse {
    sort_and_deduplicate_loads(&mut items);
    let has_more = supports_cursor && items.len() > limit as usize;
    items.truncate(limit as usize);
    let summary = summarize(&items);
    let next_cursor = has_more
        .then(|| {
            items.last().and_then(|job| {
                encode_load_cursor(job.create_time.as_deref()?, job.job_id.as_deref()?)
            })
        })
        .flatten();
    LoadListResponse {
        total: items.len(),
        has_more: next_cursor.is_some(),
        next_cursor,
        items,
        source: source.to_string(),
        summary,
    }
}

fn sort_and_deduplicate_loads(items: &mut Vec<LoadJob>) {
    items.sort_by(|left, right| {
        right
            .create_time
            .cmp(&left.create_time)
            .then_with(|| numeric_job_id(right).cmp(&numeric_job_id(left)))
    });

    let mut job_ids = HashSet::new();
    items.retain(|job| match &job.job_id {
        Some(job_id) => job_ids.insert(job_id.clone()),
        None => true,
    });
}

fn numeric_job_id(job: &LoadJob) -> i64 {
    job.job_id
        .as_deref()
        .and_then(|job_id| job_id.parse().ok())
        .unwrap_or(i64::MIN)
}

pub(crate) fn encode_load_cursor(create_time: &str, job_id: &str) -> Option<String> {
    let job_id = job_id.parse().ok()?;
    let payload =
        serde_json::to_vec(&LoadCursor { create_time: create_time.to_string(), job_id }).ok()?;
    Some(format!("v1:{}", URL_SAFE_NO_PAD.encode(payload)))
}

fn decode_load_cursor(value: Option<&str>) -> ApiResult<Option<LoadCursor>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_FILTER_LENGTH * 3 {
        return Err(ApiError::invalid_data("cursor 无效"));
    }
    let encoded = value
        .strip_prefix("v1:")
        .ok_or_else(|| ApiError::invalid_data("cursor 无效"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ApiError::invalid_data("cursor 无效"))?;
    let cursor: LoadCursor =
        serde_json::from_slice(&payload).map_err(|_| ApiError::invalid_data("cursor 无效"))?;
    if parse_timestamp(&cursor.create_time).is_none() {
        return Err(ApiError::invalid_data("cursor 无效"));
    }
    Ok(Some(cursor))
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
    if let Some(job_id) = params.job_id.as_deref() {
        if job.job_id.as_deref() != Some(job_id) {
            return false;
        }
    }
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
        routine_load: None,
        actions: Vec::new(),
        doris_failure: None,
        stage_timeline,
    }
}

/// 仅为选中的失败 Doris 作业补充 `SHOW LOAD` 中的原始诊断字段。
///
/// Doris `SHOW LOAD` 只支持 `LABEL` 和 `STATE` 谓词，不能以未证实的 `ID` 谓词查询。
/// 因此先在已知数据库内精确匹配 Label，再在结果中严格匹配已选 JobId；不会遍历数据库，
/// 也不会暴露同 Label 的其他作业。
pub async fn load_doris_failure_details(
    client: &MySQLClient,
    job: &LoadJob,
) -> ApiResult<Option<DorisLoadFailureDetails>> {
    if !is_failed_state(&job.state) {
        return Ok(None);
    }
    let (Some(database), Some(label), Some(job_id)) =
        (job.database.as_deref(), job.label.as_deref(), job.job_id.as_deref())
    else {
        return Ok(None);
    };
    let sql = build_doris_load_failure_query(database, label)?;
    let (columns, rows) = client.query_raw(&sql).await?;
    Ok(find_doris_load_failure_details(&columns, &rows, job_id))
}

pub(crate) fn build_doris_load_failure_query(database: &str, label: &str) -> ApiResult<String> {
    let label = validated_filter(Some(label), "label")?
        .ok_or_else(|| ApiError::invalid_data("label 无效"))?;
    Ok(format!(
        "SHOW LOAD FROM {} WHERE LABEL = {}",
        quote_identifier(database)?,
        sql_string(&label),
    ))
}

pub(crate) fn find_doris_load_failure_details(
    columns: &[String],
    rows: &[Vec<String>],
    job_id: &str,
) -> Option<DorisLoadFailureDetails> {
    rows.iter().find_map(|row| {
        let values = row_map(columns, row);
        (value(&values, &["job_id", "jobid", "id"]).as_deref() == Some(job_id))
            .then(|| {
                let details = DorisLoadFailureDetails {
                    url: value(&values, &["url"]),
                    error_msg: value(&values, &["error_msg", "errormsg", "errmsg"]),
                    job_details: value(&values, &["job_details", "jobdetails"]),
                };
                (details.url.is_some()
                    || details.error_msg.is_some()
                    || details.job_details.is_some())
                .then_some(details)
            })
            .flatten()
    })
}

/// 按需查询 Routine Load 父作业的真实消费位点和当前子任务。
///
/// `information_schema.loads` 展示的是执行任务，父作业名只在其 `PROPERTIES.job_name`
/// 中提供；没有该字段时不猜测标签格式，也不查询错误的作业。
pub async fn load_routine_details(
    client: &MySQLClient,
    job: &LoadJob,
) -> ApiResult<Option<RoutineLoadDetails>> {
    let Some(database) = job.database.as_deref() else {
        return Ok(None);
    };
    let Some(name) = routine_load_name(job) else {
        return Ok(None);
    };
    let job_sql = format!(
        "SHOW ALL ROUTINE LOAD FOR {}.{}",
        quote_identifier(database)?,
        quote_identifier(&name)?,
    );
    let (columns, rows) = client.query_raw(&job_sql).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let values = row_map(&columns, row);
    let task_sql = format!(
        "SHOW ROUTINE LOAD TASK FROM {} WHERE JobName = {}",
        quote_identifier(database)?,
        sql_string(&name),
    );
    let tasks = match client.query_raw(&task_sql).await {
        Ok((columns, rows)) => rows
            .iter()
            .map(|row| parse_routine_load_task(&columns, row))
            .collect(),
        Err(error) => {
            tracing::debug!(error = %error, "routine load tasks unavailable");
            Vec::new()
        },
    };

    Ok(Some(RoutineLoadDetails {
        state: value(&values, &["state"]),
        current_task_num: number(&values, &["current_task_num", "currenttasknum"])
            .and_then(|value| u32::try_from(value).ok()),
        statistics: value(&values, &["statistics", "statistic"]),
        progress: value(&values, &["progress"]),
        timestamp_progress: value(&values, &["timestamp_progress", "timestampprogress"]),
        latest_source_position: value(&values, &["latest_source_position", "latestsourceposition"]),
        offset_lag: value(&values, &["offset_lag", "offsetlag", "lag"]),
        reason_of_state_changed: value(
            &values,
            &["reasons_of_state_changed", "reason_of_state_changed", "reasonofstatechanged"],
        ),
        error_log_urls: value(&values, &["error_log_urls", "errorlogurls"]),
        tracking_sql: value(&values, &["tracking_sql", "trackingsql"]),
        other_msg: value(&values, &["other_msg", "othermsg"]),
        tasks,
    }))
}

pub(crate) fn routine_load_name(job: &LoadJob) -> Option<String> {
    let properties = serde_json::from_str::<serde_json::Value>(job.properties.as_deref()?).ok()?;
    ["job_name", "jobName"]
        .iter()
        .find_map(|key| properties.get(key)?.as_str().map(ToOwned::to_owned))
        .filter(|name| !name.trim().is_empty())
}

pub(crate) fn parse_routine_load_task(columns: &[String], row: &[String]) -> RoutineLoadTask {
    let values = row_map(columns, row);
    RoutineLoadTask {
        task_id: value(&values, &["task_id", "taskid"]),
        txn_id: value(&values, &["txn_id", "txnid"]),
        txn_status: value(&values, &["txn_status", "txnstatus"]),
        create_time: value(&values, &["create_time", "createtime"]),
        last_scheduled_time: value(&values, &["last_scheduled_time", "lastscheduledtime"]),
        execute_start_time: value(&values, &["execute_start_time", "executestarttime"]),
        be_id: value(&values, &["be_id", "beid"]),
        data_source_properties: value(&values, &["data_source_properties", "datasourceproperties"]),
        message: value(&values, &["message"]),
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

    let (code, label, suggestion, steps) = if contains_any(
        &normalized,
        &["timeout", "timed out", "deadline", "超时"],
    ) {
        (
            "Timeout",
            "超时",
            "检查 FE/计算节点负载、网络延迟和导入批次大小",
            vec![
                "在集群概览与节点管理核对 FE/BE 当时的负载、心跳与重启记录",
                "减小单次提交的文件体积，或拆分为多个 Label 分批导入",
                "确认不是瞬时抖动后，再提高导入超时：PROPERTIES(\"timeout\" = \"3600\")",
            ],
        )
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
        (
            "ThresholdExceeded",
            "过滤阈值",
            "检查数据质量和 max_filter_ratio 配置",
            vec![
                "先用 rejected_record_path 抽样被拒记录，区分脏数据与格式不匹配",
                "确认数据质量可接受后，再放宽容忍度：PROPERTIES(\"max_filter_ratio\" = \"0.1\")（默认 0）",
                "若整行被拒源于分隔符或列顺序，先修正文件格式，不要用阈值掩盖",
            ],
        )
    } else if contains_any(
        &normalized,
        &[
            "no rows were imported",
            "no rows imported",
            "all rows were filtered",
            "no valid rows",
            "empty file",
            "no data",
            "无数据",
            "没有数据",
            "空文件",
        ],
    ) {
        (
            "NoRowsLoaded",
            "无有效数据",
            "确认源文件或上游非空，且列与过滤条件不会滤掉全部行",
            vec![
                "确认源文件或上游 Topic 非空，且字段数与目标表列数一致",
                "核对列分隔符与列顺序；JSON 需为每行一个对象（NDJSON）",
                "检查过滤条件是否把所有行都滤掉，分区导入还要确认分区键取值存在",
                "确认数据可接受后，用新的 Label 重新提交（同一 Label 不能重复使用）",
            ],
        )
    } else if contains_any(
        &normalized,
        &["permission", "privilege", "access denied", "unauthorized", "权限", "禁止"],
    ) {
        (
            "PermissionDenied",
            "权限不足",
            "检查导入用户对目标库表和对象存储的权限",
            vec![
                "确认导入账号对目标库表具备 INSERT 权限",
                "Broker/Routine 还需要目标存储或 Kafka 侧的认证已配置在集群节点上",
                "确认没有使用已被回收或改密的账号重新提交",
            ],
        )
    } else if contains_any(
        &normalized,
        &["not found", "does not exist", "unknown table", "unknown database", "不存在", "找不到"],
    ) {
        (
            "TargetMissing",
            "目标不存在",
            "确认目标 Catalog、数据库、表和分区仍然存在",
            vec![
                "在查询管理的对象树确认目标数据库、表与分区仍然存在",
                "核对语句中的 Catalog、库名与表名大小写是否一致",
                "目标表被重命名或删除后需要重新指定目标表再提交",
            ],
        )
    } else if contains_any(
        &normalized,
        &["parse", "format", "delimiter", "json", "csv", "type mismatch", "格式", "解析"],
    ) {
        (
            "FormatError",
            "格式错误",
            "检查文件格式、列顺序、分隔符和字段类型",
            vec![
                "核对列顺序、列数与分隔符，建议先用单行样本文件验证",
                "JSON 需要每行一个对象（NDJSON），不接受数组包裹",
                "检查字段类型能否转换：严格模式下类型不符会导致整行被拒",
            ],
        )
    } else if contains_any(
        &normalized,
        &["out of memory", "oom", "resource", "no available", "memory limit", "资源", "内存"],
    ) {
        (
            "ResourceExhausted",
            "资源不足",
            "检查资源组、内存上限和并发导入任务数量",
            vec![
                "在资源组管理查看该用户或资源组的内存上限与当前占用",
                "降低同一时间的并发导入数量，或与查询高峰错峰执行",
                "单批次过大的导入应拆分；确认资源确实不足时再调整资源组配额",
            ],
        )
    } else {
        (
            "Unknown",
            "未知原因",
            "查看完整错误信息和 Profile，必要时重试并联系管理员",
            vec![
                "展开完整错误信息与 PROFILE_ID，按时间点核对 FE/BE 日志",
                "间隔后重试一次，排除瞬时故障",
                "反复出现时记录 Label、时间与 PROFILE_ID 以便继续排查",
            ],
        )
    };

    Some(LoadFailureCause {
        code: code.to_string(),
        label: label.to_string(),
        suggestion: suggestion.to_string(),
        steps: steps.into_iter().map(ToOwned::to_owned).collect(),
    })
}

/// 按引擎返回的父作业状态给出可执行处置动作。
///
/// 只有 Routine Load 的父作业状态可判定时才给动作：终态（已停止/已取消）与
/// 未知状态一律不给，避免按猜测下发语句。
pub(crate) fn available_actions(job: &LoadJob) -> ApiResult<Vec<LoadAction>> {
    let Some(name) = routine_load_name(job) else {
        return Ok(Vec::new());
    };
    let Some(database) = job.database.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(state) = job
        .routine_load
        .as_ref()
        .and_then(|details| details.state.as_deref())
    else {
        return Ok(Vec::new());
    };

    let database = quote_identifier(database)?;
    let name = quote_identifier(&name)?;
    let upper = state.to_ascii_uppercase();
    let actions = match upper.as_str() {
        "PAUSED" => vec![LoadAction {
            action: "resume_routine".to_string(),
            label: "恢复作业".to_string(),
            description: "父作业当前为 PAUSED，恢复后继续消费上游消息".to_string(),
            statement: format!("RESUME ROUTINE LOAD FOR {database}.{name}"),
        }],
        "RUNNING" | "NEED_SCHEDULE" => vec![LoadAction {
            action: "pause_routine".to_string(),
            label: "暂停作业".to_string(),
            description: "先暂停作业，修复上游或参数后再恢复".to_string(),
            statement: format!("PAUSE ROUTINE LOAD FOR {database}.{name}"),
        }],
        _ => Vec::new(),
    };
    Ok(actions)
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

fn validated_job_id(value: &str) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() || value.parse::<u64>().is_err() {
        return Err(ApiError::invalid_data("job_id 无效"));
    }
    Ok(value.to_string())
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
