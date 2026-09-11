use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;
use std::time::Duration;
use stellar_macros::app_db;

use crate::AppState;
use crate::models::Backend;
use crate::services::create_adapter;
use crate::services::metrics_collector_service::fill_backend_cache_capacity;
use crate::utils::ApiResult;

fn fill_storage(backends: &mut [Backend]) {
    for backend in backends {
        fill_backend_cache_capacity(backend);
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
    if let Some(mut backends) = state
        .metrics_collector_service
        .cached_backends(cluster_id, Duration::from_secs(90))
        .or_else(|| state.metrics_collector_service.stale_backends(cluster_id))
    {
        fill_storage(&mut backends);
        return Ok(Json(backends));
    }
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let mut backends = adapter.get_backends().await?;
    state.metrics_collector_service.store_backends(cluster_id, backends.clone());
    fill_storage(&mut backends);
    Ok(Json(backends))
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
    state.metrics_collector_service.invalidate_backends(cluster_id);

    Ok(Json(serde_json::json!({
        "message": format!("Backend {}:{} deleted successfully", host, port)
    })))
}
