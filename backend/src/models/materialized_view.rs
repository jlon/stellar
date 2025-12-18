use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Materialized view basic information (from SHOW MATERIALIZED VIEWS)
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct MaterializedView {
    /// Materialized view ID
    pub id: String,

    /// Materialized view name
    pub name: String,

    /// Database name
    pub database_name: String,

    /// Refresh type: ROLLUP(sync)/MANUAL(manual)/ASYNC(auto)/INCREMENTAL(incremental)
    pub refresh_type: String,

    /// Is active
    pub is_active: bool,

    /// Partition type (RANGE/UNPARTITIONED/LIST)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_type: Option<String>,

    /// Refresh task ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// Refresh task name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,

    /// Last refresh start time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_start_time: Option<String>,

    /// Last refresh finished time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_finished_time: Option<String>,

    /// Last refresh duration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_duration: Option<String>,

    /// Last refresh state: SUCCESS/RUNNING/FAILED/PENDING
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_state: Option<String>,

    /// Row count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<i64>,

    /// CREATE statement text
    pub text: String,
}

/// Request to create materialized view
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMaterializedViewRequest {
    /// Complete CREATE MATERIALIZED VIEW SQL statement
    pub sql: String,
}

/// Request to refresh materialized view
#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshMaterializedViewRequest {
    /// Start partition (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_start: Option<String>,

    /// End partition (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_end: Option<String>,

    /// Force refresh
    #[serde(default)]
    pub force: bool,

    /// Refresh mode: SYNC/ASYNC
    #[serde(default = "default_refresh_mode")]
    pub mode: String,
}

fn default_refresh_mode() -> String {
    "ASYNC".to_string()
}

/// Request to alter materialized view
#[derive(Debug, Deserialize, ToSchema)]
pub struct AlterMaterializedViewRequest {
    /// ALTER clause (e.g., RENAME new_name, ACTIVE, INACTIVE, etc.)
    pub alter_clause: String,
}

/// Materialized view DDL response
#[derive(Debug, Serialize, ToSchema)]
pub struct MaterializedViewDDL {
    pub mv_name: String,
    pub ddl: String,
}
