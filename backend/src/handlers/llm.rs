//! LLM API Handlers
//!
//! REST API endpoints for LLM service management and analysis.

use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use stellar_macros::app_db;

use crate::AppState;
use crate::middleware::OrgContext;
use crate::services::llm::{
    CreateProviderRequest, LLMError, LLMProviderInfo, LLMService, UpdateProviderRequest,
};
use crate::services::profile_analyzer::analyzer::QueryComplexity;

// ============================================================================
// Provider Management APIs
// ============================================================================

#[app_db]
/// List all LLM providers
/// GET /api/llm/providers
pub async fn list_providers(
    State(state): State<Arc<AppState<DB>>>,
) -> Result<impl IntoResponse, LLMApiError> {
    let providers = state.llm_service.list_providers().await?;
    Ok(Json(providers))
}

#[app_db]
/// Get provider by ID
/// GET /api/llm/providers/:id
pub async fn get_provider(
    State(state): State<Arc<AppState<DB>>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state
        .llm_service
        .get_provider(id)
        .await?
        .ok_or(LLMError::ProviderNotFound(id.to_string()))?;
    Ok(Json(provider))
}

#[app_db]
/// Get active provider
/// GET /api/llm/providers/active
pub async fn get_active_provider(
    State(state): State<Arc<AppState<DB>>>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state.llm_service.get_active_provider().await?;
    Ok(Json(provider))
}

#[app_db]
/// Create a new provider
/// POST /api/llm/providers
pub async fn create_provider(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(req): Json<CreateProviderRequest>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state.llm_service.create_provider(req).await?;
    let provider = LLMProviderInfo::from(&provider);
    log_provider_audit(&state, &org_ctx, "create", &provider).await;
    Ok((StatusCode::CREATED, Json(provider)))
}

#[app_db]
/// Update a provider
/// PUT /api/llm/providers/:id
pub async fn update_provider(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state.llm_service.update_provider(id, req).await?;
    let provider = LLMProviderInfo::from(&provider);
    log_provider_audit(&state, &org_ctx, "update", &provider).await;
    Ok(Json(provider))
}

#[app_db]
/// Delete a provider
/// DELETE /api/llm/providers/:id
pub async fn delete_provider(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state
        .llm_service
        .get_provider(id)
        .await?
        .ok_or(LLMError::ProviderNotFound(id.to_string()))?;
    state.llm_service.delete_provider(id).await?;
    log_provider_audit(&state, &org_ctx, "delete", &provider).await;
    Ok(StatusCode::NO_CONTENT)
}

#[app_db]
/// Activate a provider
/// POST /api/llm/providers/:id/activate
pub async fn activate_provider(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, LLMApiError> {
    state.llm_service.activate_provider(id).await?;
    let provider = state
        .llm_service
        .get_provider(id)
        .await?
        .ok_or(LLMError::ProviderNotFound(id.to_string()))?;
    log_provider_audit(&state, &org_ctx, "activate", &provider).await;
    Ok(Json(provider))
}

#[app_db]
/// Deactivate a provider
/// POST /api/llm/providers/:id/deactivate
pub async fn deactivate_provider(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, LLMApiError> {
    state.llm_service.deactivate_provider(id).await?;
    let provider = state
        .llm_service
        .get_provider(id)
        .await?
        .ok_or(LLMError::ProviderNotFound(id.to_string()))?;
    log_provider_audit(&state, &org_ctx, "deactivate", &provider).await;
    Ok(Json(provider))
}

#[app_db]
/// Test connection to a provider
/// POST /api/llm/providers/:id/test
pub async fn test_provider_connection(
    State(state): State<Arc<AppState<DB>>>,
    Extension(org_ctx): Extension<OrgContext>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, LLMApiError> {
    let provider = state
        .llm_service
        .get_provider(id)
        .await?
        .ok_or(LLMError::ProviderNotFound(id.to_string()))?;
    let result = state.llm_service.test_connection(id).await?;
    log_provider_audit(&state, &org_ctx, "test_connection", &provider).await;
    Ok(Json(result))
}

#[app_db]
async fn log_provider_audit<DB: crate::db::AppDb>(
    state: &Arc<AppState<DB>>,
    org_ctx: &OrgContext,
    action: &'static str,
    provider: &LLMProviderInfo,
) {
    crate::services::op_audit::log_op_best_effort(
        &state.db,
        crate::services::op_audit::OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: None,
            action,
            target_type: "llm_provider",
            target_id: Some(provider.id),
            target_name: &provider.display_name,
        },
    )
    .await;
}

// ============================================================================
// Status API
// ============================================================================

#[app_db]
/// Get LLM service status
/// GET /api/llm/status
pub async fn get_status(
    State(state): State<Arc<AppState<DB>>>,
) -> Result<impl IntoResponse, LLMApiError> {
    let providers = state.llm_service.list_providers().await?;
    let active_provider = providers.iter().find(|p| p.is_active);

    Ok(Json(LLMStatusResponse {
        enabled: state.llm_service.is_available(),
        active_provider: active_provider.cloned(),
        provider_count: providers.len(),
    }))
}

#[derive(Serialize)]
pub struct LLMStatusResponse {
    pub enabled: bool,
    pub active_provider: Option<LLMProviderInfo>,
    pub provider_count: usize,
}

// ============================================================================
// Analysis API (for direct LLM calls)
// ============================================================================

#[app_db]
/// Request root cause analysis
/// POST /api/llm/analyze/root-cause
pub async fn analyze_root_cause(
    State(state): State<Arc<AppState<DB>>>,
    Json(req): Json<RootCauseAnalysisApiRequest>,
) -> Result<impl IntoResponse, LLMApiError> {
    use crate::models::cluster::ClusterType;
    use crate::services::llm::{
        ExecutionPlanForLLM, QuerySummaryForLLM, RootCauseAnalysisRequest,
        RootCauseAnalysisResponse,
    };

    let complexity = QueryComplexity::from_sql(&req.sql_statement);
    let cluster_type = req
        .cluster_type
        .as_ref()
        .map(|s| ClusterType::from_str_loose(s))
        .unwrap_or_default();

    let llm_request = RootCauseAnalysisRequest::builder()
        .query_summary(QuerySummaryForLLM {
            sql_statement: req.sql_statement.clone(),
            query_type: req.query_type.clone(),
            query_complexity: Some(format!("{:?}", complexity)),
            cluster_type,
            total_time_seconds: req.total_time_seconds,
            scan_bytes: req.scan_bytes,
            output_rows: req.output_rows,
            be_count: req.be_count,
            has_spill: req.has_spill,
            spill_bytes: None,
            session_variables: req.session_variables.clone().unwrap_or_default(),
        })
        .execution_plan(ExecutionPlanForLLM {
            dag_description: req.dag_description.clone(),
            hotspot_nodes: vec![],
        })
        .diagnostics(req.diagnostics.clone().unwrap_or_default())
        .key_metrics(req.key_metrics.clone().unwrap_or_default())
        .build()
        .map_err(|e| LLMError::ApiError(e.to_string()))?;

    let llm_result: crate::services::llm::LLMAnalysisResult<RootCauseAnalysisResponse> = state
        .llm_service
        .analyze(&llm_request, &req.query_id, req.cluster_id, false)
        .await?;

    Ok(Json(llm_result.response))
}

#[derive(Debug, Deserialize)]
pub struct RootCauseAnalysisApiRequest {
    pub query_id: String,
    #[serde(default)]
    pub cluster_id: Option<i64>,
    /// Cluster type: "starrocks" or "doris" (case-insensitive)
    #[serde(default)]
    pub cluster_type: Option<String>,
    pub sql_statement: String,
    pub query_type: String,
    pub total_time_seconds: f64,
    #[serde(default)]
    pub scan_bytes: u64,
    #[serde(default)]
    pub output_rows: u64,
    #[serde(default = "default_be_count")]
    pub be_count: u32,
    #[serde(default)]
    pub has_spill: bool,
    pub dag_description: String,
    #[serde(default)]
    pub session_variables: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub diagnostics: Option<Vec<crate::services::llm::DiagnosticForLLM>>,
    #[serde(default)]
    pub key_metrics: Option<KeyMetricsForLLM>,
}

fn default_be_count() -> u32 {
    3
}

#[allow(dead_code)]
fn truncate_sql(sql: &str, max_len: usize) -> String {
    if sql.len() <= max_len {
        sql.to_string()
    } else {
        format!("{}... (truncated)", &sql[..max_len])
    }
}

use crate::services::llm::KeyMetricsForLLM;

// ============================================================================
// Error Handling
// ============================================================================

pub struct LLMApiError(LLMError);

impl From<LLMError> for LLMApiError {
    fn from(err: LLMError) -> Self {
        Self(err)
    }
}

impl IntoResponse for LLMApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self.0 {
            LLMError::NoProviderConfigured => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            LLMError::ProviderNotFound(_) => (StatusCode::NOT_FOUND, self.0.to_string()),
            LLMError::Disabled => (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string()),
            LLMError::CredentialEncryptionUnavailable => {
                (StatusCode::SERVICE_UNAVAILABLE, self.0.to_string())
            },
            LLMError::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, self.0.to_string()),
            LLMError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.0.to_string()),
            LLMError::ApiError(_) => (StatusCode::BAD_GATEWAY, self.0.to_string()),
            LLMError::ParseError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()),
            LLMError::DatabaseError(e) => {
                tracing::error!("LLM database error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Database error: {}", e))
            },
            LLMError::SerializationError(e) => {
                tracing::error!("LLM serialization error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Serialization error: {}", e))
            },
        };

        let body = Json(serde_json::json!({
            "error": message,
            "code": status.as_u16(),
        }));

        (status, body).into_response()
    }
}
