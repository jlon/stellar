use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use stellar_macros::app_db;

use crate::services::metrics_collector_service::fill_backend_capacity;
use crate::{
    AppState,
    db::AppDb,
    middleware::OrgContext,
    models::Backend,
    services::{StarRocksClient, create_adapter},
    utils::{ApiError, ApiResult},
};

const MAX_BACKEND_DIAGNOSTIC_BYTES: usize = 2 * 1024 * 1024;
const MAX_MEMORY_TRACKERS: usize = 12;
const MAX_BLOCKING_DRIVERS: usize = 100;

#[derive(Debug, Deserialize)]
pub struct BackendDiagnosticQuery {
    cluster_id: i64,
    backend_id: String,
    host: String,
    heartbeat_port: String,
    http_port: String,
    include: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BackendDiagnosticDetail {
    BlockingDrivers,
    Compaction,
}

impl BackendDiagnosticDetail {
    fn parse(value: Option<&str>) -> ApiResult<Option<Self>> {
        match value {
            None => Ok(None),
            Some("blocking_drivers") => Ok(Some(Self::BlockingDrivers)),
            Some("compaction") => Ok(Some(Self::Compaction)),
            Some(_) => Err(ApiError::invalid_data("Unsupported backend diagnostic detail")),
        }
    }

    fn endpoint(self) -> BackendDiagnosticEndpoint {
        match self {
            Self::BlockingDrivers => BackendDiagnosticEndpoint::BlockingDrivers,
            Self::Compaction => BackendDiagnosticEndpoint::Compaction,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum BackendDiagnosticEndpoint {
    Memory,
    DataCache,
    BlockingDrivers,
    Compaction,
}

impl BackendDiagnosticEndpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Memory => "/metrics/memory",
            Self::DataCache => "/api/datacache/app_stat",
            Self::BlockingDrivers => "/api/pipeline_blocking_drivers/stat",
            Self::Compaction => "/api/compaction/running",
        }
    }
}

#[derive(Serialize)]
pub struct BackendDiagnosticResponse {
    pub deployment_mode: String,
    pub captured_at: String,
    pub memory: DiagnosticProbe<BackendMemorySummary>,
    pub data_cache: DiagnosticProbe<BackendDataCacheSummary>,
    pub blocking_drivers: Option<DiagnosticProbe<BlockingDriversSummary>>,
    pub compaction: Option<DiagnosticProbe<CompactionSummary>>,
}

#[derive(Serialize)]
pub struct DiagnosticProbe<T> {
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> DiagnosticProbe<T> {
    fn available(data: T) -> Self {
        Self { data: Some(data), error: None }
    }

    fn unavailable(error: ApiError) -> Self {
        Self { data: None, error: Some(diagnostic_error_message(&error)) }
    }

    fn is_available(&self) -> bool {
        self.data.is_some()
    }
}

#[derive(Serialize)]
pub struct BackendMemorySummary {
    pub process_bytes: u64,
    pub metadata_bytes: Option<u64>,
    pub update_bytes: Option<u64>,
    pub top_trackers: Vec<MemoryTracker>,
}

#[derive(Serialize)]
pub struct MemoryTracker {
    pub name: String,
    pub bytes: u64,
    pub percent_of_process: f64,
}

#[derive(Serialize)]
pub struct BackendDataCacheSummary {
    pub block_hit_rate: Option<f64>,
    pub block_hit_rate_last_minute: Option<f64>,
    pub page_hit_rate: Option<f64>,
    pub page_hit_rate_last_minute: Option<f64>,
}

#[derive(Serialize)]
pub struct BlockingDriversSummary {
    pub query_count: usize,
    pub fragment_count: usize,
    pub driver_count: usize,
    pub drivers: Vec<BlockingDriver>,
}

#[derive(Serialize)]
pub struct BlockingDriver {
    pub query_id: String,
    pub fragment_id: String,
    pub driver_id: i64,
    pub state: String,
    pub fragment_status: String,
}

#[derive(Serialize)]
pub struct CompactionSummary {
    pub max_task_num: i64,
    pub running_task_num: i64,
    pub base_task_num: i64,
    pub cumulative_task_num: i64,
    pub candidate_num: i64,
    pub tablet_num: i64,
}

#[derive(Deserialize)]
struct RawMemoryTracker {
    name: String,
    size: String,
    #[serde(default, rename = "child")]
    children: Vec<RawMemoryTracker>,
}

fn fill_storage(backends: &mut [Backend], shared_data: bool) {
    for backend in backends {
        fill_backend_capacity(backend, shared_data);
    }
}

// Get all backends for a cluster (BE nodes in shared-nothing, CN nodes in shared-data)
#[utoipa::path(
    get,
    path = "/api/clusters/backends",
    responses(
        (status = 200, description = "List of compute nodes (BE in shared-nothing, CN in shared-data)", body = Vec<Backend>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Backends"
)]
#[app_db]
pub async fn list_backends(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<Backend>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    let cluster_id = cluster.id;
    let shared_data = cluster.is_shared_data();
    if let Some(mut backends) = state
        .metrics_collector_service
        .cached_backends(cluster_id, Duration::from_secs(90))
        .or_else(|| state.metrics_collector_service.stale_backends(cluster_id))
    {
        fill_storage(&mut backends, shared_data);
        return Ok(Json(backends));
    }
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let mut backends = adapter.get_backends().await?;
    state
        .metrics_collector_service
        .store_backends(cluster_id, backends.clone());
    fill_storage(&mut backends, shared_data);
    Ok(Json(backends))
}

/// Read mode-aware, node-local diagnostics through a fixed StarRocks BE HTTP allowlist.
/// The browser supplies a node identity only; it never controls the upstream URL or path.
#[utoipa::path(
    get,
    path = "/api/clusters/backends/diagnostics",
    params(
        ("cluster_id" = i64, Query, description = "Active cluster ID"),
        ("backend_id" = String, Query, description = "Discovered BE or CN ID"),
        ("host" = String, Query, description = "Discovered node host"),
        ("heartbeat_port" = String, Query, description = "Discovered heartbeat port"),
        ("http_port" = String, Query, description = "Discovered BE HTTP port"),
        ("include" = Option<String>, Query, description = "Optional fixed detail: blocking_drivers or compaction")
    ),
    responses(
        (status = 200, description = "Mode-aware backend diagnostics"),
        (status = 400, description = "Invalid detail or unsupported deployment mode"),
        (status = 404, description = "Cluster or node is no longer available")
    ),
    security(("bearer_auth" = [])),
    tag = "Backends"
)]
#[app_db]
pub async fn get_backend_diagnostics(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Query(query): Query<BackendDiagnosticQuery>,
) -> ApiResult<Json<BackendDiagnosticResponse>> {
    let cluster = state.cluster_service.get_cluster(query.cluster_id).await?;
    crate::handlers::frontend::validate_selected_cluster(&cluster, &org_ctx)?;
    if !cluster.is_starrocks() {
        return Err(ApiError::not_implemented(
            "Backend diagnostics are supported for StarRocks clusters only",
        ));
    }

    let detail = BackendDiagnosticDetail::parse(query.include.as_deref())?;
    if detail == Some(BackendDiagnosticDetail::Compaction) && cluster.is_shared_data() {
        return Err(ApiError::invalid_data(
            "CN node diagnostics do not include cluster compaction",
        ));
    }
    let client = StarRocksClient::new(cluster.clone(), state.mysql_pool_manager.clone());
    let backend = resolve_backend(&client, &query).await?;
    let http_client = backend_http_client()?;

    let memory_url = backend_url(&cluster, &backend, BackendDiagnosticEndpoint::Memory)?;
    let cache_url = backend_url(&cluster, &backend, BackendDiagnosticEndpoint::DataCache)?;
    let (memory, data_cache) = tokio::join!(
        read_backend_probe(&http_client, memory_url, parse_memory_summary),
        read_backend_probe(&http_client, cache_url, parse_data_cache_summary),
    );

    let optional_probe = match detail {
        Some(detail) => {
            let url = backend_url(&cluster, &backend, detail.endpoint())?;
            Some(read_backend_detail_probe(&http_client, url, detail).await)
        },
        None => None,
    };
    let (blocking_drivers, compaction) = match (detail, optional_probe) {
        (
            Some(BackendDiagnosticDetail::BlockingDrivers),
            Some(BackendDetailProbe::BlockingDrivers(probe)),
        ) => (Some(probe), None),
        (
            Some(BackendDiagnosticDetail::Compaction),
            Some(BackendDetailProbe::Compaction(probe)),
        ) => (None, Some(probe)),
        _ => (None, None),
    };
    let complete = memory.is_available()
        && data_cache.is_available()
        && blocking_drivers
            .as_ref()
            .is_none_or(DiagnosticProbe::is_available)
        && compaction
            .as_ref()
            .is_none_or(DiagnosticProbe::is_available);
    let any_available = memory.is_available()
        || data_cache.is_available()
        || blocking_drivers
            .as_ref()
            .is_some_and(DiagnosticProbe::is_available)
        || compaction
            .as_ref()
            .is_some_and(DiagnosticProbe::is_available);
    let outcome = if complete {
        "success"
    } else if any_available {
        "partial"
    } else {
        "failure"
    };
    audit_backend_diagnostics(&state, &org_ctx, &cluster, &backend, outcome).await?;

    Ok(Json(BackendDiagnosticResponse {
        deployment_mode: cluster.deployment_mode.to_string(),
        captured_at: chrono::Utc::now().to_rfc3339(),
        memory,
        data_cache,
        blocking_drivers,
        compaction,
    }))
}

// Delete a backend/compute node (BE in shared-nothing, CN in shared-data)
#[utoipa::path(
    delete,
    path = "/api/clusters/backends/{host}/{port}",
    params(
        ("host" = String, Path, description = "Node host"),
        ("port" = String, Path, description = "Node heartbeat port")
    ),
    responses(
        (status = 200, description = "Node deleted successfully"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Failed to delete node")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Backends"
)]
#[app_db]
pub async fn delete_backend(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path((host, port)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    tracing::info!("Deleting backend {}:{} from cluster {}", host, port, cluster.id);

    let cluster_id = cluster.id;
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter.drop_backend(&host, &port).await?;
    state
        .metrics_collector_service
        .invalidate_backends(cluster_id);

    Ok(Json(serde_json::json!({
        "message": format!("Backend {}:{} deleted successfully", host, port)
    })))
}

async fn resolve_backend(
    client: &StarRocksClient,
    query: &BackendDiagnosticQuery,
) -> ApiResult<Backend> {
    if query.backend_id.trim().is_empty()
        || query.host.trim().is_empty()
        || query.heartbeat_port.trim().is_empty()
        || query.http_port.trim().is_empty()
    {
        return Err(ApiError::invalid_data("Invalid backend diagnostic identity"));
    }

    client
        .get_backends()
        .await?
        .into_iter()
        .find(|backend| {
            backend.backend_id == query.backend_id
                && backend.host == query.host
                && backend.heartbeat_port == query.heartbeat_port
                && backend.http_port == query.http_port
        })
        .ok_or_else(|| ApiError::not_found("Backend is no longer present in the active cluster"))
}

fn backend_http_client() -> ApiResult<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ApiError::bad_gateway("Failed to initialize backend HTTP client"))
}

/// Construct a BE endpoint from engine-discovered identity and a compile-time allowlist only.
pub(crate) fn backend_url(
    cluster: &crate::models::Cluster,
    backend: &Backend,
    endpoint: BackendDiagnosticEndpoint,
) -> ApiResult<reqwest::Url> {
    let host = backend.host.trim();
    let port = backend
        .http_port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| ApiError::invalid_data("Invalid discovered backend HTTP port"))?;
    if host.is_empty() || host != backend.host {
        return Err(ApiError::invalid_data("Invalid discovered backend host"));
    }

    let authority_host = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(address)) => format!("[{address}]"),
        _ => host.to_string(),
    };
    let scheme = if cluster.enable_ssl { "https" } else { "http" };
    let mut url = reqwest::Url::parse(&format!("{scheme}://{authority_host}:{port}/"))
        .map_err(|_| ApiError::invalid_data("Invalid discovered backend host"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || !url
            .host_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(&authority_host))
        || url.port_or_known_default() != Some(port)
    {
        return Err(ApiError::invalid_data("Invalid discovered backend authority"));
    }
    url.set_path(endpoint.path());
    Ok(url)
}

async fn read_backend_probe<T>(
    client: &reqwest::Client,
    url: reqwest::Url,
    parse: fn(serde_json::Value) -> ApiResult<T>,
) -> DiagnosticProbe<T> {
    match fetch_backend_json(client, url).await.and_then(parse) {
        Ok(data) => DiagnosticProbe::available(data),
        Err(error) => DiagnosticProbe::unavailable(error),
    }
}

enum BackendDetailProbe {
    BlockingDrivers(DiagnosticProbe<BlockingDriversSummary>),
    Compaction(DiagnosticProbe<CompactionSummary>),
}

async fn read_backend_detail_probe(
    client: &reqwest::Client,
    url: reqwest::Url,
    detail: BackendDiagnosticDetail,
) -> BackendDetailProbe {
    match detail {
        BackendDiagnosticDetail::BlockingDrivers => BackendDetailProbe::BlockingDrivers(
            read_backend_probe(client, url, parse_blocking_drivers_summary).await,
        ),
        BackendDiagnosticDetail::Compaction => BackendDetailProbe::Compaction(
            read_backend_probe(client, url, parse_compaction_summary).await,
        ),
    }
}

async fn fetch_backend_json(
    client: &reqwest::Client,
    url: reqwest::Url,
) -> ApiResult<serde_json::Value> {
    // BE HTTP has no reviewed Stellar-compatible authentication middleware. Never forward
    // cluster credentials to it; URL construction and endpoint selection are the boundary.
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json, text/plain")
        .send()
        .await
        .map_err(|_| ApiError::bad_gateway("Failed to contact the StarRocks backend"))?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ApiError::not_found(
            "This StarRocks backend does not provide the requested diagnostic endpoint",
        ));
    }
    if !status.is_success() {
        return Err(ApiError::bad_gateway(format!(
            "StarRocks backend returned HTTP {}",
            status.as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BACKEND_DIAGNOSTIC_BYTES as u64)
    {
        return Err(ApiError::bad_gateway(
            "StarRocks backend diagnostic response exceeds the size limit",
        ));
    }

    let mut response = response;
    let mut body = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::bad_gateway("Failed to read the StarRocks backend response"))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_BACKEND_DIAGNOSTIC_BYTES {
            return Err(ApiError::bad_gateway(
                "StarRocks backend diagnostic response exceeds the size limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| {
        ApiError::bad_gateway("StarRocks backend returned an invalid diagnostic response")
    })
}

pub(crate) fn parse_memory_summary(value: serde_json::Value) -> ApiResult<BackendMemorySummary> {
    let roots: Vec<RawMemoryTracker> = serde_json::from_value(value)
        .map_err(|_| ApiError::bad_gateway("StarRocks backend returned an invalid memory tree"))?;
    let process = roots
        .first()
        .ok_or_else(|| ApiError::bad_gateway("StarRocks backend returned an empty memory tree"))?;
    let process_bytes = memory_tracker_bytes(process)?;
    let metadata_bytes = roots.get(1).map(memory_tracker_bytes).transpose()?;
    let update_bytes = roots.get(2).map(memory_tracker_bytes).transpose()?;

    let mut stack: Vec<&RawMemoryTracker> = process.children.iter().collect();
    let mut trackers = Vec::new();
    while let Some(tracker) = stack.pop() {
        let bytes = memory_tracker_bytes(tracker)?;
        trackers.push(MemoryTracker {
            name: tracker.name.clone(),
            bytes,
            percent_of_process: if process_bytes == 0 {
                0.0
            } else {
                bytes as f64 * 100.0 / process_bytes as f64
            },
        });
        stack.extend(tracker.children.iter());
    }
    trackers.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.name.cmp(&right.name))
    });
    trackers.truncate(MAX_MEMORY_TRACKERS);

    Ok(BackendMemorySummary { process_bytes, metadata_bytes, update_bytes, top_trackers: trackers })
}

fn memory_tracker_bytes(tracker: &RawMemoryTracker) -> ApiResult<u64> {
    tracker
        .size
        .parse::<i64>()
        .ok()
        .filter(|size| *size >= 0)
        .map(|size| size as u64)
        .ok_or_else(|| {
            ApiError::bad_gateway("StarRocks backend returned an invalid memory tracker size")
        })
}

pub(crate) fn parse_data_cache_summary(
    value: serde_json::Value,
) -> ApiResult<BackendDataCacheSummary> {
    if value.get("error").is_some() {
        return Err(ApiError::bad_gateway("StarRocks Data Cache is not ready on this node"));
    }
    Ok(BackendDataCacheSummary {
        block_hit_rate: json_number(&value, "block_cache_hit_rate"),
        block_hit_rate_last_minute: json_number(&value, "block_cache_hit_rate_last_minute"),
        page_hit_rate: json_number(&value, "page_cache_hit_rate"),
        page_hit_rate_last_minute: json_number(&value, "page_cache_hit_rate_last_minute"),
    })
}

pub(crate) fn parse_blocking_drivers_summary(
    value: serde_json::Value,
) -> ApiResult<BlockingDriversSummary> {
    if value.get("error").is_some() {
        return Err(ApiError::bad_gateway("StarRocks blocking-driver diagnostics are unavailable"));
    }
    let queries = value
        .get("queries_in_workgroup")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            ApiError::bad_gateway("StarRocks backend returned invalid blocking-driver data")
        })?;
    let mut fragment_count = 0;
    let mut driver_count = 0;
    let mut drivers = Vec::new();
    for query in queries {
        let query_id = json_string(query, "query_id");
        let fragments = query
            .get("fragments")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                ApiError::bad_gateway("StarRocks backend returned invalid blocking fragments")
            })?;
        fragment_count += fragments.len();
        for fragment in fragments {
            let fragment_id = json_string(fragment, "fragment_id");
            let fragment_status = json_string(fragment, "fragment_status");
            let raw_drivers = fragment
                .get("drivers")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    ApiError::bad_gateway("StarRocks backend returned invalid blocking drivers")
                })?;
            driver_count += raw_drivers.len();
            for driver in raw_drivers {
                if drivers.len() == MAX_BLOCKING_DRIVERS {
                    break;
                }
                drivers.push(BlockingDriver {
                    query_id: query_id.clone(),
                    fragment_id: fragment_id.clone(),
                    driver_id: json_number(driver, "driver_id").unwrap_or_default() as i64,
                    state: json_string(driver, "state"),
                    fragment_status: fragment_status.clone(),
                });
            }
        }
    }
    Ok(BlockingDriversSummary { query_count: queries.len(), fragment_count, driver_count, drivers })
}

pub(crate) fn parse_compaction_summary(value: serde_json::Value) -> ApiResult<CompactionSummary> {
    if value.get("status").is_some() && value.get("running_task_num").is_none() {
        return Err(ApiError::bad_gateway("StarRocks compaction diagnostics are unavailable"));
    }
    let running_task_num = json_number(&value, "running_task_num").ok_or_else(|| {
        ApiError::bad_gateway("StarRocks backend returned invalid compaction data")
    })?;
    Ok(CompactionSummary {
        max_task_num: json_number(&value, "max_task_num").unwrap_or_default() as i64,
        running_task_num: running_task_num as i64,
        base_task_num: json_number(&value, "base_task_num").unwrap_or_default() as i64,
        cumulative_task_num: json_number(&value, "cumulative_task_num").unwrap_or_default()
            as i64,
        candidate_num: json_number(&value, "candidate_num").unwrap_or_default() as i64,
        tablet_num: json_number(&value, "tablet_num").unwrap_or_default() as i64,
    })
}

fn json_number(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(number) => number.parse().ok(),
        _ => None,
    })
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn diagnostic_error_message(error: &ApiError) -> String {
    match error {
        ApiError::ResourceNotFound(_) => "当前节点未提供该诊断端点".to_string(),
        ApiError::BadGateway(_) | ApiError::ClusterConnectionFailed { .. } => {
            "节点未能返回诊断数据，请确认节点在线后重试".to_string()
        },
        _ => "诊断数据暂不可用".to_string(),
    }
}

#[app_db]
async fn audit_backend_diagnostics<DB: AppDb>(
    state: &AppState<DB>,
    org_ctx: &OrgContext,
    cluster: &crate::models::Cluster,
    backend: &Backend,
    outcome: &str,
) -> ApiResult<()> {
    let node_type = if cluster.is_shared_data() { "cn" } else { "be" };
    let target_name = truncate_audit_target(&format!(
        "{} {}:{} [{}]",
        node_type, backend.host, backend.heartbeat_port, outcome
    ));
    crate::services::op_audit::log_op(
        &state.db,
        crate::services::op_audit::OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "diagnose",
            target_type: "backend",
            target_id: backend.backend_id.parse().ok(),
            target_name: &target_name,
        },
    )
    .await
    .map_err(|error| {
        tracing::warn!(
            target: "audit",
            audit_event = "backend_diagnostics",
            cluster_id = cluster.id,
            backend_id = %backend.backend_id,
            "Failed to persist backend diagnostics audit: {}",
            error
        );
        ApiError::internal_error("Failed to record backend diagnostic access audit")
    })?;
    tracing::info!(
        target: "audit",
        audit_event = "backend_diagnostics",
        user_id = org_ctx.user_id,
        username = %org_ctx.username,
        organization_id = ?org_ctx.organization_id,
        cluster_id = cluster.id,
        backend_id = %backend.backend_id,
        backend_host = %backend.host,
        backend_http_port = %backend.http_port,
        deployment_mode = %cluster.deployment_mode,
        outcome,
        "StarRocks backend diagnostics"
    );
    Ok(())
}

fn truncate_audit_target(value: &str) -> String {
    value.chars().take(200).collect()
}
