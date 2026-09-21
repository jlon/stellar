use crate::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use std::collections::HashSet;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::models::{ProfileDetail, ProfileListItem, ProfileRetestResponse, ProfileRetestRun};
use crate::services::MySQLClient;
use crate::services::cluster_adapter::create_adapter;
use crate::services::llm::{
    DiagnosticForLLM, ExecutionPlanForLLM, HotspotNodeForLLM, KeyMetricsForLLM, LLMService,
    OperatorDetailForLLM, ProfileDataForLLM, QuerySummaryForLLM, RootCauseAnalysisRequest,
    RootCauseAnalysisResponse, ScanDetailForLLM, determine_connector_type, determine_table_type,
};
use crate::services::profile_analyzer::{
    AnalysisContext, ClusterVariables, LLMEnhancedAnalysis, ProfileAnalysisResponse,
    analyze_profile_with_context, analyzer::QueryComplexity,
};
use crate::utils::{ApiResult, error::ApiError};

/// Validate and sanitize query_id to prevent SQL injection
/// StarRocks query_id format: UUID like "12345678-1234-1234-1234-123456789abc"
///
/// Returns the sanitized (trimmed) query_id as a String.
/// The sanitized version is what should be used for:
/// - SQL queries (security)
/// - API responses (consistency)
/// - Error messages (clarity)
fn sanitize_query_id(query_id: &str) -> Result<String, ApiError> {
    let id = query_id.trim();

    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ApiError::invalid_data("Invalid query_id format"));
    }

    Ok(id.to_string())
}

// List all query profiles for a cluster
#[utoipa::path(
    get,
    path = "/api/clusters/profiles",
    responses(
        (status = 200, description = "List of query profiles", body = Vec<ProfileListItem>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Profiles"
)]
#[app_db]
pub async fn list_profiles(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<ProfileListItem>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    tracing::info!("Fetching profile list for cluster {}", cluster.id);

    let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let profiles = adapter.list_profiles().await?;

    tracing::info!("Successfully fetched {} profiles", profiles.len());
    Ok(Json(profiles))
}

/// 同一 SQL 指纹的执行序列，用于对比处置前后的耗时。
///
/// 仅呈现引擎事实：按归一化指纹分组，不做因果推断；`scanned` 说明本次比对的
/// 数据边界（引擎 profile 列表条数），列表超出部分不参与比对。
#[utoipa::path(
    get,
    path = "/api/clusters/profiles/{query_id}/retest",
    params(
        ("query_id" = String, Path, description = "Query ID")
    ),
    responses(
        (status = 200, description = "同一指纹的执行记录", body = ProfileRetestResponse),
        (status = 404, description = "Profile 不存在")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Profiles"
)]
#[app_db]
pub async fn get_profile_retest(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(query_id): Path<String>,
) -> ApiResult<Json<ProfileRetestResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let safe_query_id = sanitize_query_id(&query_id)?;
    let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let profiles = adapter.list_profiles().await?;

    // profile 列表是"最近 N 条"的滚动窗口，被诊断的查询可能已滑出；
    // 此时回退到 StarRocks 审计日志按 queryId 取原始 SQL，保持比对可用。
    let audit = crate::services::audit_log_service::AuditLogService::new(
        Arc::clone(&state.mysql_pool_manager),
        state.audit_config.clone(),
    );
    let baseline_sql = match profiles.iter().find(|item| item.query_id == safe_query_id) {
        Some(item) => item.statement.clone(),
        None => match audit.get_query_sql(&cluster, &safe_query_id).await {
            Ok((statement, _)) => statement,
            // 高频集群存在窗口期：profile 列表已滚动、审计尚未落盘。
            // 最后一次回退是按 ID 直取 profile 原文并解析 Query 段。
            Err(_) => {
                let content = adapter.get_profile(&safe_query_id).await.map_err(|error| {
                    ApiError::not_found(format!("无法比对：查不到该查询的 Profile（{error}）"))
                })?;
                extract_query_sql(&content).ok_or_else(|| {
                    ApiError::not_found("无法比对：Profile 原文中没有可解析的查询语句")
                })?
            },
        },
    };

    let fingerprint = crate::handlers::query::sql_fingerprint(&baseline_sql);
    let window_minutes: i64 = 24 * 60;

    // 主数据源：审计日志（全量、不受 profile 滚动窗口限制）。
    let mut audit_runs: Vec<ProfileRetestRun> = Vec::new();
    let mut audit_available = false;
    const AUDIT_LIMIT: usize = 1_000;
    if let Some(hint) = crate::services::audit_log_service::like_hint(&baseline_sql) {
        match audit
            .get_query_runs(&cluster, &hint, window_minutes, AUDIT_LIMIT)
            .await
        {
            Ok(rows) => {
                audit_available = true;
                audit_runs = rows
                    .into_iter()
                    .filter(|row| crate::handlers::query::sql_fingerprint(&row.stmt) == fingerprint)
                    .map(|row| ProfileRetestRun {
                        is_baseline: row.query_id == safe_query_id,
                        query_id: row.query_id,
                        start_time: normalize_time(&row.timestamp),
                        time_ms: row.time_ms,
                        // 审计日志不提供查询状态，保持为空而不是猜测
                        state: None,
                    })
                    .collect();
            },
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "audit query runs unavailable, falling back to profile window"
                );
            },
        }
    }

    // 补充数据源：profile 滚动窗口（审计未启用或尚未写入时仍可用）。
    let (_, profile_runs) = build_retest_runs(&profiles, &safe_query_id, &baseline_sql);
    let runs = merge_runs(audit_runs, profile_runs);
    let truncated = runs.len() >= AUDIT_LIMIT && audit_available;

    Ok(Json(ProfileRetestResponse {
        fingerprint,
        scanned: profiles.len(),
        audit_available,
        window_minutes,
        truncated,
        runs,
    }))
}

/// 合并两个来源的执行记录：审计优先，按 query_id 去重，按时间升序排列。
pub(crate) fn merge_runs(
    primary: Vec<ProfileRetestRun>,
    fallback: Vec<ProfileRetestRun>,
) -> Vec<ProfileRetestRun> {
    let mut seen: HashSet<String> = primary.iter().map(|run| run.query_id.clone()).collect();
    let mut merged = primary;
    for run in fallback {
        if seen.insert(run.query_id.clone()) {
            merged.push(run);
        }
    }
    merged.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    merged
}

/// 从 profile 原文解析查询语句：`Query:` 段之后、下一个顶层字段之前。
///
/// 只用于获取指纹与展示（不执行），因此对格式差异容忍度高于执行路径。
pub(crate) fn extract_query_sql(profile_content: &str) -> Option<String> {
    let mut started = false;
    let mut sql = String::new();
    for line in profile_content.lines() {
        let trimmed = line.trim();
        if !started {
            if trimmed.eq_ignore_ascii_case("query:") {
                started = true;
            }
            continue;
        }
        if is_profile_section(trimmed) {
            break;
        }
        sql.push_str(line);
        sql.push('\n');
    }
    let sql = sql.trim();
    (!sql.is_empty()).then(|| sql.to_string())
}

/// 判断是否为 profile 的顶层字段行（如 `Summary:`、`Query ID: abc`）。
///
/// StarRocks 的字段行既可能是 `Key:` 也可能是 `Key: value`，因此以冒号前的
/// 键名判定；键名只含字母/空格/下划线且首字母大写，SQL 里的 `-- note: x`
/// 这类注释不会命中。
fn is_profile_section(line: &str) -> bool {
    let Some((key, _value)) = line.split_once(':') else {
        return false;
    };
    !key.is_empty()
        && key.len() <= 40
        && key.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '_')
}

/// 归一化时间戳为 `YYYY-MM-DD HH:MM:SS`，使审计与 profile 两种来源可比较排序。
pub(crate) fn normalize_time(value: &str) -> String {
    value.trim().chars().take(19).collect()
}

/// 按归一化指纹收集同一查询的执行序列（时间升序）。
///
/// 耗时段解析失败的记录会被丢弃，避免用 0 或估算值参与对比。
pub(crate) fn build_retest_runs(
    profiles: &[ProfileListItem],
    baseline_id: &str,
    baseline_sql: &str,
) -> (String, Vec<ProfileRetestRun>) {
    let fingerprint = crate::handlers::query::sql_fingerprint(baseline_sql);

    let mut runs: Vec<ProfileRetestRun> = profiles
        .iter()
        .filter(|item| crate::handlers::query::sql_fingerprint(&item.statement) == fingerprint)
        .filter_map(|item| {
            Some(ProfileRetestRun {
                query_id: item.query_id.clone(),
                start_time: normalize_time(&item.start_time),
                time_ms: parse_profile_time(&item.time)?,
                state: Some(item.state.clone()),
                is_baseline: item.query_id == baseline_id,
            })
        })
        .collect();
    runs.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    (fingerprint, runs)
}

/// 解析引擎 profile 列表的耗时段：`9ms`、`1s234ms`、`2m3s`、`1m`。
///
/// 解析失败返回 None（例如缺少单位或含未知字符），由调用方决定丢弃，
/// 不猜测数值。
pub(crate) fn parse_profile_time(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut total: u64 = 0;
    let mut number = String::new();
    let mut unit = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            if !unit.is_empty() {
                total = total.saturating_add(unit_to_millis(&number, &unit)?);
                number.clear();
                unit.clear();
            }
            number.push(ch);
        } else if ch.is_ascii_alphabetic() {
            unit.push(ch);
        } else {
            return None;
        }
    }
    if number.is_empty() || unit.is_empty() {
        return None;
    }
    total = total.saturating_add(unit_to_millis(&number, &unit)?);
    Some(total)
}

fn unit_to_millis(number: &str, unit: &str) -> Option<u64> {
    let value: u64 = number.parse().ok()?;
    match unit.to_ascii_lowercase().as_str() {
        "ms" => Some(value),
        "s" => Some(value.saturating_mul(1_000)),
        "m" => Some(value.saturating_mul(60_000)),
        "h" => Some(value.saturating_mul(3_600_000)),
        _ => None,
    }
}

// Get detailed profile for a specific query
#[utoipa::path(
    get,
    path = "/api/clusters/profiles/{query_id}",
    params(
        ("query_id" = String, Path, description = "Query ID")
    ),
    responses(
        (status = 200, description = "Query profile detail", body = ProfileDetail),
        (status = 404, description = "No active cluster found or profile not found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Profiles"
)]
#[app_db]
pub async fn get_profile(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(query_id): Path<String>,
) -> ApiResult<Json<ProfileDetail>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let safe_query_id = sanitize_query_id(&query_id)?;

    if query_id.trim() != query_id {
        tracing::debug!("Query ID sanitized: '{}' -> '{}'", query_id, safe_query_id);
    }

    tracing::info!("Fetching profile detail for query {} in cluster {}", safe_query_id, cluster.id);

    let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let profile_content = adapter.get_profile(&safe_query_id).await?;

    tracing::info!("Profile content length: {} bytes", profile_content.len());

    Ok(Json(ProfileDetail { query_id: safe_query_id, profile_content }))
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct AnalyzeProfileQuery {
    #[serde(default)]
    pub refresh: bool,
}

/// Analyze a query profile and return structured visualization data
#[utoipa::path(
    get,
    path = "/api/clusters/profiles/{query_id}/analyze",
    params(
        ("query_id" = String, Path, description = "Query ID to analyze"),
        ("refresh" = Option<bool>, Query, description = "Bypass analysis cache and re-parse")
    ),
    responses(
        (status = 200, description = "Profile analysis result with execution tree"),
        (status = 404, description = "No active cluster found or profile not found"),
        (status = 500, description = "Profile parsing failed")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Profiles"
)]
#[app_db]
pub async fn analyze_profile_handler(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(query_id): Path<String>,
    Query(query): Query<AnalyzeProfileQuery>,
) -> ApiResult<Json<ProfileAnalysisResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let safe_query_id = sanitize_query_id(&query_id)?;

    if query_id.trim() != query_id {
        tracing::debug!("Query ID sanitized: '{}' -> '{}'", query_id, safe_query_id);
    }

    if query.refresh {
        state
            .profile_analysis_cache
            .invalidate(cluster.id, &safe_query_id);
    } else if let Some(cached) = state.profile_analysis_cache.get(cluster.id, &safe_query_id) {
        tracing::info!(
            "Profile analysis cache hit for query {} in cluster {}",
            safe_query_id,
            cluster.id
        );
        return Ok(Json(attach_llm_pending(cached, state.llm_service.is_available())));
    }

    tracing::info!("Analyzing profile for query {} in cluster {}", safe_query_id, cluster.id);

    let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let profile_content = adapter.get_profile(&safe_query_id).await?;

    tracing::info!(
        "Profile content length: {} bytes for query {}",
        profile_content.len(),
        safe_query_id
    );

    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);
    let cluster_variables = fetch_cluster_variables(&mysql_client).await;

    let context = AnalysisContext { cluster_variables, cluster_id: Some(cluster.id) };

    let response = analyze_profile_with_context(&profile_content, &context)
        .map_err(|e| ApiError::internal_error(format!("Analysis failed: {}", e)))?;

    state
        .profile_analysis_cache
        .insert(cluster.id, safe_query_id, response.clone());

    Ok(Json(attach_llm_pending(response, state.llm_service.is_available())))
}

fn attach_llm_pending(
    mut response: ProfileAnalysisResponse,
    llm_available: bool,
) -> ProfileAnalysisResponse {
    response.llm_analysis = if llm_available {
        Some(LLMEnhancedAnalysis {
            available: true,
            status: "pending".to_string(),
            ..Default::default()
        })
    } else {
        None
    };
    response
}

/// Request body for LLM enhancement
#[derive(Debug, serde::Deserialize)]
pub struct EnhanceProfileRequest {
    /// Pre-analyzed profile data from rule engine (avoids re-parsing)
    pub analysis_data: ProfileAnalysisResponse,
    /// Force refresh - bypass cache and call LLM API
    #[serde(default)]
    pub force_refresh: bool,
}

#[app_db]
/// POST /api/clusters/:cluster_id/profiles/:query_id/enhance
///
/// Enhance profile analysis with LLM - called async by frontend after DAG is rendered.
/// Receives pre-analyzed data to avoid redundant profile parsing.
pub async fn enhance_profile_handler(
    State(state): State<Arc<AppState<DB>>>,
    Path((cluster_id, query_id)): Path<(i64, String)>,
    Json(req): Json<EnhanceProfileRequest>,
) -> ApiResult<Json<LLMEnhancedAnalysis>> {
    let safe_query_id = sanitize_query_id(&query_id)?;

    if !state.llm_service.is_available() {
        return Ok(Json(LLMEnhancedAnalysis {
            available: false,
            status: "LLM service not available".to_string(),
            ..Default::default()
        }));
    }

    // Get cluster to determine cluster type
    let cluster = state.cluster_service.get_cluster(cluster_id).await.ok();
    let cluster_type = cluster.as_ref().map(|c| c.cluster_type).unwrap_or_default();

    let cluster_variables = {
        if let Some(ref c) = cluster {
            if let Ok(pool) = state.mysql_pool_manager.get_pool(c).await {
                let mysql_client = MySQLClient::from_pool(pool);
                fetch_cluster_variables(&mysql_client).await
            } else {
                None
            }
        } else {
            None
        }
    };

    match enhance_with_llm(
        &state.llm_service,
        &req.analysis_data,
        &safe_query_id,
        Some(cluster_id),
        cluster_variables.as_ref(),
        cluster_type,
        req.force_refresh,
    )
    .await
    {
        Ok(llm_analysis) => Ok(Json(llm_analysis)),
        Err(e) => Ok(Json(LLMEnhancedAnalysis {
            available: false,
            status: format!("failed: {}", e),
            ..Default::default()
        })),
    }
}

/// Enhance profile analysis with LLM-based root cause analysis
///
/// Builds a request from the rule engine results and calls LLM for deeper analysis.
/// Results are merged using ResultMerger to combine rule-based and LLM insights.
#[app_db]
async fn enhance_with_llm(
    llm_service: &std::sync::Arc<crate::services::llm::LLMServiceImpl<DB>>,
    response: &ProfileAnalysisResponse,
    query_id: &str,
    cluster_id: Option<i64>,
    cluster_variables: Option<&ClusterVariables>,
    cluster_type: crate::models::cluster::ClusterType,
    force_refresh: bool,
) -> Result<LLMEnhancedAnalysis, String> {
    #[allow(unused_imports)]
    use crate::services::profile_analyzer::{
        LLMCausalChain, LLMHiddenIssue, MergedRecommendation, MergedRootCause,
    };
    use std::collections::HashMap;

    let summary = response.summary.as_ref();

    let session_vars: HashMap<String, String> = cluster_variables.cloned().unwrap_or_default();

    let sql = summary.map(|s| s.sql_statement.as_str()).unwrap_or("");
    let complexity = QueryComplexity::from_sql(sql);

    let query_summary = QuerySummaryForLLM {
        sql_statement: summary.map(|s| s.sql_statement.clone()).unwrap_or_default(),
        query_type: summary
            .and_then(|s| s.query_type.clone())
            .unwrap_or_else(|| "SELECT".to_string()),
        query_complexity: Some(format!("{:?}", complexity)),
        cluster_type,
        total_time_seconds: summary
            .map(|s| s.total_time_ms.unwrap_or(0.0) / 1000.0)
            .unwrap_or(0.0),
        scan_bytes: summary.and_then(|s| s.total_bytes_read).unwrap_or(0),
        output_rows: summary.and_then(|s| s.result_rows).unwrap_or(0),
        be_count: summary
            .and_then(|s| s.total_instance_count.map(|c| c as u32))
            .unwrap_or(3),
        has_spill: summary
            .and_then(|s| {
                s.query_spill_bytes
                    .as_ref()
                    .map(|b| !b.is_empty() && b != "0")
            })
            .unwrap_or(false),
        spill_bytes: summary.and_then(|s| s.query_spill_bytes.clone()),
        session_variables: session_vars,
    };

    let dag_description = response
        .execution_tree
        .as_ref()
        .map(|tree| {
            tree.nodes
                .iter()
                .take(10)
                .map(|n| n.operator_name.clone())
                .collect::<Vec<_>>()
                .join(" -> ")
        })
        .unwrap_or_else(|| "Unknown DAG".to_string());

    let hotspot_nodes: Vec<HotspotNodeForLLM> = response
        .execution_tree
        .as_ref()
        .map(|tree| {
            tree.nodes
                .iter()
                .filter(|n| n.time_percentage.unwrap_or(0.0) > 15.0)
                .take(5)
                .map(|n| HotspotNodeForLLM {
                    operator: n.operator_name.clone(),
                    plan_node_id: n.plan_node_id.unwrap_or(-1),
                    time_percentage: n.time_percentage.unwrap_or(0.0),
                    key_metrics: n
                        .unique_metrics
                        .iter()
                        .take(5)
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    upstream_operators: vec![],
                })
                .collect()
        })
        .unwrap_or_default();

    let execution_plan = ExecutionPlanForLLM { dag_description, hotspot_nodes };

    let diagnostics: Vec<DiagnosticForLLM> = response
        .aggregated_diagnostics
        .iter()
        .map(|d| DiagnosticForLLM {
            rule_id: d.rule_id.clone(),
            severity: d.severity.clone(),
            operator: d
                .affected_nodes
                .first()
                .map(|s| s.split('/').next_back().unwrap_or("unknown"))
                .unwrap_or("unknown")
                .to_string(),
            plan_node_id: None,
            message: format!("{} ({}个节点)", d.message, d.node_count),
            evidence: {
                let mut e = HashMap::new();
                e.insert("reason".to_string(), d.reason.clone());
                e.insert("affected_nodes".to_string(), d.affected_nodes.join(", "));
                e
            },

            threshold_info: None,
        })
        .collect();

    let scan_details: Vec<ScanDetailForLLM> = response
        .execution_tree
        .as_ref()
        .map(|tree| {
            tree.nodes
                .iter()
                .filter(|n| n.operator_name.contains("SCAN"))
                .map(|n| {
                    let table_name = n.unique_metrics.get("Table").cloned().unwrap_or_default();
                    let table_type = determine_table_type(&table_name);
                    let connector_type = if table_type == "external" {
                        Some(determine_connector_type(&n.unique_metrics))
                    } else {
                        Some("native".to_string())
                    };

                    ScanDetailForLLM {
                        plan_node_id: n.plan_node_id.unwrap_or(-1),
                        table_name: table_name.clone(),
                        scan_type: n.operator_name.clone(),
                        table_type,
                        connector_type,
                        rows_read: n
                            .unique_metrics
                            .get("RawRowsRead")
                            .and_then(|s| s.replace(",", "").parse().ok())
                            .unwrap_or(0),
                        rows_returned: n.rows.unwrap_or(0),
                        filter_ratio: 0.0,
                        scan_ranges: n
                            .unique_metrics
                            .get("ScanRanges")
                            .and_then(|s| s.replace(",", "").parse().ok()),
                        bytes_read: n
                            .unique_metrics
                            .get("BytesRead")
                            .and_then(|s| s.replace(",", "").replace(" B", "").parse().ok()),
                        io_time_ms: None,
                        cache_hit_rate: n
                            .unique_metrics
                            .get("DataCacheHitRate")
                            .and_then(|s| s.trim_end_matches('%').parse().ok()),
                        predicates: n.unique_metrics.get("Predicates").cloned(),
                        partitions_scanned: n.unique_metrics.get("PartitionsScanned").cloned(),
                        full_table_path: if table_name.contains('.') {
                            Some(table_name.clone())
                        } else {
                            None
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let operators: Vec<OperatorDetailForLLM> = response
        .execution_tree
        .as_ref()
        .map(|tree| {
            tree.nodes
                .iter()
                .filter(|n| n.time_percentage.unwrap_or(0.0) > 5.0)
                .map(|n| OperatorDetailForLLM {
                    operator: n.operator_name.clone(),
                    plan_node_id: n.plan_node_id.unwrap_or(-1),
                    time_pct: n.time_percentage.unwrap_or(0.0),
                    rows: n.rows.unwrap_or(0),
                    estimated_rows: None,
                    memory_bytes: None,
                    metrics: n.unique_metrics.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let profile_data = ProfileDataForLLM {
        operators,
        time_distribution: None,
        scan_details,
        join_details: vec![],
        agg_details: vec![],
        exchange_details: vec![],
    };

    let llm_request = RootCauseAnalysisRequest::builder()
        .query_summary(query_summary)
        .execution_plan(execution_plan)
        .diagnostics(diagnostics)
        .key_metrics(KeyMetricsForLLM::default())
        .profile_data(profile_data)
        .build()
        .map_err(|e| e.to_string())?;

    let start_time = std::time::Instant::now();
    let llm_result = llm_service
        .analyze(&llm_request, query_id, cluster_id, force_refresh)
        .await
        .map_err(|e| e.to_string())?;
    let elapsed_time_ms = start_time.elapsed().as_millis() as u64;

    let llm_response = llm_result.response;
    let from_cache = llm_result.from_cache;

    let root_causes = merge_root_causes(&response.aggregated_diagnostics, &llm_response);
    let recommendations = merge_recommendations(&response.aggregated_diagnostics, &llm_response);

    Ok(LLMEnhancedAnalysis {
        available: true,
        status: "completed".to_string(),
        root_causes,
        causal_chains: llm_response
            .causal_chains
            .into_iter()
            .map(|c| LLMCausalChain { chain: c.chain, explanation: c.explanation })
            .collect(),
        merged_recommendations: recommendations,
        summary: llm_response.summary,
        hidden_issues: llm_response
            .hidden_issues
            .into_iter()
            .map(|h| LLMHiddenIssue { issue: h.issue, suggestion: h.suggestion })
            .collect(),
        from_cache,
        elapsed_time_ms: Some(elapsed_time_ms),
    })
}

/// Merge root causes from rule engine and LLM
fn merge_root_causes(
    rule_diagnostics: &[crate::services::profile_analyzer::AggregatedDiagnostic],
    llm_response: &RootCauseAnalysisResponse,
) -> Vec<crate::services::profile_analyzer::MergedRootCause> {
    use crate::services::profile_analyzer::MergedRootCause;
    use std::collections::HashSet;

    let mut merged = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for llm_rc in &llm_response.root_causes {
        let id = llm_rc.root_cause_id.clone();
        seen_ids.insert(id.clone());

        let related_rules: Vec<String> = llm_rc
            .symptoms
            .iter()
            .filter(|s| rule_diagnostics.iter().any(|d| &d.rule_id == *s))
            .cloned()
            .collect();

        let source = if related_rules.is_empty() { "llm" } else { "both" };

        merged.push(MergedRootCause {
            id,
            related_rule_ids: related_rules,
            description: llm_rc.description.clone(),
            is_implicit: llm_rc.is_implicit,
            confidence: llm_rc.confidence,
            source: source.to_string(),
            evidence: llm_rc.evidence.clone(),
            symptoms: llm_rc.symptoms.clone(),
        });
    }

    for diag in rule_diagnostics {
        let is_covered = llm_response
            .root_causes
            .iter()
            .any(|rc| rc.symptoms.contains(&diag.rule_id));

        if !is_covered {
            let id = format!("rule_{}", diag.rule_id);
            if !seen_ids.contains(&id) {
                seen_ids.insert(id.clone());
                merged.push(MergedRootCause {
                    id,
                    related_rule_ids: vec![diag.rule_id.clone()],
                    description: diag.message.clone(),
                    is_implicit: false,
                    confidence: 1.0,
                    source: "rule".to_string(),
                    evidence: vec![diag.reason.clone()],
                    symptoms: vec![],
                });
            }
        }
    }

    merged.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged
}

/// Merge recommendations from rule engine and LLM
fn merge_recommendations(
    rule_diagnostics: &[crate::services::profile_analyzer::AggregatedDiagnostic],
    llm_response: &RootCauseAnalysisResponse,
) -> Vec<crate::services::profile_analyzer::MergedRecommendation> {
    use crate::services::profile_analyzer::MergedRecommendation;
    use std::collections::HashSet;

    let mut merged = Vec::new();
    let mut seen_actions: HashSet<String> = HashSet::new();

    for rec in &llm_response.recommendations {
        let action_key = normalize_action(&rec.action);
        if !seen_actions.contains(&action_key) {
            seen_actions.insert(action_key);
            merged.push(MergedRecommendation {
                priority: rec.priority,
                action: rec.action.clone(),
                expected_improvement: rec.expected_improvement.clone(),
                sql_example: rec.sql_example.clone(),
                source: "llm".to_string(),
                related_root_causes: vec![],
                is_root_cause_fix: true,
            });
        }
    }

    let mut rule_priority = merged.len() as u32 + 1;
    for diag in rule_diagnostics {
        for suggestion in &diag.suggestions {
            let action_key = normalize_action(suggestion);
            if !seen_actions.contains(&action_key) {
                seen_actions.insert(action_key.clone());
                merged.push(MergedRecommendation {
                    priority: rule_priority,
                    action: suggestion.clone(),
                    expected_improvement: String::new(),
                    sql_example: None,
                    source: "rule".to_string(),
                    related_root_causes: vec![diag.rule_id.clone()],
                    is_root_cause_fix: false,
                });
                rule_priority += 1;
            } else if let Some(existing) = merged
                .iter_mut()
                .find(|r| normalize_action(&r.action) == action_key)
                && existing.source == "llm"
            {
                existing.source = "both".to_string();
            }
        }
    }

    merged.sort_by_key(|r| r.priority);
    merged
}

/// Normalize action text for deduplication
fn normalize_action(action: &str) -> String {
    action
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// Truncate SQL statement for LLM request (kept for future use)
#[allow(dead_code)]
fn truncate_sql(sql: &str, max_len: usize) -> String {
    if sql.len() <= max_len { sql.to_string() } else { format!("{}...", &sql[..max_len]) }
}

/// Parameters we query from cluster for smart recommendations
/// These are used to provide context-aware parameter suggestions
const CLUSTER_VARIABLE_NAMES: &[&str] = &[
    "query_mem_limit",
    "query_timeout",
    "enable_spill",
    "pipeline_dop",
    "parallel_fragment_exec_instance_num",
    "io_tasks_per_scan_operator",
    "enable_global_runtime_filter",
    "runtime_join_filter_push_down_limit",
    "enable_scan_datacache",
    "enable_populate_datacache",
    "enable_query_cache",
    "pipeline_profile_level",
];

/// Fetch relevant session variables from the cluster
///
/// Returns `None` if query fails (graceful degradation).
/// This allows analysis to continue even if variable fetching fails,
/// though parameter recommendations may be less accurate.
async fn fetch_cluster_variables(mysql_client: &MySQLClient) -> Option<ClusterVariables> {
    let sql = format!(
        "SHOW VARIABLES WHERE Variable_name IN ({})",
        CLUSTER_VARIABLE_NAMES
            .iter()
            .map(|name| format!("'{}'", name))
            .collect::<Vec<_>>()
            .join(",")
    );

    match mysql_client.query_raw(&sql).await {
        Ok((_, rows)) => {
            let mut variables = ClusterVariables::new();
            for row in rows {
                if row.len() >= 2 {
                    let var_name = row[0].clone();
                    let var_value = row[1].clone();
                    variables.insert(var_name, var_value);
                }
            }
            tracing::debug!(
                "Fetched {} cluster variables for smart recommendations",
                variables.len()
            );
            Some(variables)
        },
        Err(e) => {
            tracing::warn!(
                "Failed to fetch cluster variables: {}, analysis will continue without them",
                e
            );
            None
        },
    }
}
