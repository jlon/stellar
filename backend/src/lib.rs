//! Stellar Library
//!
//! This library contains all the core modules for the Stellar application.

// Allow some clippy lints that are too strict for this codebase
#![allow(clippy::collapsible_if)]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::manual_contains)]
#![allow(clippy::manual_map)]
#![allow(clippy::let_and_return)]
#![allow(clippy::manual_pattern_char_comparison)]
#![allow(clippy::if_same_then_else)]

use sqlx::Pool;
use std::{path::PathBuf, sync::Arc};

use crate::db::AppDb;

pub mod config;
pub mod db;
pub mod embedded;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod services;
pub mod utils;

#[cfg(test)]
#[path = "tests/permission_request_service_test.rs"]
mod permission_request_service_test;

#[cfg(test)]
#[path = "tests/log_archive_test.rs"]
mod log_archive_test;

#[cfg(test)]
#[path = "tests/bootstrap_test.rs"]
mod bootstrap_test;

#[cfg(test)]
#[path = "tests/config_test.rs"]
mod config_test;

use crate::services::NotificationService;
use crate::services::profile_analyzer::ProfileAnalysisCache;

// Re-export commonly used types
pub use config::Config;
pub use services::llm::{LLMError, LLMProviderInfo, LLMService, LLMServiceImpl};
pub use services::{
    AgentRuntimeService, AuthService, CasbinService, ClusterService, DataStatisticsService,
    DbAuthQueryService, MetricsCollectorService, MySQLPoolManager, OpsAgentService,
    OrganizationService, OverviewService, PermissionRequestService, PermissionService, RoleService,
    SystemFunctionService, UserRoleService, UserService,
};
pub use utils::JwtUtil;

/// Application shared state
///
/// Design Philosophy: Keep it simple - Rust's type system IS our DI container.
/// No need for Service Container pattern with dyn Any.
/// All services are wrapped in Arc for cheap cloning and thread safety.
#[derive(Clone)]
pub struct AppState<DB: AppDb> {
    pub db: Pool<DB>,

    pub mysql_pool_manager: Arc<MySQLPoolManager>,
    pub jwt_util: Arc<JwtUtil>,
    pub audit_config: config::AuditLogConfig,
    pub log_file: Option<PathBuf>,

    pub auth_service: Arc<AuthService<DB>>,
    pub cluster_service: Arc<ClusterService<DB>>,
    pub organization_service: Arc<OrganizationService<DB>>,
    pub system_function_service: Arc<SystemFunctionService<DB>>,
    pub metrics_collector_service: Arc<MetricsCollectorService<DB>>,
    pub data_statistics_service: Arc<DataStatisticsService<DB>>,
    pub overview_service: Arc<OverviewService<DB>>,

    pub casbin_service: Arc<CasbinService>,
    pub permission_service: Arc<PermissionService<DB>>,
    pub role_service: Arc<RoleService<DB>>,
    pub user_role_service: Arc<UserRoleService<DB>>,
    pub user_service: Arc<UserService<DB>>,

    pub llm_service: Arc<LLMServiceImpl<DB>>,

    pub db_auth_query_service: Arc<DbAuthQueryService<DB>>,
    pub permission_request_service: Arc<PermissionRequestService<DB>>,
    pub profile_analysis_cache: Arc<ProfileAnalysisCache>,
    pub ops_agent_service: Arc<OpsAgentService<DB>>,
    pub notification_service: Arc<NotificationService<DB>>,

    pub agent_runtime_service: Arc<AgentRuntimeService<DB>>,
}

impl<DB: AppDb> AppState<DB> {
    /// AI 会话相关表共用 OpsAgentService 的连接池（避免重复建池）。
    pub fn ops_agent_service_pool(&self) -> sqlx::Pool<DB> {
        self.ops_agent_service.pool()
    }
}
