use std::sync::Arc;

use axum::{Extension, Json, extract::State, http::StatusCode};

use crate::{
    AppState,
    middleware::OrgContext,
    models::{CreatePhysicalHostRequest, CreateSrPackageRequest, PhysicalHost, SrPackage},
    utils::{ApiError, ApiResult},
};

#[utoipa::path(
    get,
    path = "/api/sr-ops/hosts",
    responses(
        (status = 200, description = "Physical host inventory", body = Vec<PhysicalHost>)
    ),
    security(("bearer_auth" = [])),
    tag = "Physical Deployment"
)]
pub async fn list_hosts(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
) -> ApiResult<Json<Vec<PhysicalHost>>> {
    let organization_id = (!org_ctx.is_super_admin)
        .then_some(org_ctx.organization_id)
        .flatten();
    let hosts = state
        .physical_host_service
        .list_hosts(organization_id)
        .await?;
    Ok(Json(hosts))
}

#[utoipa::path(
    post,
    path = "/api/sr-ops/hosts",
    request_body = CreatePhysicalHostRequest,
    responses(
        (status = 201, description = "Physical host registered", body = PhysicalHost),
        (status = 400, description = "Invalid host or organization"),
        (status = 401, description = "Not authorized to manage deployment hosts")
    ),
    security(("bearer_auth" = [])),
    tag = "Physical Deployment"
)]
pub async fn create_host(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(request): Json<CreatePhysicalHostRequest>,
) -> ApiResult<(StatusCode, Json<PhysicalHost>)> {
    let organization_id = resolve_organization_id(&org_ctx, request.organization_id)?;
    ensure_organization_admin(&state, &org_ctx, organization_id).await?;

    tracing::info!(
        user_id = org_ctx.user_id,
        organization_id,
        hostname = %request.hostname,
        ssh_target = %request.ssh_target,
        "Registering physical deployment host"
    );

    let host = state
        .physical_host_service
        .create_host(request, organization_id)
        .await?;
    Ok((StatusCode::CREATED, Json(host)))
}

#[utoipa::path(
    get,
    path = "/api/sr-ops/packages",
    responses(
        (status = 200, description = "Controlled StarRocks package inventory", body = Vec<SrPackage>)
    ),
    security(("bearer_auth" = [])),
    tag = "Physical Deployment"
)]
pub async fn list_packages(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
) -> ApiResult<Json<Vec<SrPackage>>> {
    let organization_id = (!org_ctx.is_super_admin)
        .then_some(org_ctx.organization_id)
        .flatten();
    let packages = state.package_service.list_packages(organization_id).await?;
    Ok(Json(packages))
}

#[utoipa::path(
    post,
    path = "/api/sr-ops/packages",
    request_body = CreateSrPackageRequest,
    responses(
        (status = 201, description = "Controlled StarRocks package registered", body = SrPackage),
        (status = 400, description = "Invalid package metadata or organization"),
        (status = 401, description = "Not authorized to manage deployment packages")
    ),
    security(("bearer_auth" = [])),
    tag = "Physical Deployment"
)]
pub async fn create_package(
    State(state): State<Arc<AppState>>,
    Extension(org_ctx): Extension<OrgContext>,
    Json(request): Json<CreateSrPackageRequest>,
) -> ApiResult<(StatusCode, Json<SrPackage>)> {
    let organization_id = resolve_organization_id(&org_ctx, request.organization_id)?;
    ensure_organization_admin(&state, &org_ctx, organization_id).await?;

    tracing::info!(
        user_id = org_ctx.user_id,
        organization_id,
        version = %request.version,
        "Registering controlled StarRocks package"
    );

    let package = state
        .package_service
        .create_package(request, organization_id)
        .await?;
    Ok((StatusCode::CREATED, Json(package)))
}

fn resolve_organization_id(
    org_ctx: &OrgContext,
    requested_organization_id: Option<i64>,
) -> ApiResult<i64> {
    if org_ctx.is_super_admin {
        return requested_organization_id
            .filter(|organization_id| *organization_id > 0)
            .ok_or_else(|| {
                ApiError::validation_error("organization_id is required for super administrators")
            });
    }

    let organization_id = org_ctx
        .organization_id
        .ok_or_else(|| ApiError::forbidden("A current organization is required to manage hosts"))?;
    if let Some(requested_organization_id) = requested_organization_id
        && requested_organization_id != organization_id
    {
        return Err(ApiError::forbidden(
            "Organization administrators cannot register hosts for another organization",
        ));
    }

    Ok(organization_id)
}

async fn ensure_organization_admin(
    state: &AppState,
    org_ctx: &OrgContext,
    organization_id: i64,
) -> ApiResult<()> {
    if org_ctx.is_super_admin {
        return Ok(());
    }

    let is_organization_admin: i64 = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM user_roles ur
            JOIN roles r ON r.id = ur.role_id
            WHERE ur.user_id = ?
              AND r.organization_id = ?
              AND r.code LIKE 'org_admin_%'
        )
        "#,
    )
    .bind(org_ctx.user_id)
    .bind(organization_id)
    .fetch_one(&state.db)
    .await?;

    if is_organization_admin == 0 {
        return Err(ApiError::forbidden(
            "Only organization administrators can register deployment hosts",
        ));
    }

    Ok(())
}
