use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::utils::{ApiError, ApiResult};

/// Stable identity for a materialized view. Names are never sufficient because
/// the same name may exist in multiple databases or represent a ROLLUP index.
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, PartialEq, Eq)]
pub struct MaterializedViewRef {
    pub database: String,
    pub name: String,
    pub kind: MaterializedViewKind,
}

impl MaterializedViewRef {
    pub fn validate(&self) -> ApiResult<()> {
        Self::validate_identifier("database", &self.database)?;
        Self::validate_identifier("materialized view", &self.name)
    }

    pub fn validate_identifier(label: &str, value: &str) -> ApiResult<()> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(ApiError::invalid_data(format!("{label} identifier is invalid")));
        }
        Ok(())
    }

    pub fn quoted_name(&self) -> String {
        format!("`{}`.`{}`", self.database, self.name)
    }

    pub fn display_name(&self) -> String {
        format!("{}.{} ({})", self.database, self.name, self.kind.as_str())
    }
}

/// The two MV implementations have different management SQL and dependency
/// semantics, so callers must state which one they mean.
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterializedViewKind {
    Async,
    Rollup,
}

impl MaterializedViewKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Async => "async",
            Self::Rollup => "rollup",
        }
    }
}

/// Materialized view basic information.
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct MaterializedView {
    pub id: String,
    pub name: String,
    pub database_name: String,
    pub kind: MaterializedViewKind,
    pub refresh_type: String,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_start_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_finished_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<i64>,
    /// Engine-provided definition text. This replaces the old ambiguous `text`
    /// field so the API and frontend have one name for the same value.
    pub definition: String,
}

impl MaterializedView {
    pub fn object_ref(&self) -> MaterializedViewRef {
        MaterializedViewRef {
            database: self.database_name.clone(),
            name: self.name.clone(),
            kind: self.kind,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameMaterializedViewRequest {
    pub new_name: String,
}

impl RenameMaterializedViewRequest {
    pub fn validate(&self) -> ApiResult<()> {
        MaterializedViewRef::validate_identifier("new materialized view", &self.new_name)
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshMaterializedViewRequest {
    pub mode: RefreshMode,
    #[serde(default)]
    pub force: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partition: Option<PartitionRange>,
}

impl RefreshMaterializedViewRequest {
    pub fn validate(&self) -> ApiResult<()> {
        if let Some(partition) = &self.partition {
            partition.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    Async,
    Sync,
    Auto,
    Complete,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
#[serde(deny_unknown_fields)]
pub struct PartitionRange {
    pub start: PartitionValue,
    pub end: PartitionValue,
}

impl PartitionRange {
    fn validate(&self) -> ApiResult<()> {
        self.start.validate()?;
        self.end.validate()
    }
}

/// A partition boundary is data, not a SQL fragment. The server validates the
/// typed value before rendering the engine's fixed refresh syntax.
#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PartitionValue {
    Date(String),
    Timestamp(String),
    Integer(i64),
    String(String),
}

impl PartitionValue {
    fn validate(&self) -> ApiResult<()> {
        match self {
            Self::Date(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map(|_| ())
                .map_err(|_| ApiError::invalid_data("partition date must be YYYY-MM-DD")),
            Self::Timestamp(value) => NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
                .map(|_| ())
                .map_err(|_| {
                    ApiError::invalid_data("partition timestamp must be YYYY-MM-DD HH:MM:SS")
                }),
            Self::Integer(_) => Ok(()),
            Self::String(value) => {
                if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                    return Err(ApiError::invalid_data("partition string is invalid"));
                }
                Ok(())
            },
        }
    }

    pub fn sql_literal(&self) -> ApiResult<String> {
        self.validate()?;
        Ok(match self {
            Self::Integer(value) => value.to_string(),
            Self::Date(value) | Self::Timestamp(value) | Self::String(value) => {
                format!("'{}'", value.replace('\'', "''"))
            },
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMaterializedViewStateRequest {
    pub state: MaterializedViewState,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterializedViewState {
    Active,
    Inactive,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRefreshScheduleRequest {
    pub schedule: RefreshSchedule,
}

impl UpdateRefreshScheduleRequest {
    pub fn validate(&self) -> ApiResult<()> {
        self.schedule.validate()
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RefreshSchedule {
    Manual,
    Scheduled { interval: u32, unit: RefreshIntervalUnit },
}

impl RefreshSchedule {
    pub fn validate(self) -> ApiResult<()> {
        if let Self::Scheduled { interval, .. } = self
            && !(1..=8_760).contains(&interval)
        {
            return Err(ApiError::invalid_data("refresh interval must be between 1 and 8760"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefreshIntervalUnit {
    Hour,
    Day,
}

impl RefreshIntervalUnit {
    pub const fn sql_keyword(self) -> &'static str {
        match self {
            Self::Hour => "HOUR",
            Self::Day => "DAY",
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MaterializedViewPreview {
    pub ddl: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MaterializedViewDDL {
    pub object: MaterializedViewRef,
    pub ddl: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyEvidence {
    Verified,
    Partial,
    Annotated,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencySource {
    StarRocksObjectDependencies,
    RollupParent,
    DorisDefinition,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Table,
    View,
    MaterializedView,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct DependencyObject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    pub name: String,
    pub kind: RelationKind,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct MaterializedViewDependency {
    pub object: DependencyObject,
    pub evidence: DependencyEvidence,
    pub source: DependencySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_snippet: Option<String>,
    /// Time at which this direct relationship was read from the engine or
    /// conservatively extracted from its stored definition.
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MaterializedViewDependencies {
    pub object: MaterializedViewRef,
    pub dependencies: Vec<MaterializedViewDependency>,
    pub complete: bool,
    pub warnings: Vec<String>,
    /// Time at which this bounded dependency response was assembled.
    pub read_at: DateTime<Utc>,
}

/// A bounded, evidence-backed opportunity to investigate an asynchronous
/// StarRocks materialized view. It is not executable DDL or a benefit claim.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MaterializedViewOpportunity {
    /// A query pattern with comments and literal values removed on the server.
    pub sql_pattern: String,
    pub source_database: String,
    pub source_table: String,
    pub execution_count: u64,
    pub total_duration_ms: u64,
    pub average_duration_ms: u64,
    pub p95_duration_ms: u64,
    pub first_seen: String,
    pub last_seen: String,
}

/// Read-only workload evidence used to discover bounded MV opportunities.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MaterializedViewOpportunityResponse {
    pub supported: bool,
    pub engine: String,
    pub source: String,
    pub observed_at: DateTime<Utc>,
    pub window_hours: i64,
    pub sampled_query_count: usize,
    /// The audit source or result list reached a configured upper bound.
    pub truncated: bool,
    pub candidates: Vec<MaterializedViewOpportunity>,
    pub warnings: Vec<String>,
}
