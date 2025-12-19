use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;

use crate::{
    services::{create_adapter, mysql_client::MySQLClient},
    utils::error::{ApiError, ApiResult},
};

/// Get all sessions (connections) for a cluster
#[utoipa::path(
    get,
    path = "/api/clusters/sessions",
    responses(
        (status = 200, description = "Sessions list", body = Vec<Session>),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer" = [])
    )
)]
pub async fn get_sessions(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let sessions = adapter.get_sessions().await?;

    Ok(Json(sessions))
}

/// Kill a session (connection)
#[utoipa::path(
    delete,
    path = "/api/clusters/sessions/{session_id}",
    params(
        ("session_id" = String, Path, description = "Session/Connection ID")
    ),
    responses(
        (status = 200, description = "Session killed successfully"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer" = [])
    )
)]
pub async fn kill_session(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(session_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);

    kill_session_via_starrocks(&mysql_client, &session_id).await?;

    Ok((StatusCode::OK, Json(json!({ "message": "Session killed successfully" }))))
}


async fn kill_session_via_starrocks(mysql_client: &MySQLClient, session_id: &str) -> ApiResult<()> {
    tracing::info!("Killing session: {}", session_id);

    let kill_sql = format!("KILL CONNECTION {}", session_id);

    match mysql_client.execute(&kill_sql).await {
        Ok(_) => {
            tracing::info!("Successfully killed session: {}", session_id);
            Ok(())
        },
        Err(e) => {
            tracing::warn!("KILL CONNECTION failed, trying KILL: {:?}", e);

            let fallback_sql = format!("KILL {}", session_id);
            mysql_client.execute(&fallback_sql).await.map_err(|err| {
                tracing::error!("Failed to kill session {}: {:?}", session_id, err);
                ApiError::cluster_connection_failed(format!("Failed to kill session: {:?}", err))
            })?;
            Ok(())
        },
    }
}
