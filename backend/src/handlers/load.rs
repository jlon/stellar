use axum::{
    Json,
    body::Bytes,
    extract::{Multipart, Path, Query, State},
};
use futures::stream;
use std::sync::Arc;
use std::time::Duration;
use stellar_macros::app_db;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::AppState;
use crate::models::{ClusterType, LoadJob, LoadListResponse, LoadQueryParams, StreamLoadResponse};
use crate::services::{
    MySQLClient, QueryExecutionHistoryService, cluster_timeout, create_adapter, load_service,
    query_execution_history_service::ExecutionRecord, stream_load,
};
use crate::utils::{ApiResult, get_active_cluster_for_org};

/// 查询当前组织活跃集群的导入任务。
#[utoipa::path(
    get,
    path = "/api/clusters/loads",
    params(
        ("db" = Option<String>, Query, description = "数据库名称"),
        ("type" = Option<String>, Query, description = "导入类型"),
        ("state" = Option<String>, Query, description = "任务状态"),
        ("search" = Option<String>, Query, description = "按作业 ID、Label、表名或错误信息搜索"),
        ("range" = Option<String>, Query, description = "时间范围：24h、7d、30d 或 all"),
        ("limit" = Option<u32>, Query, description = "返回条数，最多 500"),
        ("cursor" = Option<String>, Query, description = "上一页末尾位置，由响应 next_cursor 返回")
    ),
    responses(
        (status = 200, description = "导入任务列表", body = LoadListResponse),
        (status = 404, description = "没有可用的活跃集群")
    ),
    security(("bearer_auth" = [])),
    tag = "Load Management"
)]
#[app_db]
pub async fn list_loads(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Query(params): Query<LoadQueryParams>,
) -> ApiResult<Json<LoadListResponse>> {
    let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));
    let response = load_service::list_loads(&client, cluster.cluster_type, &params).await?;
    Ok(Json(response))
}

/// 查询单个导入任务的完整详情。
#[utoipa::path(
    get,
    path = "/api/clusters/loads/{job_id}",
    params(
        ("job_id" = String, Path, description = "作业 ID"),
        ("db" = Option<String>, Query, description = "数据库名称")
    ),
    responses(
        (status = 200, description = "导入任务详情", body = LoadJob),
        (status = 404, description = "导入任务不存在")
    ),
    security(("bearer_auth" = [])),
    tag = "Load Management"
)]
#[app_db]
pub async fn get_load(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(job_id): Path<String>,
    Query(mut params): Query<LoadQueryParams>,
) -> ApiResult<Json<LoadJob>> {
    let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));

    params.job_id = Some(job_id.clone());
    params.search = None;
    params.range = Some("all".to_string());
    params.limit = Some(500);
    params.cursor = None;
    let response = load_service::list_loads(&client, cluster.cluster_type, &params).await?;
    let mut job = response
        .items
        .into_iter()
        .find(|item| item.job_id.as_deref() == Some(job_id.as_str()))
        .ok_or_else(|| crate::utils::ApiError::not_found(format!("导入任务 {} 不存在", job_id)))?;
    if cluster.cluster_type == crate::models::ClusterType::StarRocks
        && job.load_type == "ROUTINE_LOAD"
    {
        match load_service::load_routine_details(&client, &job).await {
            Ok(Some(details)) => {
                if job.tracking_sql.is_none() {
                    job.tracking_sql = details.tracking_sql.clone();
                }
                job.routine_load = Some(details);
            },
            Ok(None) => {},
            Err(error) => {
                tracing::debug!(error = %error, job_id = %job_id, "routine load details unavailable");
            },
        }
    }
    if cluster.cluster_type == crate::models::ClusterType::Doris {
        match load_service::load_doris_failure_details(&client, &job).await {
            Ok(details) => job.doris_failure = details,
            Err(error) => {
                tracing::debug!(error = %error, job_id = %job_id, "Doris load failure details unavailable");
            },
        }
    }
    Ok(Json(job))
}

/// 将浏览器上传的 CSV/JSON 文件通过 Stream Load 发送到一个健康的 StarRocks BE/CN。
///
/// 不经通用 SQL 执行接口，避免文件内容或传输参数进入 SQL 历史；外部存储与 Kafka
/// 凭据也不在此接口接收。
#[utoipa::path(
    post,
    path = "/api/clusters/queries/stream-load",
    request_body(content = String, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Stream Load 引擎结果", body = StreamLoadResponse),
        (status = 400, description = "上传参数或引擎返回无效"),
        (status = 404, description = "没有可用的活跃集群")
    ),
    security(("bearer_auth" = [])),
    tag = "Load Management"
)]
#[app_db]
pub async fn stream_load(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    mut multipart: Multipart,
) -> ApiResult<Json<StreamLoadResponse>> {
    let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
    if cluster.cluster_type != ClusterType::StarRocks {
        return Err(crate::utils::ApiError::validation_error(
            "当前仅支持向 StarRocks 集群发起 Stream Load",
        ));
    }

    let mut database = None;
    let mut table = None;
    let mut format = None;
    let mut label = None;
    let mut column_separator = None;
    let mut staged_file = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                if let Some((_, path)) = staged_file.take() {
                    let _ = tokio::fs::remove_file(path).await;
                }
                return Err(crate::utils::ApiError::invalid_data(format!(
                    "读取上传内容失败: {error}"
                )));
            },
        };
        let field = field;
        if staged_file.is_some() {
            if let Some((_, path)) = staged_file.take() {
                let _ = tokio::fs::remove_file(path).await;
            }
            return Err(crate::utils::ApiError::validation_error("文件字段必须位于最后"));
        }
        let name = field
            .name()
            .ok_or_else(|| crate::utils::ApiError::validation_error("上传字段缺少名称"))?
            .to_string();

        match name.as_str() {
            "database" => {
                set_form_value(&mut database, read_field_text(field, 256).await?, "数据库")?
            },
            "table" => set_form_value(&mut table, read_field_text(field, 256).await?, "目标表")?,
            "format" => set_form_value(&mut format, read_field_text(field, 32).await?, "文件格式")?,
            "label" => set_form_value(&mut label, read_field_text(field, 128).await?, "Label")?,
            "column_separator" => {
                set_form_value(&mut column_separator, read_field_text(field, 8).await?, "列分隔符")?
            },
            "file" => {
                let spec = stream_load::validate_stream_load_spec(
                    database.as_deref().unwrap_or_default(),
                    table.as_deref().unwrap_or_default(),
                    format.as_deref().unwrap_or_default(),
                    label.as_deref(),
                    column_separator.as_deref(),
                )?;
                let path = stage_stream_load_file(field).await?;
                staged_file = Some((spec, path));
            },
            _ => return Err(crate::utils::ApiError::validation_error("不支持的上传字段")),
        }
    }

    let (spec, path) = staged_file.ok_or_else(|| {
        crate::utils::ApiError::validation_error("请选择要导入的 CSV 或 JSON 文件")
    })?;
    let result = send_stream_load(&state, &cluster, &spec, &path).await;
    let _ = tokio::fs::remove_file(&path).await;
    let result = result?;

    let history = QueryExecutionHistoryService::new(state.db.clone());
    let description = format!(
        "STREAM LOAD `{}`.`{}`{}",
        spec.database,
        spec.table,
        spec.label
            .as_deref()
            .map(|label| format!(" WITH LABEL `{label}`"))
            .unwrap_or_default()
    );
    let _ = history
        .record_execution(ExecutionRecord {
            user_id: org_ctx.user_id,
            cluster_id: cluster.id,
            catalog: None,
            database_name: Some(&spec.database),
            sql_statement: &description,
            execution_time_ms: None,
            row_count: result.number_loaded_rows.map(|value| value as i64),
            success: result.success,
            error_message: result.message.as_deref(),
        })
        .await;

    Ok(Json(result))
}

async fn send_stream_load<DB: crate::db::AppDb>(
    state: &Arc<AppState<DB>>,
    cluster: &crate::models::Cluster,
    spec: &stream_load::StreamLoadSpec,
    path: &std::path::Path,
) -> ApiResult<StreamLoadResponse> {
    let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let backend = adapter
        .get_backends()
        .await?
        .into_iter()
        .find(|node| node.alive.eq_ignore_ascii_case("true"))
        .ok_or_else(|| {
            crate::utils::ApiError::cluster_connection_failed(
                "没有健康的 BE 或 CN 可执行 Stream Load",
            )
        })?;
    let url = stream_load::stream_load_url(cluster, &backend, spec)?;
    let file = tokio::fs::File::open(path).await.map_err(|error| {
        crate::utils::ApiError::internal_error(format!("读取上传临时文件失败: {error}"))
    })?;
    let body = reqwest::Body::wrap_stream(stream::try_unfold(file, |mut file| async move {
        let mut buffer = vec![0; 64 * 1024];
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            Ok::<Option<(Bytes, tokio::fs::File)>, std::io::Error>(None)
        } else {
            buffer.truncate(read);
            Ok::<Option<(Bytes, tokio::fs::File)>, std::io::Error>(Some((
                Bytes::from(buffer),
                file,
            )))
        }
    }));

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(|error| {
            crate::utils::ApiError::internal_error(format!("创建 Stream Load 客户端失败: {error}"))
        })?;
    let mut request = client
        .put(url)
        .basic_auth(&cluster.username, cluster.get_auth_password())
        .header("Expect", "100-continue")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(body);
    if let Some(label) = spec.label.as_deref() {
        request = request.header("label", label);
    }
    if let Some(format) = spec.format.as_header_value() {
        request = request.header("format", format);
    }
    if let Some(separator) = spec.column_separator.as_deref() {
        request = request.header("column_separator", separator);
    }

    let response = request.send().await.map_err(|error| {
        crate::utils::ApiError::cluster_connection_failed(format!("Stream Load 请求失败: {error}"))
    })?;
    let status = response.status();
    let payload = response.text().await.map_err(|error| {
        crate::utils::ApiError::cluster_connection_failed(format!(
            "读取 Stream Load 结果失败: {error}"
        ))
    })?;
    if !status.is_success() {
        let message = payload.chars().take(1_000).collect::<String>();
        return Err(crate::utils::ApiError::cluster_connection_failed(format!(
            "Stream Load 返回 HTTP {status}: {message}"
        )));
    }
    Ok(stream_load::parse_stream_load_response(&payload))
}

fn set_form_value(target: &mut Option<String>, value: String, field: &str) -> ApiResult<()> {
    if target.replace(value).is_some() {
        return Err(crate::utils::ApiError::validation_error(format!("{field} 不能重复")));
    }
    Ok(())
}

async fn read_field_text(
    mut field: axum::extract::multipart::Field<'_>,
    max_bytes: usize,
) -> ApiResult<String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|error| {
        crate::utils::ApiError::invalid_data(format!("读取上传字段失败: {error}"))
    })? {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(crate::utils::ApiError::validation_error("上传字段过长"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| crate::utils::ApiError::validation_error("上传字段不是有效文本"))
}

async fn stage_stream_load_file(
    mut field: axum::extract::multipart::Field<'_>,
) -> ApiResult<std::path::PathBuf> {
    let path = stream_load_temp_path();
    let result = async {
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
            .map_err(|error| {
                crate::utils::ApiError::internal_error(format!("创建上传临时文件失败: {error}"))
            })?;
        let mut size = 0_u64;
        while let Some(chunk) = field.chunk().await.map_err(|error| {
            crate::utils::ApiError::invalid_data(format!("读取上传文件失败: {error}"))
        })? {
            size = size.saturating_add(chunk.len() as u64);
            if size > stream_load::MAX_STREAM_LOAD_BYTES {
                return Err(crate::utils::ApiError::validation_error(format!(
                    "文件不能超过 {} MiB",
                    stream_load::MAX_STREAM_LOAD_BYTES / 1024 / 1024
                )));
            }
            output.write_all(&chunk).await.map_err(|error| {
                crate::utils::ApiError::internal_error(format!("写入上传临时文件失败: {error}"))
            })?;
        }
        output.flush().await.map_err(|error| {
            crate::utils::ApiError::internal_error(format!("刷新上传临时文件失败: {error}"))
        })?;
        Ok::<(), crate::utils::ApiError>(())
    }
    .await;

    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(error);
    }
    Ok(path)
}

fn stream_load_temp_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("stellar-stream-load-{}.tmp", uuid::Uuid::new_v4()))
}
