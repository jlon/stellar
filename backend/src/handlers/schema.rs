use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::Arc};
use stellar_macros::app_db;

use crate::{
    AppState,
    db::AppDb,
    handlers::frontend::validate_selected_cluster,
    middleware::OrgContext,
    models::{
        Cluster, ClusterType, DependencyEvidence, DependencySource, MaterializedViewKind,
        MaterializedViewRef, RelationKind, SchemaColumn, SchemaDependencyDirection,
        SchemaObjectDependencies, SchemaObjectDependency, SchemaObjectDetail, SchemaObjectIdentity,
        SchemaObjectKind, SchemaObjectSummary, SchemaParseStatus, SchemaPhysicalProperties,
        SchemaRelationKind,
    },
    services::{
        MySQLClient, cluster_timeout, create_adapter,
        op_audit::{OpAuditEntry, log_op_best_effort},
    },
    utils::{ApiError, ApiResult},
};

const MAX_DDL_BYTES: usize = 1_048_576;
const MAX_DEPENDENCIES_PER_DIRECTION: usize = 25;
/// The graph includes the selected object, leaving at most 49 dependency nodes.
const MAX_SCHEMA_DEPENDENCY_NODES: usize = 49;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaObjectListQuery {
    pub catalog: Option<String>,
    pub database: String,
}

/// Lists objects in one database and issues opaque, user-bound detail references.
#[utoipa::path(
    get,
    path = "/api/clusters/{cluster_id}/schema/objects",
    params(
        ("cluster_id" = i64, Path, description = "Active cluster ID"),
        ("catalog" = Option<String>, Query, description = "Catalog name"),
        ("database" = String, Query, description = "Database name")
    ),
    responses((status = 200, description = "Schema objects", body = Vec<SchemaObjectSummary>)),
    security(("bearer_auth" = [])),
    tag = "Schema"
)]
#[app_db]
pub async fn list_schema_objects(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Path(cluster_id): Path<i64>,
    Query(query): Query<SchemaObjectListQuery>,
) -> ApiResult<Json<Vec<SchemaObjectSummary>>> {
    let cluster = selected_cluster(&state, &org_ctx, cluster_id).await?;
    let catalog = query.catalog.unwrap_or_else(|| cluster.catalog.clone());
    validate_identifier("catalog", &catalog)?;
    validate_identifier("database", &query.database)?;

    let objects = list_current_objects(&state, &cluster, &catalog, &query.database).await?;
    let response = objects
        .into_iter()
        .map(|(name, object_kind)| {
            let identity = SchemaObjectIdentity {
                cluster_id,
                catalog: catalog.clone(),
                database: query.database.clone(),
                name: name.clone(),
                object_kind,
            };
            Ok(SchemaObjectSummary {
                name,
                object_kind,
                object_ref: state
                    .schema_object_reference_store
                    .issue(identity, &org_ctx)?,
            })
        })
        .collect::<ApiResult<Vec<_>>>()?;
    Ok(Json(response))
}

/// Reads one object from a server-issued reference only.
#[utoipa::path(
    get,
    path = "/api/clusters/{cluster_id}/schema/objects/{object_ref}",
    params(
        ("cluster_id" = i64, Path, description = "Active cluster ID"),
        ("object_ref" = String, Path, description = "Opaque object reference")
    ),
    responses((status = 200, description = "Schema object detail", body = SchemaObjectDetail)),
    security(("bearer_auth" = [])),
    tag = "Schema"
)]
#[app_db]
pub async fn get_schema_object(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Path((cluster_id, object_ref)): Path<(i64, String)>,
) -> ApiResult<Json<SchemaObjectDetail>> {
    let cluster = selected_cluster(&state, &org_ctx, cluster_id).await?;
    let identity =
        state
            .schema_object_reference_store
            .resolve(&object_ref, cluster_id, &org_ctx)?;
    Ok(Json(read_schema_object(&state, &cluster, identity).await?))
}

/// Re-reads a single referenced object and records the user-triggered refresh.
#[utoipa::path(
    post,
    path = "/api/clusters/{cluster_id}/schema/objects/{object_ref}/refresh",
    params(
        ("cluster_id" = i64, Path, description = "Active cluster ID"),
        ("object_ref" = String, Path, description = "Opaque object reference")
    ),
    responses((status = 200, description = "Refreshed schema object", body = SchemaObjectDetail)),
    security(("bearer_auth" = [])),
    tag = "Schema"
)]
#[app_db]
pub async fn refresh_schema_object(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Path((cluster_id, object_ref)): Path<(i64, String)>,
) -> ApiResult<Json<SchemaObjectDetail>> {
    let cluster = selected_cluster(&state, &org_ctx, cluster_id).await?;
    let identity =
        state
            .schema_object_reference_store
            .resolve(&object_ref, cluster_id, &org_ctx)?;
    let detail = read_schema_object(&state, &cluster, identity).await?;
    let target_name = display_identity(&detail.identity);
    log_op_best_effort(
        &state.db,
        OpAuditEntry {
            user_id: org_ctx.user_id,
            username: &org_ctx.username,
            organization_id: org_ctx.organization_id,
            action: "schema:refresh",
            target_type: "schema_object",
            target_id: Some(cluster_id),
            target_name: &target_name,
        },
    )
    .await;
    Ok(Json(detail))
}

/// Returns a bounded, one-hop dependency map from engine metadata only.
#[utoipa::path(
    get,
    path = "/api/clusters/{cluster_id}/schema/objects/{object_ref}/dependencies",
    params(
        ("cluster_id" = i64, Path, description = "Active cluster ID"),
        ("object_ref" = String, Path, description = "Opaque object reference")
    ),
    responses((status = 200, description = "One-hop schema dependencies", body = SchemaObjectDependencies)),
    security(("bearer_auth" = [])),
    tag = "Schema"
)]
#[app_db]
pub async fn get_schema_object_dependencies(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<OrgContext>,
    Path((cluster_id, object_ref)): Path<(i64, String)>,
) -> ApiResult<Json<SchemaObjectDependencies>> {
    let cluster = selected_cluster(&state, &org_ctx, cluster_id).await?;
    let identity =
        state
            .schema_object_reference_store
            .resolve(&object_ref, cluster_id, &org_ctx)?;
    ensure_schema_object_exists(&state, &cluster, &identity).await?;
    Ok(Json(read_schema_object_dependencies(&state, &cluster, identity, &org_ctx).await?))
}

#[app_db]
async fn selected_cluster<DB: AppDb>(
    state: &AppState<DB>,
    org_ctx: &OrgContext,
    cluster_id: i64,
) -> ApiResult<Cluster> {
    let cluster = state.cluster_service.get_cluster(cluster_id).await?;
    validate_selected_cluster(&cluster, org_ctx)?;
    Ok(cluster)
}

#[app_db]
async fn read_schema_object_dependencies<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    identity: SchemaObjectIdentity,
    org_ctx: &OrgContext,
) -> ApiResult<SchemaObjectDependencies> {
    match cluster.cluster_type {
        ClusterType::StarRocks => {
            read_starrocks_schema_dependencies(state, cluster, identity, org_ctx).await
        },
        ClusterType::Doris => {
            read_doris_schema_dependencies(state, cluster, identity, org_ctx).await
        },
    }
}

#[app_db]
async fn read_starrocks_schema_dependencies<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    identity: SchemaObjectIdentity,
    org_ctx: &OrgContext,
) -> ApiResult<SchemaObjectDependencies> {
    const UPSTREAM_SQL: &str = "SELECT ref_object_catalog, ref_object_database, ref_object_name, ref_object_type \
        FROM sys.object_dependencies \
        WHERE object_catalog = ? AND object_database = ? AND object_name = ? \
        ORDER BY ref_object_catalog, ref_object_database, ref_object_name \
        LIMIT 26";
    const DOWNSTREAM_SQL: &str = "SELECT object_catalog, object_database, object_name, object_type \
        FROM sys.object_dependencies \
        WHERE ref_object_catalog = ? AND ref_object_database = ? AND ref_object_name = ? \
        ORDER BY object_catalog, object_database, object_name \
        LIMIT 26";

    let pool = state.mysql_pool_manager.get_pool(cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
    let mut session = client.create_session().await?;
    let params = (identity.catalog.clone(), identity.database.clone(), identity.name.clone());
    let mut warnings = Vec::new();
    let mut complete = identity.object_kind == SchemaObjectKind::MaterializedView;
    if !complete {
        warnings.push(
            "StarRocks engine metadata currently verifies asynchronous materialized-view edges only; this object's map may be incomplete."
                .to_string(),
        );
    }

    let upstream = match session
        .query_with_params(UPSTREAM_SQL, params.clone())
        .await
    {
        Ok((_, rows)) => rows,
        Err(error) => {
            complete = false;
            warnings.push(format!("Unable to read upstream engine dependencies: {error}"));
            Vec::new()
        },
    };
    let downstream = match session.query_with_params(DOWNSTREAM_SQL, params).await {
        Ok((_, rows)) => rows,
        Err(error) => {
            complete = false;
            warnings.push(format!("Unable to read downstream engine dependencies: {error}"));
            Vec::new()
        },
    };

    let observed_at = Utc::now();
    let mut dependencies = Vec::new();
    let (upstream_dependencies, upstream_warnings, upstream_complete) =
        parse_starrocks_dependencies(
            upstream,
            SchemaDependencyDirection::Upstream,
            &identity,
            org_ctx,
            &state.schema_object_reference_store,
            observed_at,
        )?;
    dependencies.extend(upstream_dependencies);
    warnings.extend(upstream_warnings);
    complete &= upstream_complete;
    let (downstream_dependencies, downstream_warnings, downstream_complete) =
        parse_starrocks_dependencies(
            downstream,
            SchemaDependencyDirection::Downstream,
            &identity,
            org_ctx,
            &state.schema_object_reference_store,
            observed_at,
        )?;
    dependencies.extend(downstream_dependencies);
    warnings.extend(downstream_warnings);
    complete &= downstream_complete;
    if dependencies.len() > MAX_SCHEMA_DEPENDENCY_NODES {
        dependencies.truncate(MAX_SCHEMA_DEPENDENCY_NODES);
        complete = false;
        warnings.push(format!(
            "Dependencies exceeded the {} object graph limit.",
            MAX_SCHEMA_DEPENDENCY_NODES + 1,
        ));
    }

    Ok(SchemaObjectDependencies {
        object: identity,
        dependencies,
        complete,
        warnings,
        read_at: Utc::now(),
    })
}

fn parse_starrocks_dependencies(
    rows: Vec<Vec<String>>,
    direction: SchemaDependencyDirection,
    source: &SchemaObjectIdentity,
    org_ctx: &OrgContext,
    reference_store: &crate::services::SchemaObjectReferenceStore,
    observed_at: chrono::DateTime<Utc>,
) -> ApiResult<(Vec<SchemaObjectDependency>, Vec<String>, bool)> {
    let mut dependencies = Vec::new();
    let mut warnings = Vec::new();
    let mut complete = true;
    if rows.len() > MAX_DEPENDENCIES_PER_DIRECTION {
        complete = false;
        warnings.push(format!(
            "{} dependencies exceeded the {} object limit.",
            match direction {
                SchemaDependencyDirection::Upstream => "Upstream",
                SchemaDependencyDirection::Downstream => "Downstream",
            },
            MAX_DEPENDENCIES_PER_DIRECTION,
        ));
    }

    for row in rows.into_iter().take(MAX_DEPENDENCIES_PER_DIRECTION) {
        let Some(object) = dependency_identity_from_row(source.cluster_id, &row) else {
            complete = false;
            warnings.push("An engine dependency row had an invalid object identity.".to_string());
            continue;
        };
        dependencies.push(SchemaObjectDependency {
            direction,
            relation_kind: SchemaRelationKind::MvReads,
            object_ref: reference_store.issue(object.clone(), org_ctx)?,
            object,
            object_parse_status: None,
            evidence: DependencyEvidence::Verified,
            source: DependencySource::StarRocksObjectDependencies,
            evidence_snippet: Some("sys.object_dependencies".to_string()),
            observed_at,
        });
    }
    Ok((dependencies, warnings, complete))
}

fn dependency_identity_from_row(cluster_id: i64, row: &[String]) -> Option<SchemaObjectIdentity> {
    let catalog = row.first()?.trim().to_string();
    let database = row.get(1)?.trim().to_string();
    let name = row.get(2)?.trim().to_string();
    let object_kind = row.get(3).map(String::as_str).map(object_kind_from_type)?;
    if validate_identifier("catalog", &catalog).is_err()
        || validate_identifier("database", &database).is_err()
        || validate_identifier("object", &name).is_err()
    {
        return None;
    }
    Some(SchemaObjectIdentity { cluster_id, catalog, database, name, object_kind })
}

#[app_db]
async fn read_doris_schema_dependencies<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    identity: SchemaObjectIdentity,
    org_ctx: &OrgContext,
) -> ApiResult<SchemaObjectDependencies> {
    let mut warnings = vec![
        "Doris currently exposes only bounded, parsed upstream dependencies for asynchronous materialized views; downstream edges are unavailable."
            .to_string(),
    ];
    let observed_at = Utc::now();
    let mut dependencies = Vec::new();
    if identity.object_kind == SchemaObjectKind::MaterializedView {
        let adapter = create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
        let reference = MaterializedViewRef {
            database: identity.database.clone(),
            name: identity.name.clone(),
            kind: MaterializedViewKind::Async,
        };
        match adapter.get_materialized_view_dependencies(&reference).await {
            Ok(result) => {
                let dependency_count = result.dependencies.len();
                warnings.extend(result.warnings);
                for dependency in result
                    .dependencies
                    .into_iter()
                    .take(MAX_DEPENDENCIES_PER_DIRECTION)
                {
                    let object = SchemaObjectIdentity {
                        cluster_id: identity.cluster_id,
                        catalog: dependency
                            .object
                            .catalog
                            .unwrap_or_else(|| identity.catalog.clone()),
                        database: dependency
                            .object
                            .database
                            .unwrap_or_else(|| identity.database.clone()),
                        name: dependency.object.name,
                        object_kind: schema_kind_from_relation_kind(dependency.object.kind),
                    };
                    if validate_identifier("catalog", &object.catalog).is_err()
                        || validate_identifier("database", &object.database).is_err()
                        || validate_identifier("object", &object.name).is_err()
                    {
                        warnings.push(
                            "A parsed Doris dependency had an invalid object identity.".to_string(),
                        );
                        continue;
                    }
                    dependencies.push(SchemaObjectDependency {
                        direction: SchemaDependencyDirection::Upstream,
                        relation_kind: SchemaRelationKind::MvReads,
                        object_ref: state
                            .schema_object_reference_store
                            .issue(object.clone(), org_ctx)?,
                        object,
                        object_parse_status: None,
                        evidence: dependency.evidence,
                        source: dependency.source,
                        evidence_snippet: dependency.evidence_snippet,
                        observed_at,
                    });
                }
                if dependency_count > MAX_DEPENDENCIES_PER_DIRECTION {
                    warnings.push(format!(
                        "Upstream dependencies exceeded the {} object limit.",
                        MAX_DEPENDENCIES_PER_DIRECTION,
                    ));
                }
            },
            Err(error) => warnings
                .push(format!("Unable to read Doris materialized-view dependencies: {error}")),
        }
    } else {
        warnings.push(
            "Doris view and table dependencies require the shared metadata sync and are not available yet."
                .to_string(),
        );
    }

    Ok(SchemaObjectDependencies {
        object: identity,
        dependencies,
        complete: false,
        warnings,
        read_at: Utc::now(),
    })
}

fn schema_kind_from_relation_kind(kind: RelationKind) -> SchemaObjectKind {
    match kind {
        RelationKind::Table => SchemaObjectKind::Table,
        RelationKind::View => SchemaObjectKind::View,
        RelationKind::MaterializedView => SchemaObjectKind::MaterializedView,
        RelationKind::Unknown => SchemaObjectKind::Table,
    }
}

#[app_db]
async fn list_current_objects<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    catalog: &str,
    database: &str,
) -> ApiResult<Vec<(String, SchemaObjectKind)>> {
    let pool = state.mysql_pool_manager.get_pool(cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
    let mut session = client.create_session().await?;
    session.use_catalog(catalog, &cluster.cluster_type).await?;

    let database_identifier = quote_identifier(database)?;
    let (columns, rows, _) = match session
        .execute(&format!("SHOW FULL TABLES FROM {database_identifier}"))
        .await
    {
        Ok(result) => result,
        Err(full_tables_error) => {
            tracing::debug!("SHOW FULL TABLES is unavailable: {}", full_tables_error);
            session
                .execute(&format!("SHOW TABLES FROM {database_identifier}"))
                .await?
        },
    };
    let name_index = columns
        .iter()
        .position(|column| !column.eq_ignore_ascii_case("Table_type"))
        .unwrap_or(0);
    let type_index = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("Table_type"));

    let mut type_map = table_type_map(&mut session, database).await;
    for (name, object_kind) in materialized_view_type_map(&mut session, database).await {
        type_map.insert(name, object_kind);
    }

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = row.get(name_index)?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let kind = type_map.get(&name).copied().unwrap_or_else(|| {
                type_index
                    .and_then(|index| row.get(index))
                    .map(|value| object_kind_from_type(value))
                    .unwrap_or(SchemaObjectKind::Table)
            });
            Some((name, kind))
        })
        .collect())
}

async fn table_type_map(
    session: &mut crate::services::mysql_client::MySQLSession,
    database: &str,
) -> BTreeMap<String, SchemaObjectKind> {
    let sql = "SELECT TABLE_NAME, TABLE_TYPE FROM information_schema.tables WHERE TABLE_SCHEMA = ?";
    match session
        .query_with_params(sql, (database.to_string(),))
        .await
    {
        Ok((_, rows)) => rows
            .into_iter()
            .filter_map(|row| {
                let name = row.first()?.trim().to_string();
                (!name.is_empty()).then(|| {
                    (
                        name,
                        object_kind_from_type(row.get(1).map(String::as_str).unwrap_or_default()),
                    )
                })
            })
            .collect(),
        Err(error) => {
            tracing::debug!("Unable to classify schema objects from information_schema: {}", error);
            BTreeMap::new()
        },
    }
}

async fn materialized_view_type_map(
    session: &mut crate::services::mysql_client::MySQLSession,
    database: &str,
) -> BTreeMap<String, SchemaObjectKind> {
    let sql = "SELECT TABLE_NAME FROM information_schema.materialized_views WHERE TABLE_SCHEMA = ?";
    match session
        .query_with_params(sql, (database.to_string(),))
        .await
    {
        Ok((_, rows)) => rows
            .into_iter()
            .filter_map(|row| {
                let name = row.first()?.trim().to_string();
                (!name.is_empty()).then_some((name, SchemaObjectKind::MaterializedView))
            })
            .collect(),
        Err(error) => {
            tracing::debug!(
                "Unable to classify materialized views from information_schema: {}",
                error
            );
            BTreeMap::new()
        },
    }
}

#[app_db]
async fn read_schema_object<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    identity: SchemaObjectIdentity,
) -> ApiResult<SchemaObjectDetail> {
    let pool = state.mysql_pool_manager.get_pool(cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
    let mut session = client.create_session().await?;
    session
        .use_catalog(&identity.catalog, &cluster.cluster_type)
        .await?;

    let ddl_sql = show_create_sql(&identity)?;
    let (ddl_columns, ddl_rows, _) = session.execute(&ddl_sql).await?;
    let ddl_raw = extract_ddl(&ddl_columns, &ddl_rows)?;
    if ddl_raw.len() > MAX_DDL_BYTES {
        return Err(ApiError::invalid_data("Object DDL exceeds the Schema Explorer size limit"));
    }

    let mut warnings = Vec::new();
    let columns = match read_columns(&mut session, &identity).await {
        Ok(columns) => columns,
        Err(error) => {
            tracing::warn!(
                object = %display_identity(&identity),
                "Schema object DDL was read but column metadata could not be read: {}",
                error
            );
            warnings.push(
                "Column metadata could not be read; the original DDL remains available."
                    .to_string(),
            );
            Vec::new()
        },
    };
    let physical_properties = parse_physical_properties(&ddl_raw);
    let parse_status = parse_status(&identity.object_kind, &columns, physical_properties.as_ref());

    Ok(SchemaObjectDetail {
        identity,
        columns,
        physical_properties,
        ddl_raw,
        parse_status,
        read_at: Utc::now(),
        warnings,
    })
}

/// Reject a still-valid reference when its engine object was deleted or replaced.
#[app_db]
async fn ensure_schema_object_exists<DB: AppDb>(
    state: &AppState<DB>,
    cluster: &Cluster,
    identity: &SchemaObjectIdentity,
) -> ApiResult<()> {
    let pool = state.mysql_pool_manager.get_pool(cluster).await?;
    let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(cluster));
    let mut session = client.create_session().await?;
    session
        .use_catalog(&identity.catalog, &cluster.cluster_type)
        .await?;
    let (columns, rows, _) = session.execute(&show_create_sql(identity)?).await?;
    extract_ddl(&columns, &rows).map(|_| ())
}

async fn read_columns(
    session: &mut crate::services::mysql_client::MySQLSession,
    identity: &SchemaObjectIdentity,
) -> ApiResult<Vec<SchemaColumn>> {
    let sql = "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, COLUMN_COMMENT, COLUMN_KEY \
               FROM information_schema.columns WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION";
    let (_, rows) = session
        .query_with_params(sql, (identity.database.clone(), identity.name.clone()))
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let name = row.first()?.trim().to_string();
            (!name.is_empty()).then(|| SchemaColumn {
                name,
                data_type: row.get(1).cloned().unwrap_or_default(),
                nullable: row.get(2).and_then(|value| {
                    match value.trim().to_ascii_uppercase().as_str() {
                        "YES" => Some(true),
                        "NO" => Some(false),
                        _ => None,
                    }
                }),
                default_value: row.get(3).filter(|value| !value.is_empty()).cloned(),
                comment: row.get(4).filter(|value| !value.is_empty()).cloned(),
                key: row.get(5).filter(|value| !value.is_empty()).cloned(),
            })
        })
        .collect())
}

pub(crate) fn show_create_sql(identity: &SchemaObjectIdentity) -> ApiResult<String> {
    let object =
        format!("{}.{}", quote_identifier(&identity.database)?, quote_identifier(&identity.name)?);
    let command = match identity.object_kind {
        SchemaObjectKind::Table | SchemaObjectKind::ExternalTable => "SHOW CREATE TABLE",
        SchemaObjectKind::View => "SHOW CREATE VIEW",
        SchemaObjectKind::MaterializedView => "SHOW CREATE MATERIALIZED VIEW",
    };
    Ok(format!("{command} {object}"))
}

pub(crate) fn extract_ddl(columns: &[String], rows: &[Vec<String>]) -> ApiResult<String> {
    let row = rows
        .first()
        .ok_or_else(|| ApiError::not_found("Schema object no longer exists"))?;
    let ddl_index = columns
        .iter()
        .position(|column| column.to_ascii_lowercase().starts_with("create "))
        .or_else(|| (row.len() > 1).then_some(1))
        .unwrap_or(0);
    row.get(ddl_index)
        .filter(|ddl| !ddl.trim().is_empty())
        .cloned()
        .ok_or_else(|| ApiError::not_found("Schema object DDL is unavailable"))
}

pub(crate) fn parse_physical_properties(ddl: &str) -> Option<SchemaPhysicalProperties> {
    let key_model =
        line_clause(ddl, &["DUPLICATE KEY", "UNIQUE KEY", "PRIMARY KEY", "AGGREGATE KEY"]);
    let partition = line_clause(ddl, &["PARTITION BY"]);
    let distribution = line_clause(ddl, &["DISTRIBUTED BY"]);
    let engine = ddl
        .lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix("ENGINE=")
                .or_else(|| line.strip_prefix("ENGINE ="))
        })
        .map(|value| {
            value
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_matches('`')
                .to_string()
        })
        .filter(|value| !value.is_empty());
    let properties = parse_properties(ddl);

    (key_model.is_some()
        || partition.is_some()
        || distribution.is_some()
        || engine.is_some()
        || !properties.is_empty())
    .then_some(SchemaPhysicalProperties { key_model, partition, distribution, engine, properties })
}

fn line_clause(ddl: &str, prefixes: &[&str]) -> Option<String> {
    ddl.lines()
        .map(str::trim)
        .find(|line| {
            prefixes
                .iter()
                .any(|prefix| line.to_ascii_uppercase().starts_with(prefix))
        })
        .map(ToString::to_string)
}

fn parse_properties(ddl: &str) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    let Some(start) = ddl.to_ascii_uppercase().find("PROPERTIES") else {
        return properties;
    };
    let Some(open) = ddl[start..].find('(').map(|offset| start + offset) else {
        return properties;
    };
    let Some(close) = ddl[open + 1..].find(')').map(|offset| open + 1 + offset) else {
        return properties;
    };

    for pair in ddl[open + 1..close].split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches(['"', '\'']);
        let value = value.trim().trim_matches(['"', '\'']);
        if !key.is_empty() {
            properties.insert(key.to_string(), value.to_string());
        }
    }
    properties
}

fn parse_status(
    object_kind: &SchemaObjectKind,
    columns: &[SchemaColumn],
    physical_properties: Option<&SchemaPhysicalProperties>,
) -> SchemaParseStatus {
    match object_kind {
        SchemaObjectKind::View if !columns.is_empty() => SchemaParseStatus::NotApplicable,
        SchemaObjectKind::View => SchemaParseStatus::RawOnly,
        _ if columns.is_empty() && physical_properties.is_none() => SchemaParseStatus::RawOnly,
        _ if physical_properties.is_some() => SchemaParseStatus::Parsed,
        _ => SchemaParseStatus::Partial,
    }
}

fn object_kind_from_type(table_type: &str) -> SchemaObjectKind {
    let table_type = table_type.trim().to_ascii_uppercase();
    if table_type.contains("MATERIALIZED") {
        SchemaObjectKind::MaterializedView
    } else if table_type.contains("VIEW") {
        SchemaObjectKind::View
    } else if table_type.contains("EXTERNAL") {
        SchemaObjectKind::ExternalTable
    } else {
        SchemaObjectKind::Table
    }
}

fn quote_identifier(value: &str) -> ApiResult<String> {
    validate_identifier("object", value)?;
    Ok(format!("`{}`", value.replace('`', "``")))
}

fn validate_identifier(label: &str, value: &str) -> ApiResult<()> {
    if value.is_empty()
        || value.len() > 256
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::invalid_data(format!("Invalid {label} identifier")));
    }
    Ok(())
}

fn display_identity(identity: &SchemaObjectIdentity) -> String {
    format!("{}.{}.{}", identity.catalog, identity.database, identity.name)
}
