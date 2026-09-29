use crate::models::{
    CreateMaterializedViewRequest, DependencyEvidence, DependencyObject, DependencySource,
    MaterializedView, MaterializedViewDependencies, MaterializedViewDependency,
    MaterializedViewKind, MaterializedViewRef, MaterializedViewState, RefreshIntervalUnit,
    RefreshMaterializedViewRequest, RefreshMode, RefreshSchedule, RelationKind,
};
use crate::services::MySQLClient;
use crate::utils::{ApiError, ApiResult};
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;

pub struct MaterializedViewService {
    mysql_client: MySQLClient,
}

impl MaterializedViewService {
    pub fn new(mysql_client: MySQLClient) -> Self {
        Self { mysql_client }
    }

    pub async fn list_materialized_views(
        &self,
        database: Option<&str>,
    ) -> ApiResult<Vec<MaterializedView>> {
        if let Some(database) = database {
            MaterializedViewRef::validate_identifier("database", database)?;
        }
        self.query_current_materialized_views(database, None).await
    }

    pub async fn get_materialized_view(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<MaterializedView> {
        reference.validate()?;
        let mut matches = self
            .query_current_materialized_views(Some(&reference.database), Some(&reference.name))
            .await?
            .into_iter()
            .filter(|materialized_view| materialized_view.kind == reference.kind);
        let materialized_view = matches
            .next()
            .ok_or_else(|| ApiError::not_found(reference.display_name()))?;
        if matches.next().is_some() {
            return Err(ApiError::invalid_data(format!(
                "materialized view identity is ambiguous: {}",
                reference.display_name()
            )));
        }
        Ok(materialized_view)
    }

    pub async fn get_materialized_view_ddl(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<String> {
        let materialized_view = self.get_materialized_view(reference).await?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Ok(materialized_view.definition);
        }

        let (_, rows) = self
            .mysql_client
            .query_raw(&format!("SHOW CREATE MATERIALIZED VIEW {}", reference.quoted_name()))
            .await?;
        Self::ddl_from_rows(&rows).ok_or_else(|| ApiError::not_found(reference.display_name()))
    }

    pub async fn create_materialized_view(
        &self,
        request: &CreateMaterializedViewRequest,
    ) -> ApiResult<()> {
        self.mysql_client
            .execute(&Self::create_materialized_view_sql(request)?)
            .await
            .map(|_| ())
    }

    pub async fn drop_materialized_view(&self, reference: &MaterializedViewRef) -> ApiResult<()> {
        self.get_materialized_view(reference).await?;
        self.mysql_client
            .execute(&format!("DROP MATERIALIZED VIEW {}", reference.quoted_name()))
            .await
            .map(|_| ())
    }

    pub async fn refresh_materialized_view(
        &self,
        reference: &MaterializedViewRef,
        request: &RefreshMaterializedViewRequest,
    ) -> ApiResult<()> {
        self.ensure_async(reference)?;
        request.validate()?;

        let mode = match request.mode {
            RefreshMode::Async => "ASYNC",
            RefreshMode::Sync => "SYNC",
            RefreshMode::Auto | RefreshMode::Complete => {
                return Err(ApiError::invalid_data("StarRocks refresh mode must be async or sync"));
            },
        };
        let mut sql = format!("REFRESH MATERIALIZED VIEW {}", reference.quoted_name());
        if let Some(partition) = &request.partition {
            sql.push_str(&format!(
                " PARTITION START ({}) END ({})",
                partition.start.sql_literal()?,
                partition.end.sql_literal()?
            ));
        }
        if request.force {
            sql.push_str(" FORCE");
        }
        sql.push_str(&format!(" WITH {mode} MODE"));
        self.mysql_client.execute(&sql).await.map(|_| ())
    }

    pub async fn cancel_refresh_materialized_view(
        &self,
        reference: &MaterializedViewRef,
        force: bool,
    ) -> ApiResult<()> {
        self.ensure_async(reference)?;
        let force = if force { " FORCE" } else { "" };
        self.mysql_client
            .execute(&format!(
                "CANCEL REFRESH MATERIALIZED VIEW {}{force}",
                reference.quoted_name()
            ))
            .await
            .map(|_| ())
    }

    pub async fn set_materialized_view_state(
        &self,
        reference: &MaterializedViewRef,
        state: MaterializedViewState,
    ) -> ApiResult<()> {
        self.ensure_async(reference)?;
        let state = match state {
            MaterializedViewState::Active => "ACTIVE",
            MaterializedViewState::Inactive => "INACTIVE",
        };
        self.mysql_client
            .execute(&format!("ALTER MATERIALIZED VIEW {} {state}", reference.quoted_name()))
            .await
            .map(|_| ())
    }

    pub async fn rename_materialized_view(
        &self,
        reference: &MaterializedViewRef,
        new_name: &str,
    ) -> ApiResult<()> {
        self.ensure_async(reference)?;
        MaterializedViewRef::validate_identifier("new materialized view", new_name)?;
        self.mysql_client
            .execute(&format!(
                "ALTER MATERIALIZED VIEW {} RENAME `{new_name}`",
                reference.quoted_name()
            ))
            .await
            .map(|_| ())
    }

    pub async fn update_refresh_schedule(
        &self,
        reference: &MaterializedViewRef,
        schedule: RefreshSchedule,
    ) -> ApiResult<()> {
        self.ensure_async(reference)?;
        let schedule = Self::refresh_schedule_clause(schedule);
        self.mysql_client
            .execute(&format!("ALTER MATERIALIZED VIEW {} {schedule}", reference.quoted_name()))
            .await
            .map(|_| ())
    }

    pub async fn get_direct_dependencies(
        &self,
        reference: &MaterializedViewRef,
    ) -> ApiResult<MaterializedViewDependencies> {
        reference.validate()?;
        let materialized_view = self.get_materialized_view(reference).await?;
        if reference.kind == MaterializedViewKind::Rollup {
            let parent = Self::rollup_parent_from_definition(&materialized_view.definition)
                .ok_or_else(|| ApiError::not_found(reference.display_name()))?;
            let observed_at = Utc::now();
            return Ok(MaterializedViewDependencies {
                object: reference.clone(),
                dependencies: vec![MaterializedViewDependency {
                    object: DependencyObject {
                        catalog: None,
                        database: Some(reference.database.clone()),
                        name: parent,
                        kind: RelationKind::Table,
                    },
                    evidence: DependencyEvidence::Verified,
                    source: DependencySource::RollupParent,
                    evidence_snippet: None,
                    observed_at,
                }],
                complete: true,
                warnings: Vec::new(),
                read_at: observed_at,
            });
        }

        let sql = Self::direct_dependencies_query(reference);
        let rows = match self.mysql_client.query(&sql).await {
            Ok(rows) => rows,
            Err(error) => {
                let read_at = Utc::now();
                return Ok(MaterializedViewDependencies {
                    object: reference.clone(),
                    dependencies: Vec::new(),
                    complete: false,
                    warnings: vec![format!(
                        "StarRocks dependency metadata is unavailable for this cluster or user: {error}"
                    )],
                    read_at,
                });
            },
        };
        let observed_at = Utc::now();
        let dependencies = rows
            .into_iter()
            .filter_map(|row| {
                let name = row.get("name")?.as_str()?.to_string();
                Some(MaterializedViewDependency {
                    object: DependencyObject {
                        catalog: Self::row_string(&row, "catalog"),
                        database: Self::row_string(&row, "database_name"),
                        name,
                        kind: Self::relation_kind(Self::row_string(&row, "object_type").as_deref()),
                    },
                    evidence: DependencyEvidence::Verified,
                    source: DependencySource::StarRocksObjectDependencies,
                    evidence_snippet: None,
                    observed_at,
                })
            })
            .collect();

        Ok(MaterializedViewDependencies {
            object: reference.clone(),
            dependencies,
            complete: true,
            warnings: Vec::new(),
            read_at: observed_at,
        })
    }

    fn ensure_async(&self, reference: &MaterializedViewRef) -> ApiResult<()> {
        reference.validate()?;
        if reference.kind == MaterializedViewKind::Rollup {
            return Err(ApiError::not_implemented(
                "ROLLUP materialized views are maintained synchronously and cannot use this operation",
            ));
        }
        Ok(())
    }

    async fn query_current_materialized_views(
        &self,
        database: Option<&str>,
        name: Option<&str>,
    ) -> ApiResult<Vec<MaterializedView>> {
        let mut filters = Vec::new();
        match database {
            Some(database) => {
                filters.push(format!("mv.TABLE_SCHEMA = {}", Self::sql_literal(database)))
            },
            None => filters
                .push("mv.TABLE_SCHEMA NOT IN ('information_schema', '_statistics_')".to_string()),
        }
        if let Some(name) = name {
            filters.push(format!("mv.TABLE_NAME = {}", Self::sql_literal(name)));
        }
        let sql = Self::current_materialized_views_query(&filters);
        Self::parse_system_table_results(self.mysql_client.query(&sql).await?)
    }

    pub(crate) fn current_materialized_views_query(filters: &[String]) -> String {
        format!(
            "SELECT mv.MATERIALIZED_VIEW_ID AS id, mv.TABLE_NAME AS name, \
             mv.TABLE_SCHEMA AS database_name, mv.REFRESH_TYPE AS refresh_type, \
             mv.IS_ACTIVE AS is_active, mv.PARTITION_TYPE AS partition_type, \
             mv.TASK_ID AS task_id, mv.TASK_NAME AS task_name, \
             mv.LAST_REFRESH_START_TIME AS last_refresh_start_time, \
             mv.LAST_REFRESH_FINISHED_TIME AS last_refresh_finished_time, \
             mv.LAST_REFRESH_DURATION AS last_refresh_duration, \
             mv.LAST_REFRESH_STATE AS last_refresh_state, COALESCE(t.TABLE_ROWS, 0) AS table_rows, \
             mv.MATERIALIZED_VIEW_DEFINITION AS definition \
             FROM information_schema.materialized_views mv \
             LEFT JOIN information_schema.tables t \
             ON mv.TABLE_SCHEMA = t.TABLE_SCHEMA AND mv.TABLE_NAME = t.TABLE_NAME \
             WHERE {}",
            filters.join(" AND ")
        )
    }

    fn parse_system_table_results(
        results: Vec<serde_json::Value>,
    ) -> ApiResult<Vec<MaterializedView>> {
        Ok(results
            .into_iter()
            .map(|row| {
                let refresh_type =
                    Self::row_string(&row, "refresh_type").unwrap_or_else(|| "UNKNOWN".to_string());
                MaterializedView {
                    id: Self::row_string(&row, "id").unwrap_or_default(),
                    name: Self::row_string(&row, "name").unwrap_or_default(),
                    database_name: Self::row_string(&row, "database_name").unwrap_or_default(),
                    kind: Self::kind_from_refresh_type(&refresh_type),
                    refresh_type,
                    is_active: Self::row_string(&row, "is_active")
                        .is_some_and(|value| value == "true" || value == "1"),
                    partition_type: Self::row_string(&row, "partition_type"),
                    task_id: Self::row_string(&row, "task_id"),
                    task_name: Self::row_string(&row, "task_name"),
                    last_refresh_start_time: Self::row_string(&row, "last_refresh_start_time"),
                    last_refresh_finished_time: Self::row_string(
                        &row,
                        "last_refresh_finished_time",
                    ),
                    last_refresh_duration: Self::row_string(&row, "last_refresh_duration"),
                    last_refresh_state: Self::row_string(&row, "last_refresh_state"),
                    rows: row.get("table_rows").and_then(|value| {
                        value
                            .as_i64()
                            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                    }),
                    definition: Self::row_string(&row, "definition").unwrap_or_default(),
                }
            })
            .collect())
    }

    pub(crate) fn kind_from_refresh_type(refresh_type: &str) -> MaterializedViewKind {
        if refresh_type.eq_ignore_ascii_case("SYNC") || refresh_type.eq_ignore_ascii_case("ROLLUP")
        {
            MaterializedViewKind::Rollup
        } else {
            MaterializedViewKind::Async
        }
    }

    pub(crate) fn rollup_parent_from_definition(definition: &str) -> Option<String> {
        static SOURCE_RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"(?is)\bfrom\s+(?:(?:`[^`]+`|[a-z_][a-z0-9_]*)\s*\.\s*){0,2}(`[^`]+`|[a-z_][a-z0-9_]*)")
                .expect("ROLLUP source regex is valid")
        });
        SOURCE_RE
            .captures(definition)
            .and_then(|capture| capture.get(1))
            .map(|name| name.as_str().trim_matches('`').replace("``", "`"))
    }

    fn ddl_from_rows(rows: &[Vec<String>]) -> Option<String> {
        rows.first()
            .and_then(|row| row.get(1).or_else(|| row.first()))
            .cloned()
    }

    fn row_string(row: &serde_json::Value, key: &str) -> Option<String> {
        row.get(key).and_then(|value| match value {
            serde_json::Value::String(value) => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            serde_json::Value::Bool(value) => Some(value.to_string()),
            _ => None,
        })
    }

    pub(crate) fn relation_kind(value: Option<&str>) -> RelationKind {
        match value.unwrap_or_default().to_ascii_uppercase().as_str() {
            value if value.contains("MATERIALIZED") => RelationKind::MaterializedView,
            value if value.contains("VIEW") => RelationKind::View,
            value
                if value.contains("TABLE")
                    || value.contains("OLAP")
                    || value.contains("CLOUD_NATIVE") =>
            {
                RelationKind::Table
            },
            _ => RelationKind::Unknown,
        }
    }

    pub(crate) fn direct_dependencies_query(reference: &MaterializedViewRef) -> String {
        let database = Self::sql_literal(&reference.database);
        let name = Self::sql_literal(&reference.name);
        format!(
            "SELECT ref_object_name AS name, ref_object_database AS database_name, \
             ref_object_catalog AS catalog, ref_object_type AS object_type \
             FROM sys.object_dependencies \
             WHERE object_database = {database} AND object_name = {name} \
             AND object_type IN ('MATERIALIZED_VIEW', 'CLOUD_NATIVE_MATERIALIZED_VIEW')"
        )
    }

    fn refresh_unit_keyword(unit: RefreshIntervalUnit) -> &'static str {
        unit.sql_keyword()
    }

    pub(crate) fn refresh_schedule_clause(schedule: RefreshSchedule) -> String {
        match schedule {
            RefreshSchedule::Manual => "REFRESH MANUAL".to_string(),
            RefreshSchedule::Scheduled { interval, unit } => format!(
                "REFRESH SCHEDULE EVERY (INTERVAL {interval} {})",
                Self::refresh_unit_keyword(unit)
            ),
        }
    }

    pub(crate) fn create_materialized_view_sql(
        request: &CreateMaterializedViewRequest,
    ) -> ApiResult<String> {
        request.validate()?;
        let build = if request.build_immediate { "IMMEDIATE" } else { "DEFERRED" };
        let partition = request
            .partition_by
            .as_ref()
            .map(|column| format!(" PARTITION BY `{column}`"))
            .unwrap_or_default();
        let distribution = request
            .distribution
            .as_ref()
            .map(|distribution| distribution.sql_clause())
            .unwrap_or_default();
        let sort = if request.sort_columns.is_empty() {
            String::new()
        } else {
            format!(
                " ORDER BY ({})",
                request
                    .sort_columns
                    .iter()
                    .map(|col| format!("`{col}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let properties = request
            .replication_num
            .map(|count| format!(" PROPERTIES (\"replication_num\" = \"{count}\")"))
            .unwrap_or_default();
        Ok(format!(
            "CREATE MATERIALIZED VIEW {} REFRESH {} {}{}{}{}{} AS {}",
            request.reference().quoted_name(),
            build,
            Self::create_refresh_schedule_clause(request.schedule),
            partition,
            distribution,
            sort,
            properties,
            request.select_sql()?,
        ))
    }

    fn create_refresh_schedule_clause(schedule: RefreshSchedule) -> String {
        match schedule {
            RefreshSchedule::Manual => "MANUAL".to_string(),
            RefreshSchedule::Scheduled { interval, unit } => {
                format!("SCHEDULE EVERY (INTERVAL {interval} {})", Self::refresh_unit_keyword(unit))
            },
        }
    }

    fn sql_literal(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }
}
