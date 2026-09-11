use axum::{Json, extract::State};
use std::sync::Arc;
use std::time::Duration;
use stellar_macros::app_db;

use crate::AppState;
use crate::models::Frontend;
use crate::services::create_adapter;
use crate::utils::ApiResult;

// Get all frontends for a cluster
#[utoipa::path(
    get,
    path = "/api/clusters/frontends",
    responses(
        (status = 200, description = "List of frontend nodes", body = Vec<Frontend>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Frontends"
)]
#[app_db]
pub async fn list_frontends(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<Frontend>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    let cluster_id = cluster.id;
    if let Some(frontends) = state
        .metrics_collector_service
        .cached_frontends(cluster_id, Duration::from_secs(90))
        .or_else(|| state.metrics_collector_service.stale_frontends(cluster_id))
    {
        return Ok(Json(frontends));
    }
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let frontends = adapter.get_frontends().await?;
    state
        .metrics_collector_service
        .store_frontends(cluster_id, frontends.clone());
    Ok(Json(frontends))
}
