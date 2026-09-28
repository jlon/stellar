use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

use super::{DependencyEvidence, DependencySource};

/// A stable object kind used by the read-only Schema Explorer API.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaObjectKind {
    Table,
    View,
    MaterializedView,
    ExternalTable,
}

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
pub struct SchemaObjectIdentity {
    pub cluster_id: i64,
    pub catalog: String,
    pub database: String,
    pub name: String,
    pub object_kind: SchemaObjectKind,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaObjectSummary {
    pub name: String,
    pub object_kind: SchemaObjectKind,
    /// Short-lived, server-issued opaque reference for detail reads.
    pub object_ref: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: Option<bool>,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaPhysicalProperties {
    pub key_model: Option<String>,
    pub partition: Option<String>,
    pub distribution: Option<String>,
    pub engine: Option<String>,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaParseStatus {
    Parsed,
    Partial,
    RawOnly,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaObjectDetail {
    pub identity: SchemaObjectIdentity,
    pub columns: Vec<SchemaColumn>,
    pub physical_properties: Option<SchemaPhysicalProperties>,
    pub ddl_raw: String,
    pub parse_status: SchemaParseStatus,
    pub read_at: DateTime<Utc>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaDependencyDirection {
    Upstream,
    Downstream,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SchemaRelationKind {
    ViewReads,
    MvReads,
    DeclaredConstraint,
}

/// A one-hop dependency whose target identity was verified by an engine
/// metadata source or marked partial by a bounded definition parser.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaObjectDependency {
    pub direction: SchemaDependencyDirection,
    pub relation_kind: SchemaRelationKind,
    pub object: SchemaObjectIdentity,
    pub object_ref: String,
    /// Object DDL was not re-read while collecting the relationship.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_parse_status: Option<SchemaParseStatus>,
    pub evidence: DependencyEvidence,
    pub source: DependencySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_snippet: Option<String>,
    /// Time at which the relationship was read from the engine metadata source.
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SchemaObjectDependencies {
    pub object: SchemaObjectIdentity,
    pub dependencies: Vec<SchemaObjectDependency>,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub read_at: DateTime<Utc>,
}
