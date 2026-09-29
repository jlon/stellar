use std::{collections::HashSet, ops::ControlFlow};

use serde::{Deserialize, Serialize};
use sqlparser::{
    ast::{Query, SetExpr, Statement, Visit, Visitor},
    dialect::MySqlDialect,
    parser::Parser,
};
use utoipa::ToSchema;

use super::{MaterializedViewKind, MaterializedViewRef, RefreshSchedule};
use crate::utils::{ApiError, ApiResult};

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMaterializedViewRequest {
    pub database: String,
    pub name: String,
    #[serde(default)]
    pub cluster_id: Option<i64>,
    #[serde(default)]
    pub source_database: String,
    #[serde(default)]
    pub source_table: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub query_sql: Option<String>,
    #[serde(default)]
    pub partition_by: Option<String>,
    #[serde(default)]
    pub distribution: Option<MvDistribution>,
    #[serde(default)]
    pub build_immediate: bool,
    #[serde(default)]
    pub sort_columns: Vec<String>,
    #[serde(default)]
    pub replication_num: Option<u8>,
    #[serde(default)]
    pub confirmed_ddl: Option<String>,
    pub schedule: RefreshSchedule,
}

impl CreateMaterializedViewRequest {
    pub fn validate(&self) -> ApiResult<()> {
        const MAX_COLUMNS: usize = 128;

        let reference = self.reference();
        reference.validate()?;
        if self.cluster_id.is_none() {
            return Err(ApiError::invalid_data("active cluster id is required"));
        }
        if self.cluster_id.is_some_and(|id| id <= 0) {
            return Err(ApiError::invalid_data("cluster id must be positive"));
        }
        if reference.name.len() > 64
            || !reference
                .name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic())
        {
            return Err(ApiError::invalid_data(
                "materialized view name must start with a letter and be at most 64 characters",
            ));
        }
        if self.query_sql.is_some() {
            if !self.source_database.is_empty()
                || !self.source_table.is_empty()
                || !self.columns.is_empty()
            {
                return Err(ApiError::invalid_data(
                    "custom query cannot include projection source fields",
                ));
            }
            self.select_sql()?;
        } else {
            MaterializedViewRef::validate_identifier("source database", &self.source_database)?;
            MaterializedViewRef::validate_identifier("source table", &self.source_table)?;
            if self.columns.is_empty() || self.columns.len() > MAX_COLUMNS {
                return Err(ApiError::invalid_data(
                    "materialized view must select between 1 and 128 columns",
                ));
            }
            let mut columns = HashSet::new();
            for column in &self.columns {
                MaterializedViewRef::validate_identifier("source column", column)?;
                if !columns.insert(column.to_ascii_lowercase()) {
                    return Err(ApiError::invalid_data("materialized view columns must be unique"));
                }
            }
        }
        if let Some(column) = &self.partition_by {
            MaterializedViewRef::validate_identifier("partition column", column)?;
            if self.query_sql.is_none()
                && !self
                    .columns
                    .iter()
                    .any(|selected| selected.eq_ignore_ascii_case(column))
            {
                return Err(ApiError::invalid_data("partition column must be a selected column"));
            }
        }
        if self.sort_columns.len() > 8
            || self
                .replication_num
                .is_some_and(|count| count == 0 || count > 10)
        {
            return Err(ApiError::invalid_data(
                "sort keys must not exceed 8; replica count must be 1 to 10",
            ));
        }
        let mut sort_keys = HashSet::new();
        for column in &self.sort_columns {
            MaterializedViewRef::validate_identifier("sort column", column)?;
            if !sort_keys.insert(column.to_ascii_lowercase())
                || self.query_sql.is_none()
                    && !self
                        .columns
                        .iter()
                        .any(|selected| selected.eq_ignore_ascii_case(column))
            {
                return Err(ApiError::invalid_data("sort keys must be distinct selected columns"));
            }
        }
        if let Some(distribution) = &self.distribution {
            distribution.validate()?;
            if let MvDistribution::Hash { columns, .. } = distribution
                && self.query_sql.is_none()
                && columns.iter().any(|column| {
                    !self
                        .columns
                        .iter()
                        .any(|selected| selected.eq_ignore_ascii_case(column))
                })
            {
                return Err(ApiError::invalid_data("hash distribution columns must be selected"));
            }
        }
        self.schedule.validate()
    }

    pub fn reference(&self) -> MaterializedViewRef {
        MaterializedViewRef {
            database: self.database.clone(),
            name: self.name.clone(),
            kind: MaterializedViewKind::Async,
        }
    }

    pub fn source_quoted_name(&self) -> String {
        format!("`{}`.`{}`", self.source_database, self.source_table)
    }

    pub fn select_sql(&self) -> ApiResult<String> {
        if let Some(sql) = &self.query_sql {
            if sql.len() > 20_000 || sql.trim().is_empty() {
                return Err(ApiError::invalid_data("query must contain 1 to 20000 bytes"));
            }
            let statements = Parser::parse_sql(&MySqlDialect {}, sql)
                .map_err(|_| ApiError::invalid_data("query must be a valid SELECT statement"))?;
            let [Statement::Query(query)] = statements.as_slice() else {
                return Err(ApiError::invalid_data(
                    "query must contain exactly one SELECT statement",
                ));
            };
            let mut guard = MvQueryGuard;
            if query.visit(&mut guard).is_break() {
                return Err(ApiError::invalid_data("query cannot write data or lock tables"));
            }
            return Ok(statements[0].to_string());
        }
        Ok(format!("SELECT {} FROM {}", self.selected_columns_sql(), self.source_quoted_name()))
    }

    pub fn selected_columns_sql(&self) -> String {
        self.columns
            .iter()
            .map(|column| format!("`{column}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Reject nested data-modifying CTEs, SELECT INTO and locking reads, not just a top-level keyword.
struct MvQueryGuard;

impl Visitor for MvQueryGuard {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        if matches!(statement, Statement::Query(_)) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if query.locks.is_empty() && contains_only_selects(&query.body) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }
}

fn contains_only_selects(expr: &SetExpr) -> bool {
    match expr {
        SetExpr::Select(select) => select.into.is_none(),
        SetExpr::Query(query) => contains_only_selects(&query.body),
        SetExpr::SetOperation { left, right, .. } => {
            contains_only_selects(left) && contains_only_selects(right)
        },
        _ => false,
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MvDistribution {
    Random { buckets: Option<u16> },
    Hash { columns: Vec<String>, buckets: Option<u16> },
}

impl MvDistribution {
    fn validate(&self) -> ApiResult<()> {
        let buckets = match self {
            Self::Random { buckets } => buckets,
            Self::Hash { columns, buckets } => {
                if columns.is_empty() || columns.len() > 8 {
                    return Err(ApiError::invalid_data("hash distribution needs 1 to 8 columns"));
                }
                let mut unique = HashSet::new();
                for column in columns {
                    MaterializedViewRef::validate_identifier("distribution column", column)?;
                    if !unique.insert(column.to_ascii_lowercase()) {
                        return Err(ApiError::invalid_data("distribution columns must be unique"));
                    }
                }
                buckets
            },
        };
        if buckets.is_some_and(|count| count == 0 || count > 1024) {
            return Err(ApiError::invalid_data("buckets must be between 1 and 1024"));
        }
        Ok(())
    }

    pub fn sql_clause(&self) -> String {
        let (kind, buckets) = match self {
            Self::Random { buckets } => ("RANDOM".to_string(), buckets),
            Self::Hash { columns, buckets } => (
                format!(
                    "HASH({})",
                    columns
                        .iter()
                        .map(|col| format!("`{col}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                buckets,
            ),
        };
        format!(
            " DISTRIBUTED BY {kind}{}",
            buckets
                .map(|count| format!(" BUCKETS {count}"))
                .unwrap_or_default()
        )
    }
}
