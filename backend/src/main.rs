use axum::{
    Router,
    body::Body,
    extract::DefaultBodyLimit,
    http::{HeaderValue, StatusCode, Uri, header},
    middleware as axum_middleware,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use sqlx::{Database, Executor, IntoArguments, MySql, Postgres, Sqlite};
use stellar::config::Config;
use stellar::db::{self, AppDb};
use stellar::embedded::WebAssets;
use stellar::models;
use stellar::services::{
    AgentRuntimeService, AuditLogService, AuthService, CasbinService, ClusterService,
    DataStatisticsService, DbAuthQueryService, LLMServiceImpl, MetricsCollectorService,
    MySQLPoolManager, NotificationService, OpsAgentService, OrganizationService, OverviewService,
    PermissionRequestService, PermissionService, RoleService, SystemFunctionService,
    UserRoleService, UserService,
};
use stellar::utils::{JwtUtil, ScheduledExecutor};
use stellar::{AppState, handlers, middleware, services};

#[derive(OpenApi)]
#[openapi(
    paths(

        handlers::auth::register,
        handlers::auth::login,
        handlers::auth::get_me,
        handlers::auth::update_me,

        handlers::cluster::create_cluster,
        handlers::cluster::list_clusters,
        handlers::cluster::get_active_cluster,
        handlers::cluster::get_cluster,
        handlers::cluster::update_cluster,
        handlers::cluster::delete_cluster,
        handlers::cluster::activate_cluster,

        handlers::organization::create_organization,
        handlers::organization::list_organizations,
        handlers::organization::get_organization,
        handlers::organization::update_organization,
        handlers::organization::delete_organization,
        handlers::cluster::get_cluster_health,

        handlers::backend::list_backends,
        handlers::frontend::list_frontends,

        handlers::materialized_view::list_materialized_views,
        handlers::materialized_view::get_materialized_view,
        handlers::materialized_view::get_materialized_view_ddl,
        handlers::materialized_view::create_materialized_view,
        handlers::materialized_view::delete_materialized_view,
        handlers::materialized_view::refresh_materialized_view,
        handlers::materialized_view::cancel_refresh_materialized_view,
        handlers::materialized_view::alter_materialized_view,

        handlers::query::list_catalogs,
        handlers::query::list_databases,
        handlers::query::list_catalogs_with_databases,
        handlers::query::list_queries,
        handlers::query::kill_query,
        handlers::query::execute_sql,
        handlers::query::list_sql_blacklist,
        handlers::query::add_sql_blacklist,
        handlers::query::delete_sql_blacklist,
        handlers::query_history::list_query_history,
        handlers::query_execution_history::list_execution_history,
        handlers::query_execution_history::delete_execution_history,
        handlers::query_execution_history::clear_execution_history,
        handlers::load::list_loads,
        handlers::load::get_load,
        handlers::load::execute_load_action,
        handlers::load::stream_load,

        handlers::sessions::get_sessions,
        handlers::sessions::kill_session,
        handlers::variables::get_variables,
        handlers::variables::update_variable,

        handlers::profile::list_profiles,
        handlers::profile::get_profile,
        handlers::profile::get_profile_retest,
        handlers::profile::analyze_profile_handler,

        handlers::system_management::get_system_functions,
        handlers::system_management::get_system_function_detail,
        handlers::system::get_runtime_info,
        handlers::log_archive::download,

        handlers::overview::get_cluster_overview,
        handlers::overview::get_health_cards,
        handlers::overview::get_performance_trends,
        handlers::overview::get_resource_trends,
        handlers::overview::get_data_statistics,
        handlers::overview::get_capacity_prediction,
        handlers::overview::get_extended_cluster_overview,
        handlers::cluster::test_cluster_connection,

        handlers::role::list_roles,
        handlers::role::get_role,
        handlers::role::create_role,
        handlers::role::update_role,        // Role
        handlers::role::delete_role,
        handlers::role::get_role_with_permissions,
        handlers::role::update_role_permissions,

        handlers::permission::list_permissions,
        handlers::permission::list_menu_permissions,
        handlers::permission::list_api_permissions,
        handlers::permission::get_permission_tree,
        handlers::permission::get_current_user_permissions,

        handlers::user_role::get_user_roles,
        handlers::user_role::assign_role_to_user,
        handlers::user_role::remove_role_from_user,

        handlers::user::list_users,
        handlers::user::get_user,
        handlers::user::create_user,
        handlers::user::update_user,
        handlers::user::delete_user,

        handlers::permission_request::list_my_requests,
        handlers::permission_request::list_pending_approvals,
        handlers::permission_request::get_request,
        handlers::permission_request::submit_request,
        handlers::permission_request::approve_request,
        handlers::permission_request::reject_request,
        handlers::permission_request::cancel_request,
        handlers::permission_request::list_db_accounts,
        handlers::permission_request::list_db_roles,
        handlers::permission_request::list_db_accounts_active,
        handlers::permission_request::list_db_roles_active,
        handlers::permission_request::preview_sql,

        handlers::resource_group::list_resource_groups,
        handlers::resource_group::get_resource_group,
        handlers::resource_group::create_resource_group,
        handlers::resource_group::update_resource_group,
        handlers::resource_group::delete_resource_group,
        handlers::resource_group::get_resource_group_usage,
        handlers::resource_group::analyze_resource_usage,
    ),
    components(
        schemas(
            models::User,
            models::UserResponse,
            models::UserWithRolesResponse,
            models::CreateUserRequest,
            models::AdminCreateUserRequest,
            models::LoginRequest,
            models::LoginResponse,
            models::AdminUpdateUserRequest,
            models::Cluster,
            models::ClusterResponse,
            models::CreateClusterRequest,
            models::UpdateClusterRequest,
            models::ClusterHealth,
            models::HealthStatus,
            models::HealthCheck,
            models::Backend,
            models::Frontend,
            models::MaterializedView,
            models::CreateMaterializedViewRequest,
            models::RefreshMaterializedViewRequest,
            models::AlterMaterializedViewRequest,
            models::MaterializedViewDDL,
            models::Query,
            models::QueryExecuteRequest,
            models::QueryExecuteResponse,
            models::CatalogWithDatabases,
            models::CatalogsWithDatabasesResponse,
            models::QueryHistoryItem,
            models::QueryHistoryResponse,
            models::QueryExecutionHistory,
            models::QueryExecutionHistoryResponse,
            models::LoadQueryParams,
            models::LoadStage,
            models::LoadFailureCause,
            models::LoadAction,
            models::LoadActionRequest,
            models::LoadActionResponse,
            models::LoadJob,
            models::LoadSummary,
            models::LoadListResponse,
            models::StreamLoadResponse,
            models::ProfileListItem,
            models::ProfileDetail,
            models::ProfileRetestRun,
            models::ProfileRetestResponse,
            models::RuntimeInfo,
            models::MetricsSummary,
            models::SystemFunction,
            models::CreateFunctionRequest,
            models::UpdateOrderRequest,
            models::FunctionOrder,
            models::Role,
            models::RoleResponse,
            models::CreateRoleRequest,
            models::UpdateRoleRequest,
            models::RoleWithPermissions,
            models::Permission,
            models::PermissionResponse,
            models::PermissionTree,
            models::UpdateRolePermissionsRequest,
            models::AssignUserRoleRequest,
            services::ClusterOverview,
            services::ExtendedClusterOverview,
            services::HealthCard,
            services::HealthStatus,
            services::ClusterHealth,
            services::KeyPerformanceIndicators,
            services::ResourceMetrics,
            services::MaterializedViewStats,
            services::LoadJobStats,
            services::TransactionStats,
            services::SchemaChangeStats,
            services::CompactionStats,
            services::BECompactionScore,
            services::CompactionDetailStats,
            services::TopPartitionByScore,
            services::CompactionTaskStats,
            services::CompactionDurationStats,
            services::SessionStats,
            services::RunningQuery,
            services::NetworkIOStats,
            services::Alert,
            services::AlertLevel,
            services::PerformanceTrends,
            services::ResourceTrends,
            services::MetricsSnapshot,
            services::ClusterResourceSummary,
            services::DataStatistics,
            services::TopTableBySize,
            services::TopTableByAccess,
            services::CapacityPrediction,
            models::PermissionRequest,
            models::PermissionRequestResponse,
            models::SubmitRequestDto,
            models::ApprovalDto,
            models::RequestDetails,
            models::RequestQueryFilter,
            models::PaginatedResponse::<models::PermissionRequestResponse>,
            models::DbAccountDto,
            models::DbRoleDto,
            models::ResourceGroup,
            models::Classifier,
            models::CreateResourceGroupRequest,
            models::UpdateResourceGroupRequest,
            models::ClassifierRequest,
            models::ResourceGroupUsage,
            models::ResourceUsageAnalysis,
            models::UserCpuUsage,
            models::UserMemoryUsage,
            models::UserConcurrency,
        )
    ),
    tags(
        (name = "Authentication", description = "User authentication endpoints"),
        (name = "Clusters", description = "Cluster management endpoints"),
        (name = "Backends", description = "Backend node management"),
        (name = "Frontends", description = "Frontend node management"),
        (name = "Materialized Views", description = "Materialized view management"),
        (name = "Queries", description = "Query management"),
        (name = "Load Management", description = "Load task management"),
        (name = "Profiles", description = "Query profile management"),
        (name = "System", description = "System information"),
        (name = "Roles", description = "Role management"),
        (name = "Permissions", description = "Permission management"),
        (name = "Users", description = "User role management"),
        (name = "Permission Requests", description = "Permission request workflow"),
        (name = "Database Authentication", description = "Database account and role management"),
        (name = "Resource Groups", description = "Resource group management"),
    ),
    modifiers(&SecurityAddon)
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.as_mut().unwrap();
        components.add_security_scheme(
            "bearer_auth",
            utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
                utoipa::openapi::security::HttpAuthScheme::Bearer,
            )),
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;

    // Zero-config mode: persist a generated JWT secret so restarts keep
    // sessions valid. Explicitly provided secrets always win.
    if let Some(secret) = stellar::db::bootstrap::ensure_jwt_secret(
        config.data_dir.as_deref(),
        &config.auth.jwt_secret,
    )? {
        config.auth.jwt_secret = secret;
    }

    // The guard must outlive every log call, so it stays bound in `main` for the
    // whole process lifetime.
    let _log_guard = init_logging(&config.logging);

    tracing::info!("Stellar starting up");
    tracing::info!("Configuration loaded successfully");

    // 按连接串协议选择数据库后端，同一二进制运行时双后端支持
    let kind = stellar::db::DatabaseKind::from_url(&config.database.url)?;
    match kind {
        stellar::db::DatabaseKind::Sqlite => run::<Sqlite>(config).await?,
        stellar::db::DatabaseKind::MySql => run::<MySql>(config).await?,
        stellar::db::DatabaseKind::Postgres => run::<Postgres>(config).await?,
    }

    Ok(())
}

/// Initializes tracing with optional daily-rotated file logging.
///
/// The returned guard owns the non-blocking writer thread: dropping it stops the
/// writer and silently discards every later log record, so the caller must keep
/// it alive for the entire process. Returns `None` when logging goes to stdout
/// only (no `logging.file`, or an unwritable log directory).
fn init_logging(
    logging: &stellar::config::LoggingConfig,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_filter = tracing_subscriber::EnvFilter::new(&logging.level);
    let registry = tracing_subscriber::registry().with(log_filter);

    let Some(log_file) = &logging.file else {
        registry.with(tracing_subscriber::fmt::layer()).init();
        return None;
    };

    let log_path = std::path::Path::new(log_file);
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let log_dir = log_path.parent().and_then(|p| p.to_str()).unwrap_or("logs");
    let file_name = log_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("stellar.log");
    let file_prefix = file_name.strip_suffix(".log").unwrap_or(file_name);

    // Unwritable log dir must degrade to stdout, never panic: services run
    // as dedicated users from arbitrary cwds (e.g. systemd) where creating
    // ./logs may be denied. Probe-write before initializing the appender.
    let probe = format!("{log_dir}/.stellar-write-test-{}", std::process::id());
    let probe_path = std::path::Path::new(&probe);
    if std::fs::write(probe_path, b"")
        .and_then(|_| std::fs::remove_file(probe_path))
        .is_err()
    {
        eprintln!(
            "[stellar] WARNING: log directory '{log_dir}' is not writable, falling back to stdout logging"
        );
        registry.with(tracing_subscriber::fmt::layer()).init();
        return None;
    }

    let file_appender = tracing_appender::rolling::daily(log_dir, file_prefix);
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    registry
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("File logging enabled: {log_dir}/{file_prefix}.<YYYY-MM-DD> (daily rotation)");
    Some(guard)
}

/// 泛型应用主流程：DB 由 main 的分发处具体化，全部代码路径在此单态化。
///（bin crate 中宏的 `crate::` 路径不可用，此处手写完整能力 bound）
#[allow(clippy::too_many_lines)]
async fn run<DB: AppDb>(config: Config) -> Result<(), Box<dyn std::error::Error>>
where
    for<'q> <DB as AppDb>::Query<'q>: Send,
    for<'c> &'c mut <DB as Database>::Connection: Executor<'c, Database = DB> + Send,
    for<'q> <DB as Database>::Arguments<'q>: IntoArguments<'q, DB> + Default,
    usize: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'a> &'a str: sqlx::ColumnIndex<<DB as Database>::Row>,
    for<'q> i64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> i32: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> f64: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> bool: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> &'q str: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> DateTime<Utc>: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> NaiveDateTime: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> NaiveDate: sqlx::Encode<'q, DB> + sqlx::Decode<'q, DB> + sqlx::Type<DB>,
    for<'q> Option<String>: sqlx::Encode<'q, DB>,
    for<'q> Option<i64>: sqlx::Encode<'q, DB>,
    for<'q> Option<i32>: sqlx::Encode<'q, DB>,
    for<'q> Option<f64>: sqlx::Encode<'q, DB>,
    for<'q> Option<bool>: sqlx::Encode<'q, DB>,
    for<'q> Option<DateTime<Utc>>: sqlx::Encode<'q, DB>,
    for<'q> Option<&'q str>: sqlx::Encode<'q, DB>,
    for<'q> Option<NaiveDateTime>: sqlx::Encode<'q, DB>,
    for<'q> Option<NaiveDate>: sqlx::Encode<'q, DB>,
    for<'q> stellar::models::cluster::ClusterType:
        sqlx::Type<DB> + sqlx::Decode<'q, DB> + sqlx::Encode<'q, DB>,
    for<'q> stellar::models::cluster::DeploymentMode:
        sqlx::Type<DB> + sqlx::Decode<'q, DB> + sqlx::Encode<'q, DB>,
    for<'q> Option<&'q str>: sqlx::Encode<'q, DB>,
    for<'q> Option<DateTime<Utc>>: sqlx::Encode<'q, DB>,
{
    let pool = db::create_pool::<DB>(&config.database.url).await?;
    tracing::info!("Database pool created successfully");

    // Bootstrap behavior is explicitly selected by STELLAR_ENV. Existing
    // initialized accounts are never changed automatically.
    if db::bootstrap::ensure_root_user::<DB>(
        &pool,
        config.runtime_mode,
        std::env::var("STELLAR_ROOT_PASSWORD").ok(),
    )
    .await?
    .is_some()
    {
        tracing::info!(runtime_mode = ?config.runtime_mode, "Initial admin credential initialized");
    }

    let jwt_util = Arc::new(JwtUtil::new(&config.auth.jwt_secret, &config.auth.jwt_expires_in));
    let mysql_pool_manager = Arc::new(MySQLPoolManager::new());

    let auth_service = Arc::new(AuthService::new(pool.clone(), Arc::clone(&jwt_util)));

    let cluster_service =
        Arc::new(ClusterService::new(pool.clone(), Arc::clone(&mysql_pool_manager)));

    let organization_service = Arc::new(OrganizationService::new(pool.clone()));

    let system_function_service = Arc::new(SystemFunctionService::new(
        Arc::new(pool.clone()),
        Arc::clone(&mysql_pool_manager),
        Arc::clone(&cluster_service),
    ));

    let metrics_collector_service = Arc::new(MetricsCollectorService::new(
        pool.clone(),
        Arc::clone(&cluster_service),
        Arc::clone(&mysql_pool_manager),
        config.audit.clone(),
        config.metrics.retention_days,
    ));

    let data_statistics_service = Arc::new(DataStatisticsService::new(
        pool.clone(),
        Arc::clone(&cluster_service),
        Arc::clone(&mysql_pool_manager),
        config.audit.clone(),
    ));

    let overview_service = Arc::new(
        OverviewService::new(
            pool.clone(),
            Arc::clone(&cluster_service),
            Arc::clone(&mysql_pool_manager),
        )
        .with_data_statistics(Arc::clone(&data_statistics_service)),
    );

    let casbin_service = Arc::new(
        CasbinService::new()
            .await
            .map_err(|e| format!("Failed to initialize Casbin service: {}", e))?,
    );

    casbin_service
        .reload_policies_from_db(&pool)
        .await
        .map_err(|e| format!("Failed to load initial policies: {}", e))?;
    tracing::info!("Casbin policies loaded from database");

    let permission_service =
        Arc::new(PermissionService::new(pool.clone(), Arc::clone(&casbin_service)));

    let role_service = Arc::new(RoleService::new(
        pool.clone(),
        Arc::clone(&casbin_service),
        Arc::clone(&permission_service),
    ));

    let user_role_service =
        Arc::new(UserRoleService::new(pool.clone(), Arc::clone(&casbin_service)));

    let user_service = Arc::new(UserService::new(pool.clone(), Arc::clone(&casbin_service)));

    let llm_service = Arc::new(LLMServiceImpl::with_encryption_key(
        pool.clone(),
        true,
        24,
        &config.security.llm_provider_encryption_key,
    ));
    let migrated_credentials = llm_service
        .migrate_legacy_credentials()
        .await
        .map_err(|e| format!("Failed to secure existing AI connections: {e}"))?;
    if migrated_credentials > 0 {
        tracing::info!(migrated_credentials, "Encrypted legacy AI connection credentials");
    }
    tracing::info!("LLM service initialized");

    let db_auth_query_service = Arc::new(DbAuthQueryService::new(
        Arc::clone(&mysql_pool_manager),
        Arc::clone(&cluster_service),
    ));
    tracing::info!("DbAuthQueryService initialized with real cluster query support");

    let permission_request_service = Arc::new(PermissionRequestService::new(
        pool.clone(),
        &config.security.permission_request_encryption_key,
    ));
    tracing::info!("PermissionRequestService initialized");

    let profile_analysis_cache =
        Arc::new(stellar::services::profile_analyzer::ProfileAnalysisCache::new());

    let notification_service = Arc::new(NotificationService::new(pool.clone()));
    let ops_agent_service = Arc::new(OpsAgentService::new(
        pool.clone(),
        Arc::clone(&mysql_pool_manager),
        config.audit.clone(),
        Arc::clone(&cluster_service),
        Arc::clone(&metrics_collector_service),
        &config.security.llm_provider_encryption_key,
    ));
    tracing::info!("OpsAgentService initialized");

    let audit_service =
        Arc::new(AuditLogService::new(Arc::clone(&mysql_pool_manager), config.audit.clone()));
    let agent_runtime_service = Arc::new(AgentRuntimeService::new(
        pool.clone(),
        Arc::clone(&mysql_pool_manager),
        Arc::clone(&cluster_service),
        audit_service,
        config.agent.clone(),
        &config.security.llm_provider_encryption_key,
    ));
    tracing::info!("AgentRuntimeService initialized");

    let app_state = AppState {
        db: pool.clone(),
        mysql_pool_manager: Arc::clone(&mysql_pool_manager),
        jwt_util: Arc::clone(&jwt_util),
        audit_config: config.audit.clone(),
        log_file: config.logging.file.clone().map(std::path::PathBuf::from),
        auth_service: Arc::clone(&auth_service),
        cluster_service: Arc::clone(&cluster_service),
        organization_service: Arc::clone(&organization_service),
        system_function_service: Arc::clone(&system_function_service),
        metrics_collector_service: Arc::clone(&metrics_collector_service),
        data_statistics_service: Arc::clone(&data_statistics_service),
        overview_service: Arc::clone(&overview_service),
        casbin_service: Arc::clone(&casbin_service),
        permission_service: Arc::clone(&permission_service),
        role_service: Arc::clone(&role_service),
        user_role_service: Arc::clone(&user_role_service),
        user_service: Arc::clone(&user_service),
        llm_service: Arc::clone(&llm_service),
        db_auth_query_service: Arc::clone(&db_auth_query_service),
        permission_request_service: Arc::clone(&permission_request_service),
        profile_analysis_cache,
        ops_agent_service,
        notification_service,
        agent_runtime_service: Arc::clone(&agent_runtime_service),
    };

    if config.metrics.enabled {
        let interval = std::time::Duration::from_secs(config.metrics.interval_secs);
        tracing::info!(
            "Starting metrics collector with interval: {}s (retention_days={})",
            config.metrics.interval_secs,
            config.metrics.retention_days
        );
        let executor = ScheduledExecutor::new("metrics-collector", interval);
        let service = Arc::clone(&metrics_collector_service);
        tokio::spawn(async move {
            let warmup = Arc::clone(&service);
            tokio::spawn(async move {
                if let Err(e) = warmup.collect_once().await {
                    tracing::error!("Initial metrics collection failed: {}", e);
                }
            });
            executor.start(service).await;
        });
    } else {
        tracing::warn!("Metrics collector disabled by configuration");
    }

    let _baseline_refresh_handle = services::start_baseline_refresh_task(
        Arc::clone(&mysql_pool_manager),
        Arc::clone(&cluster_service),
        config.audit.clone(),
        3600,
    );
    tracing::info!("Baseline refresh task started (interval: 1 hour)");

    if config.agent.enabled {
        let interval = std::time::Duration::from_secs(config.agent.interval_secs);
        tracing::info!("Agent runtime loop started with interval: {}s", config.agent.interval_secs);
        let executor = ScheduledExecutor::new("agent-runtime", interval);
        let service = Arc::clone(&agent_runtime_service);
        tokio::spawn(async move {
            if let Err(e) = service.run_once().await {
                tracing::error!("Agent runtime initial tick failed: {}", e);
            }
            executor.start(service).await;
        });
    } else {
        tracing::warn!("Agent runtime loop disabled by configuration");
    }

    let app_state_arc = Arc::new(app_state);

    let auth_state = middleware::AuthState {
        jwt_util: Arc::clone(&jwt_util),
        casbin_service: Arc::clone(&casbin_service),
        db: pool.clone(),
    };

    let public_routes = Router::new()
        .route("/api/auth/register", post(handlers::auth::register))
        .route("/api/auth/login", post(handlers::auth::login))
        .with_state(Arc::clone(&app_state_arc));

    let protected_routes = Router::new()
        .route("/api/auth/me", get(handlers::auth::get_me))
        .route("/api/auth/me", put(handlers::auth::update_me))
        .route("/api/clusters", post(handlers::cluster::create_cluster))
        .route("/api/clusters", get(handlers::cluster::list_clusters))
        .route("/api/clusters/active", get(handlers::cluster::get_active_cluster))
        .route("/api/clusters/health/test", post(handlers::cluster::test_cluster_connection))
        .route("/api/clusters/backends", get(handlers::backend::list_backends))
        .route("/api/clusters/backends/:host/:port", delete(handlers::backend::delete_backend))
        .route("/api/clusters/frontends", get(handlers::frontend::list_frontends))
        .route("/api/clusters/catalogs", get(handlers::query::list_catalogs))
        .route("/api/clusters/databases", get(handlers::query::list_databases))
        .route("/api/clusters/tables", get(handlers::query::list_tables))
        .route(
            "/api/clusters/catalogs-databases",
            get(handlers::query::list_catalogs_with_databases),
        )
        .route("/api/clusters/queries", get(handlers::query::list_queries))
        .route("/api/clusters/loads", get(handlers::load::list_loads))
        .route("/api/clusters/loads/:job_id", get(handlers::load::get_load))
        .route("/api/clusters/loads/:job_id/actions", post(handlers::load::execute_load_action))
        .route("/api/clusters/queries/execute", post(handlers::query::execute_sql))
        .route(
            "/api/clusters/queries/stream-load",
            post(handlers::load::stream_load).layer(DefaultBodyLimit::max(
                (services::stream_load::MAX_STREAM_LOAD_BYTES + 64 * 1024) as usize,
            )),
        )
        .route("/api/clusters/queries/cancel", post(handlers::query::cancel_running_query))
        .route("/api/clusters/queries/:query_id", delete(handlers::query::kill_query))
        .route("/api/clusters/queries/history", get(handlers::query_history::list_query_history))
        .route(
            "/api/clusters/queries/execution-history",
            get(handlers::query_execution_history::list_execution_history)
                .delete(handlers::query_execution_history::clear_execution_history),
        )
        .route(
            "/api/clusters/queries/execution-history/:id",
            delete(handlers::query_execution_history::delete_execution_history),
        )
        .route(
            "/api/clusters/sql-blacklist",
            get(handlers::query::list_sql_blacklist).post(handlers::query::add_sql_blacklist),
        )
        .route("/api/clusters/sql-blacklist/:id", delete(handlers::query::delete_sql_blacklist))
        .route("/api/clusters/:cluster_id/sql/diagnose", post(handlers::sql_diag::diagnose))
        .route("/api/clusters/:id", get(handlers::cluster::get_cluster))
        .route("/api/clusters/:id", put(handlers::cluster::update_cluster))
        .route("/api/clusters/:id", delete(handlers::cluster::delete_cluster))
        .route("/api/clusters/:id/activate", put(handlers::cluster::activate_cluster))
        .route(
            "/api/clusters/:id/health",
            get(handlers::cluster::get_cluster_health).post(handlers::cluster::get_cluster_health),
        )
        .route(
            "/api/organizations",
            post(handlers::organization::create_organization)
                .get(handlers::organization::list_organizations),
        )
        .route(
            "/api/organizations/:id",
            get(handlers::organization::get_organization)
                .put(handlers::organization::update_organization)
                .delete(handlers::organization::delete_organization),
        )
        .route(
            "/api/clusters/materialized_views",
            get(handlers::materialized_view::list_materialized_views)
                .post(handlers::materialized_view::create_materialized_view),
        )
        .route(
            "/api/clusters/materialized_views/:mv_name",
            get(handlers::materialized_view::get_materialized_view)
                .delete(handlers::materialized_view::delete_materialized_view)
                .put(handlers::materialized_view::alter_materialized_view),
        )
        .route(
            "/api/clusters/materialized_views/:mv_name/ddl",
            get(handlers::materialized_view::get_materialized_view_ddl),
        )
        .route(
            "/api/clusters/materialized_views/:mv_name/refresh",
            post(handlers::materialized_view::refresh_materialized_view),
        )
        .route(
            "/api/clusters/materialized_views/:mv_name/cancel",
            post(handlers::materialized_view::cancel_refresh_materialized_view),
        )
        .route("/api/clusters/profiles", get(handlers::profile::list_profiles))
        .route("/api/clusters/profiles/:query_id", get(handlers::profile::get_profile))
        .route(
            "/api/clusters/profiles/:query_id/retest",
            get(handlers::profile::get_profile_retest),
        )
        .route(
            "/api/clusters/profiles/:query_id/analyze",
            get(handlers::profile::analyze_profile_handler),
        )
        .route(
            "/api/clusters/:cluster_id/profiles/:query_id/enhance",
            post(handlers::profile::enhance_profile_handler),
        )
        .route("/api/clusters/sessions", get(handlers::sessions::get_sessions))
        .route("/api/clusters/sessions/:session_id", delete(handlers::sessions::kill_session))
        .route("/api/clusters/variables", get(handlers::variables::get_variables))
        .route("/api/clusters/configs", get(handlers::variables::get_configure_info))
        .route("/api/clusters/variables/:variable_name", put(handlers::variables::update_variable))
        .route("/api/clusters/system/runtime_info", get(handlers::system::get_runtime_info))
        .route("/api/clusters/system", get(handlers::system_management::get_system_functions))
        .route(
            "/api/clusters/system/:function_name",
            get(handlers::system_management::get_system_function_detail),
        )
        .route(
            "/api/clusters/system-functions",
            get(handlers::system_function::get_system_functions)
                .post(handlers::system_function::create_system_function),
        )
        .route(
            "/api/clusters/system-functions/orders",
            put(handlers::system_function::update_function_orders),
        )
        .route(
            "/api/clusters/system-functions/:function_id/execute",
            post(handlers::system_function::execute_system_function),
        )
        .route(
            "/api/clusters/system-functions/:function_id/favorite",
            put(handlers::system_function::toggle_function_favorite),
        )
        .route(
            "/api/clusters/system-functions/:function_id",
            put(handlers::system_function::update_function)
                .delete(handlers::system_function::delete_system_function),
        )
        .route(
            "/api/system-functions/:function_name/access-time",
            put(handlers::system_function::update_system_function_access_time),
        )
        .route(
            "/api/system-functions/category/:category_name",
            delete(handlers::system_function::delete_category),
        )
        .route("/api/clusters/overview", get(handlers::overview::get_cluster_overview))
        .route(
            "/api/clusters/overview/extended",
            get(handlers::overview::get_extended_cluster_overview),
        )
        .route("/api/clusters/overview/health", get(handlers::overview::get_health_cards))
        .route(
            "/api/clusters/overview/performance",
            get(handlers::overview::get_performance_trends),
        )
        .route("/api/clusters/overview/resources", get(handlers::overview::get_resource_trends))
        .route("/api/clusters/overview/data-stats", get(handlers::overview::get_data_statistics))
        .route(
            "/api/clusters/overview/capacity-prediction",
            get(handlers::overview::get_capacity_prediction),
        )
        .route(
            "/api/clusters/overview/compaction-details",
            get(handlers::overview::get_compaction_detail_stats),
        )
        .route("/api/roles", get(handlers::role::list_roles).post(handlers::role::create_role))
        .route(
            "/api/roles/:id",
            get(handlers::role::get_role)
                .put(handlers::role::update_role)
                .delete(handlers::role::delete_role),
        )
        .route(
            "/api/roles/:id/permissions",
            get(handlers::role::get_role_with_permissions)
                .put(handlers::role::update_role_permissions),
        )
        .route("/api/permissions", get(handlers::permission::list_permissions))
        .route("/api/permissions/menu", get(handlers::permission::list_menu_permissions))
        .route("/api/permissions/api", get(handlers::permission::list_api_permissions))
        .route("/api/permissions/tree", get(handlers::permission::get_permission_tree))
        .route("/api/auth/permissions", get(handlers::permission::get_current_user_permissions))
        .route("/api/users", get(handlers::user::list_users).post(handlers::user::create_user))
        .route(
            "/api/users/:id",
            get(handlers::user::get_user)
                .put(handlers::user::update_user)
                .delete(handlers::user::delete_user),
        )
        .route(
            "/api/users/:id/roles",
            get(handlers::user_role::get_user_roles).post(handlers::user_role::assign_role_to_user),
        )
        .route("/api/users/:id/roles/:role_id", delete(handlers::user_role::remove_role_from_user))
        .route("/api/llm/status", get(handlers::llm::get_status))
        .route(
            "/api/llm/providers",
            get(handlers::llm::list_providers).post(handlers::llm::create_provider),
        )
        .route("/api/llm/providers/active", get(handlers::llm::get_active_provider))
        .route(
            "/api/llm/providers/:id",
            get(handlers::llm::get_provider)
                .put(handlers::llm::update_provider)
                .delete(handlers::llm::delete_provider),
        )
        .route("/api/llm/providers/:id/activate", post(handlers::llm::activate_provider))
        .route("/api/llm/providers/:id/deactivate", post(handlers::llm::deactivate_provider))
        .route("/api/llm/providers/:id/test", post(handlers::llm::test_provider_connection))
        .route("/api/llm/analyze/root-cause", post(handlers::llm::analyze_root_cause))
        .route("/api/permission-requests/my", get(handlers::permission_request::list_my_requests))
        .route(
            "/api/permission-requests/pending",
            get(handlers::permission_request::list_pending_approvals),
        )
        .route("/api/permission-requests", post(handlers::permission_request::submit_request))
        .route(
            "/api/permission-requests/:request_id",
            get(handlers::permission_request::get_request),
        )
        .route(
            "/api/permission-requests/:request_id/approve",
            post(handlers::permission_request::approve_request),
        )
        .route(
            "/api/permission-requests/:request_id/reject",
            post(handlers::permission_request::reject_request),
        )
        .route(
            "/api/permission-requests/:request_id/cancel",
            post(handlers::permission_request::cancel_request),
        )
        .route(
            "/api/clusters/:cluster_id/db-auth/accounts",
            get(handlers::permission_request::list_db_accounts),
        )
        .route(
            "/api/clusters/:cluster_id/db-auth/roles",
            get(handlers::permission_request::list_db_roles),
        )
        .route(
            "/api/clusters/db-auth/accounts",
            get(handlers::permission_request::list_db_accounts_active),
        )
        .route(
            "/api/clusters/db-auth/roles",
            get(handlers::permission_request::list_db_roles_active),
        )
        .route(
            "/api/clusters/db-auth/my-permissions",
            get(handlers::permission_request::list_my_db_permissions),
        )
        .route(
            "/api/clusters/db-auth/role-permissions/:role_name",
            get(handlers::permission_request::list_role_permissions),
        )
        .route("/api/db-auth/preview-sql", post(handlers::permission_request::preview_sql))
        .route(
            "/api/clusters/resource-groups",
            get(handlers::resource_group::list_resource_groups)
                .post(handlers::resource_group::create_resource_group),
        )
        .route(
            "/api/clusters/resource-groups/usage",
            get(handlers::resource_group::get_resource_group_usage),
        )
        .route(
            "/api/clusters/resource-groups/analysis",
            get(handlers::resource_group::analyze_resource_usage),
        )
        .route(
            "/api/clusters/resource-groups/:name",
            get(handlers::resource_group::get_resource_group)
                .put(handlers::resource_group::update_resource_group)
                .delete(handlers::resource_group::delete_resource_group),
        )
        .route("/api/notifications", get(handlers::notification::list))
        .route("/api/notifications", post(handlers::notification::create))
        .route("/api/notifications/:id/read", post(handlers::notification::mark_read))
        .route("/api/op-audit-logs", get(handlers::op_audit::list))
        .route("/api/system/logs/archive", get(handlers::log_archive::download))
        .route("/api/agent/chat", post(handlers::agent_chat::chat))
        .route("/api/agent/chat/stream", post(handlers::agent_chat::stream_chat))
        .route("/api/agent/incidents", get(handlers::agent_incident::list_incidents))
        .route("/api/agent/incidents/:id", get(handlers::agent_incident::get_incident))
        .route("/api/agent/incidents/:id/investigate", post(handlers::agent_incident::investigate))
        .route("/api/agent/incidents/:id/analyze", post(handlers::agent_incident::analyze))
        .route("/api/agent/incidents/:id/close", post(handlers::agent_incident::close_incident))
        .route("/api/agent/events", get(handlers::agent_incident::list_events))
        .route(
            "/api/agent/chat-actions/:id/confirm",
            post(handlers::chat_action::confirm_chat_action),
        )
        .route(
            "/api/agent/chat-actions/:id/cancel",
            post(handlers::chat_action::cancel_chat_action),
        )
        .route("/api/agent/chat-actions", get(handlers::chat_action::list_chat_actions))
        .route(
            "/api/agent/incidents/:id/actions",
            get(handlers::agent_incident::list_actions)
                .post(handlers::agent_incident::create_action),
        )
        .route(
            "/api/agent/incidents/:id/actions/:action_id/confirm",
            post(handlers::agent_incident::confirm_action),
        )
        .route(
            "/api/agent/incidents/:id/actions/:action_id/cancel",
            post(handlers::agent_incident::cancel_action),
        )
        .route("/api/agent/sessions", get(handlers::agent_chat::list_sessions))
        .route("/api/agent/messages/:id/feedback", post(handlers::agent_chat::set_message_feedback))
        .route(
            "/api/agent/sessions/:id",
            get(handlers::agent_chat::get_session)
                .delete(handlers::agent_chat::delete_session)
                .patch(handlers::agent_chat::rename_session),
        )
        .with_state(Arc::clone(&app_state_arc))
        .layer(axum_middleware::from_fn_with_state(auth_state, middleware::auth_middleware));

    let health_routes = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(ready_check));

    let static_routes = if config.static_config.enabled {
        tracing::info!("Static file serving enabled, serving from embedded assets");
        Router::new().fallback(serve_static_files)
    } else {
        Router::new()
    };

    let app = Router::new()
        .merge(SwaggerUi::new("/api-docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(public_routes)
        .merge(protected_routes)
        .merge(health_routes)
        .merge(static_routes);

    let app = app
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Server listening on http://{}", addr);
    tracing::info!("API documentation available at http://{}/api-docs", addr);
    tracing::info!("Stellar is ready to serve requests");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}

async fn ready_check() -> &'static str {
    "READY"
}

/// Serve static files from embedded assets
/// Handles SPA routing by falling back to index.html for non-API routes
///
/// Flink-style implementation: backend is path-agnostic,
/// relies on reverse proxy (Nginx/Traefik) rewrite rules
///
/// For sub-path deployments, static assets may be requested with route segments in the path
/// (e.g., /stellar/pages/starrocks/runtime.js). This function extracts the filename
/// from such paths to correctly serve static assets.
async fn serve_static_files(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    if path.starts_with("api/") || path.starts_with("api-docs/") {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    let static_extensions = [
        "js", "css", "png", "jpg", "jpeg", "gif", "svg", "ico", "woff", "woff2", "ttf", "eot",
        "otf", "json",
    ];
    let is_static_asset = static_extensions
        .iter()
        .any(|ext| path.ends_with(&format!(".{}", ext)));

    let asset_path = if is_static_asset {
        path.split('/')
            .next_back()
            .filter(|s| s.contains('.'))
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string())
    } else {
        path.to_string()
    };

    let mut found: Option<(Vec<u8>, HeaderValue /* content_type */, bool /* is_gzip */)> = None;
    if let Some(file) = WebAssets::get(&asset_path) {
        let data: Vec<u8> = file.data.to_vec();
        found = Some((data, get_content_type(&asset_path), false));
    } else if let Some(file) = WebAssets::get(&format!("{asset_path}.gz")) {
        // Text assets are gzip-compressed at build time (build-frontend.sh)
        // and stored as <name>.gz; serve with Content-Encoding: gzip.
        let data: Vec<u8> = file.data.to_vec();
        found = Some((data, get_content_type(&asset_path), true));
    }
    if let Some((data, content_type, is_gzip)) = found {
        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type);
        if is_gzip {
            builder = builder.header(header::CONTENT_ENCODING, "gzip");
        }
        return builder.body(Body::from(data)).unwrap().into_response();
    }

    if let Some(index) = WebAssets::get("index.html") {
        let data: Vec<u8> = index.data.to_vec();
        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8");
        if data.starts_with(&[0x1f, 0x8b]) {
            builder = builder.header(header::CONTENT_ENCODING, "gzip");
        }
        return builder.body(Body::from(data)).unwrap().into_response();
    }
    if let Some(index) = WebAssets::get("index.html.gz") {
        let data: Vec<u8> = index.data.to_vec();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(data))
            .unwrap()
            .into_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// Get content type based on file extension
fn get_content_type(path: &str) -> HeaderValue {
    let ext = path.rsplit('.').next().unwrap_or("");
    let content_type = match ext {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "eot" => "application/vnd.ms-fontobject",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    };
    HeaderValue::from_static(content_type)
}
