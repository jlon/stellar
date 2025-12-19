use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

use crate::AppState;
use crate::models::{
    AddSqlBlacklistRequest, CatalogWithDatabases, CatalogsWithDatabasesResponse, Query,
    QueryExecuteRequest, QueryExecuteResponse, SingleQueryResult, SqlBlacklistItem, TableMetadata,
    TableObjectType,
};
use crate::services::create_adapter;
use crate::services::mysql_client::MySQLClient;
use crate::utils::{ApiError, ApiResult};

// Get list of catalogs using MySQL client
#[utoipa::path(
    get,
    path = "/api/clusters/catalogs",
    responses(
        (status = 200, description = "List of catalogs", body = Vec<String>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn list_catalogs(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<String>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    let catalogs = adapter.list_catalogs().await?;

    tracing::debug!("Found {} catalogs via adapter", catalogs.len());
    Ok(Json(catalogs))
}

// Get list of databases in a catalog using MySQL client
#[utoipa::path(
    get,
    path = "/api/clusters/databases",
    params(
        ("catalog" = Option<String>, Query, description = "Catalog name (optional)")
    ),
    responses(
        (status = 200, description = "List of databases", body = Vec<String>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn list_databases(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Vec<String>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    let catalog = params.get("catalog").map(|s| s.as_str());
    let mut databases = adapter.list_databases(catalog).await?;

    databases.retain(|name| name != "information_schema" && name != "_statistics_");

    tracing::debug!("Found {} databases via adapter", databases.len());
    Ok(Json(databases))
}

// Get list of tables within a database (optional catalog) using MySQL client
#[utoipa::path(
    get,
    path = "/api/clusters/tables",
    params(
        ("catalog" = Option<String>, Query, description = "Catalog name (optional)"),
        ("database" = String, Query, description = "Database name")
    ),
    responses(
        (status = 200, description = "List of tables", body = Vec<TableMetadata>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn list_tables(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Vec<TableMetadata>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);

    let database_name = match params.get("database") {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            tracing::warn!("Database parameter missing when listing tables");
            return Ok(Json(Vec::new()));
        },
    };

    let catalog_name = params.get("catalog").cloned();
    let mut session = mysql_client.create_session().await?;

    let show_tables_sql = match catalog_name.as_deref() {
        Some(catalog) if catalog != "default_catalog" => {
            format!("SHOW TABLES FROM {}.{}", catalog, database_name)
        },
        _ => format!("SHOW TABLES FROM {}", database_name),
    };

    let mut tables = match session.execute(&show_tables_sql).await {
        Ok((_, rows, _)) => rows
            .into_iter()
            .filter_map(|row| {
                let name = row
                    .first()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                if name.is_empty() {
                    None
                } else {
                    Some(TableMetadata { name, object_type: TableObjectType::Table })
                }
            })
            .collect::<Vec<_>>(),
        Err(err) => {
            tracing::warn!(
                "Failed to execute '{}': {}. Falling back to information_schema query.",
                show_tables_sql,
                err
            );

            let table_sql = r#"
                SELECT 
                    t.TABLE_NAME,
                    CASE
                        WHEN t.TABLE_TYPE = 'TABLE' OR t.TABLE_TYPE = 'BASE TABLE' THEN 'TABLE'
                        WHEN mv.TABLE_NAME IS NOT NULL THEN 'MATERIALIZED_VIEW'
                        ELSE 'VIEW'
                    END AS OBJECT_TYPE
                FROM information_schema.tables t
                LEFT JOIN information_schema.materialized_views mv
                    ON t.TABLE_SCHEMA = mv.TABLE_SCHEMA
                   AND t.TABLE_NAME = mv.TABLE_NAME
                WHERE t.TABLE_SCHEMA = ?
                ORDER BY t.TABLE_NAME
            "#;

            match session
                .query_with_params(table_sql, (database_name.clone(),))
                .await
            {
                Ok((_, rows)) => rows
                    .into_iter()
                    .filter_map(|row| {
                        let name = row
                            .first()
                            .map(|s| s.trim().to_string())
                            .unwrap_or_default();
                        if name.is_empty() {
                            return None;
                        }
                        let object_type_raw = row
                            .get(1)
                            .map(|s| s.trim().to_uppercase())
                            .unwrap_or_default();
                        let object_type = match object_type_raw.as_str() {
                            "MATERIALIZED_VIEW" => TableObjectType::MaterializedView,
                            "VIEW" => TableObjectType::View,
                            _ => TableObjectType::Table,
                        };
                        Some(TableMetadata { name, object_type })
                    })
                    .collect::<Vec<_>>(),
                Err(info_schema_err) => {
                    tracing::error!(
                        "Failed to retrieve tables for {} ({}): {} / {}",
                        database_name,
                        catalog_name.as_deref().unwrap_or("default_catalog"),
                        err,
                        info_schema_err
                    );
                    Vec::new()
                },
            }
        },
    };

    if !tables.is_empty()
        && let Ok((_, info_rows)) = session
            .query_with_params(
                r#"
                SELECT 
                    t.TABLE_NAME,
                    CASE
                        WHEN t.TABLE_TYPE = 'TABLE' OR t.TABLE_TYPE = 'BASE TABLE' THEN 'TABLE'
                        WHEN mv.TABLE_NAME IS NOT NULL THEN 'MATERIALIZED_VIEW'
                        ELSE 'VIEW'
                    END AS OBJECT_TYPE
                FROM information_schema.tables t
                LEFT JOIN information_schema.materialized_views mv
                    ON t.TABLE_SCHEMA = mv.TABLE_SCHEMA
                   AND t.TABLE_NAME = mv.TABLE_NAME
                WHERE t.TABLE_SCHEMA = ?
            "#,
                (database_name.clone(),),
            )
            .await
    {
        let type_map: std::collections::HashMap<_, _> = info_rows
            .into_iter()
            .filter_map(|row| {
                let name = row.first().map(|s| s.trim().to_string())?;
                let object_type_raw = row.get(1).map(|s| s.trim().to_uppercase())?;
                let object_type = match object_type_raw.as_str() {
                    "MATERIALIZED_VIEW" => TableObjectType::MaterializedView,
                    "VIEW" => TableObjectType::View,
                    _ => TableObjectType::Table,
                };
                Some((name, object_type))
            })
            .collect();

        for table in tables.iter_mut() {
            if let Some(object_type) = type_map.get(&table.name) {
                table.object_type = *object_type;
            }
        }
    }

    tracing::debug!(
        "Database {}{} returned {} tables via MySQL client (with type metadata)",
        catalog_name
            .as_ref()
            .map(|c| format!("{}.", c))
            .unwrap_or_default(),
        database_name,
        tables.len()
    );

    Ok(Json(tables))
}

// Get all catalogs with their databases using MySQL client (one-time response)
#[utoipa::path(
    get,
    path = "/api/clusters/catalogs-databases",
    responses(
        (status = 200, description = "All catalogs with their databases", body = CatalogsWithDatabasesResponse),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn list_catalogs_with_databases(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<CatalogsWithDatabasesResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let adapter =
        crate::services::create_adapter(cluster.clone(), state.mysql_pool_manager.clone());
    let catalog_names = adapter.list_catalogs().await?;

    tracing::debug!("Found {} catalogs, fetching databases for each...", catalog_names.len());

    let mut catalogs = Vec::new();
    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);

    let mut session = mysql_client.create_session().await?;
    for catalog_name in &catalog_names {
        let show_db_sql = format!("SHOW DATABASES FROM {}", catalog_name);
        let (_, db_rows, _) = match session.execute(&show_db_sql).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("Failed to get databases for catalog {}: {}", catalog_name, e);
                catalogs.push(CatalogWithDatabases {
                    catalog: catalog_name.clone(),
                    databases: Vec::new(),
                });
                continue;
            },
        };

        let mut databases = Vec::new();
        for row in db_rows {
            if let Some(db_name) = row.first() {
                let name = db_name.trim().to_string();

                if !name.is_empty() && name != "information_schema" && name != "_statistics_" {
                    databases.push(name);
                }
            }
        }

        tracing::debug!("Catalog {} has {} databases", catalog_name, databases.len());

        catalogs.push(CatalogWithDatabases { catalog: catalog_name.clone(), databases });
    }

    Ok(Json(CatalogsWithDatabasesResponse { catalogs }))
}

// Get all running queries for a cluster
#[utoipa::path(
    get,
    path = "/api/clusters/queries",
    responses(
        (status = 200, description = "List of running queries", body = Vec<Query>),
        (status = 404, description = "No active cluster found")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn list_queries(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<Query>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };
    let adapter = create_adapter(cluster, state.mysql_pool_manager.clone());
    let queries = adapter.get_queries().await?;
    Ok(Json(queries))
}

// Kill a query
#[utoipa::path(
    delete,
    path = "/api/clusters/queries/{query_id}",
    params(
        ("query_id" = String, Path, description = "Query ID")
    ),
    responses(
        (status = 200, description = "Query killed successfully"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn kill_query(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(query_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let valid_query_id = query_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !valid_query_id || query_id.is_empty() || query_id.len() > 64 {
        return Err(ApiError::validation_error("Invalid query ID format"));
    }

    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);

    let sql = format!("KILL QUERY '{}'", query_id);
    mysql_client.execute(&sql).await?;

    Ok((StatusCode::OK, Json(json!({ "message": "Query killed successfully" }))))
}

// Execute SQL query
// If database is provided, will execute USE database before the SQL query
#[utoipa::path(
    post,
    path = "/api/clusters/queries/execute",
    request_body = QueryExecuteRequest,
    responses(
        (status = 200, description = "Query executed successfully", body = QueryExecuteResponse),
        (status = 400, description = "Invalid SQL or query error"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "Queries"
)]
pub async fn execute_sql(
    State(state): State<Arc<crate::AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Json(request): Json<QueryExecuteRequest>,
) -> ApiResult<Json<QueryExecuteResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let pool: mysql_async::Pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql_client = MySQLClient::from_pool(pool);

    let sql_statements = parse_sql_statements(&request.sql);

    let sql_statements: Vec<String> = sql_statements.into_iter().take(5).collect();

    if sql_statements.is_empty() {
        return Ok(Json(QueryExecuteResponse { results: Vec::new(), total_execution_time_ms: 0 }));
    }

    let mut session = mysql_client.create_session().await?;

    if let Some(cat) = request.catalog.as_ref().filter(|c| !c.is_empty()) {
        session.use_catalog(cat, &cluster.cluster_type).await?;
    }

    if let Some(db) = request.database.as_ref().filter(|d| !d.is_empty()) {
        session.use_database(db).await?;
    }

    let total_start = Instant::now();
    let mut results = Vec::new();

    for sql in sql_statements {
        if sql.is_empty() {
            continue;
        }

        let sql_with_limit = apply_query_limit(&sql, request.limit.unwrap_or(1000));

        use crate::models::cluster::ClusterType;
        let mut query_result = if cluster.cluster_type == ClusterType::Doris {
            if sql_with_limit.to_uppercase().contains("INFORMATION_SCHEMA.LOADS") {
                handle_loads_query_for_doris(&sql_with_limit, &mut session, request.database.as_deref()).await
            } else if sql_with_limit.to_uppercase().contains("SHOW PROC") && sql_with_limit.to_uppercase().contains("'/COMPACTIONS'") {
                handle_compactions_query_for_doris(&mut session).await
            } else {
                let adapted_sql = adapt_sql_for_doris(&sql_with_limit);
                session.execute(&adapted_sql).await
            }
        } else {
            session.execute(&sql_with_limit).await
        };

        if let Err(ref error) = query_result {
            let error_msg = error.to_string();
            if cluster.cluster_type == ClusterType::Doris 
                && error_msg.contains("does not exist") 
                && error_msg.contains("Table [") {
                let normalized_sql = normalize_table_names_for_doris(&sql_with_limit);
                if normalized_sql != sql_with_limit {
                    tracing::info!("[Doris] Retrying SQL with normalized table names (lowercase)");
                    query_result = session.execute(&normalized_sql).await;
                }
            }
        }

        match query_result {
            Ok((columns, data_rows, execution_time_ms)) => {
                let row_count = data_rows.len();
                results.push(SingleQueryResult {
                    sql,
                    columns,
                    rows: data_rows,
                    row_count,
                    execution_time_ms,
                    success: true,
                    error: None,
                });
            },
            Err(e) => {
                let mut error_msg = e.to_string();
                
                if cluster.cluster_type == ClusterType::Doris 
                    && error_msg.contains("does not exist") 
                    && error_msg.contains("Table [") {
                    error_msg = format!("{} (Note: Doris is case-sensitive for table names. Please use the exact case as shown in SHOW TABLES.)", error_msg);
                }
                
                results.push(SingleQueryResult {
                    sql,
                    columns: Vec::new(),
                    rows: Vec::new(),
                    row_count: 0,
                    execution_time_ms: 0,
                    success: false,
                    error: Some(error_msg),
                });
            },
        }
    }

    let total_execution_time_ms = total_start.elapsed().as_millis();

    Ok(Json(QueryExecuteResponse { results, total_execution_time_ms }))
}

// Simple SQL statement parser - splits by semicolon, ignoring those in single/double quotes
fn parse_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let chars = sql.chars().peekable();

    for ch in chars {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            },
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            },
            ';' if !in_single_quote && !in_double_quote => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                }
                current.clear();
            },
            _ => {
                current.push(ch);
            },
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_string());
    }

    statements
}

fn apply_query_limit(sql: &str, limit: i32) -> String {
    let trimmed = sql.trim();
    let sql_upper = trimmed.to_uppercase();

    if sql_upper.contains("LIMIT") {
        return trimmed.to_string();
    }

    if sql_upper.starts_with("SELECT") {
        if sql_upper.contains("GET_QUERY_PROFILE")
            || sql_upper.contains("SHOW_PROFILE")
            || sql_upper.contains("EXPLAIN")
        {
            return trimmed.to_string();
        }

        let sql_without_semicolon = trimmed.trim_end_matches(';');
        format!("{} LIMIT {}", sql_without_semicolon, limit)
    } else {
        trimmed.to_string()
    }
}

// ==================== SQL Blacklist APIs ====================

// List SQL blacklist
#[utoipa::path(
    get,
    path = "/api/clusters/sql-blacklist",
    responses(
        (status = 200, description = "List of SQL blacklist items", body = Vec<SqlBlacklistItem>),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "SQL Blacklist"
)]
pub async fn list_sql_blacklist(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
) -> ApiResult<Json<Vec<SqlBlacklistItem>>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    let items = adapter.list_sql_blacklist().await?;

    Ok(Json(items))
}

// Add SQL blacklist
#[utoipa::path(
    post,
    path = "/api/clusters/sql-blacklist",
    request_body = AddSqlBlacklistRequest,
    responses(
        (status = 200, description = "SQL blacklist added successfully"),
        (status = 400, description = "Invalid pattern"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "SQL Blacklist"
)]
pub async fn add_sql_blacklist(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Json(request): Json<AddSqlBlacklistRequest>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let pattern = request.pattern.trim();
    if pattern.is_empty() {
        return Err(ApiError::validation_error("Pattern cannot be empty"));
    }

    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter.add_sql_blacklist(pattern).await?;

    Ok((StatusCode::OK, Json(json!({ "message": "SQL blacklist added successfully" }))))
}

// Delete SQL blacklist
#[utoipa::path(
    delete,
    path = "/api/clusters/sql-blacklist/{id}",
    params(("id" = String, Path, description = "Blacklist ID")),
    responses(
        (status = 200, description = "SQL blacklist deleted successfully"),
        (status = 404, description = "No active cluster found"),
        (status = 500, description = "Internal server error")
    ),
    security(("bearer_auth" = [])),
    tag = "SQL Blacklist"
)]
pub async fn delete_sql_blacklist(
    State(state): State<Arc<AppState>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    if id.is_empty() {
        return Err(ApiError::validation_error("Invalid blacklist ID format"));
    }

    let adapter = crate::services::create_adapter(cluster, state.mysql_pool_manager.clone());
    adapter.delete_sql_blacklist(&id).await?;

    Ok((StatusCode::OK, Json(json!({ "message": "SQL blacklist deleted successfully" }))))
}

async fn handle_loads_query_for_doris(
    sql: &str,
    session: &mut crate::services::mysql_client::MySQLSession,
    default_db: Option<&str>,
) -> Result<(Vec<String>, Vec<Vec<String>>, u128), crate::utils::ApiError> {
    use regex::Regex;
    
    let db_name = if let Some(caps) = Regex::new(r#"(?i)WHERE\s+DB_NAME\s*=\s*['"]?([^'";\s]+)['"]?"#)
        .ok()
        .and_then(|re| re.captures(sql)) {
        caps.get(1).map(|m| m.as_str().to_string())
    } else {
        default_db.map(|s| s.to_string())
    };
    
    let db_name = db_name.ok_or_else(|| {
        crate::utils::ApiError::validation_error("Database name not found in query. Please specify DB_NAME in WHERE clause or provide database context.")
    })?;
    
    let show_load_sql = format!("SHOW LOAD FROM `{}`", db_name);
    let (columns, rows, _exec_time) = session.execute(&show_load_sql).await?;
    
    let select_fields = if let Some(caps) = Regex::new(r#"(?i)SELECT\s+(.+?)\s+FROM"#)
        .ok()
        .and_then(|re| re.captures(sql)) {
        caps.get(1).map(|m| m.as_str().trim().to_string())
    } else {
        Some("*".to_string())
    };
    
    let requested_fields: Vec<String> = if let Some(select_clause) = select_fields {
        if select_clause.trim() == "*" {
            vec![
                "JOB_ID".to_string(),
                "LABEL".to_string(),
                "STATE".to_string(),
                "PROGRESS".to_string(),
                "TYPE".to_string(),
                "PRIORITY".to_string(),
                "SCAN_ROWS".to_string(),
                "FILTERED_ROWS".to_string(),
                "SINK_ROWS".to_string(),
                "CREATE_TIME".to_string(),
                "LOAD_START_TIME".to_string(),
                "LOAD_FINISH_TIME".to_string(),
                "ERROR_MSG".to_string(),
            ]
        } else {
            select_clause.split(',')
                .map(|f| f.trim().to_uppercase())
                .collect()
        }
    } else {
        vec!["*".to_string()]
    };
    
    let job_id_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("JobId"));
    let label_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("Label"));
    let state_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("State"));
    let progress_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("Progress"));
    let type_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("Type"));
    let create_time_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("CreateTime"));
    let load_start_time_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("LoadStartTime"));
    let load_finish_time_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("LoadFinishTime"));
    let error_msg_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("ErrorMsg"));
    let job_details_idx = columns.iter().position(|c| c.eq_ignore_ascii_case("JobDetails"));
    
    let mut mapped_rows = Vec::new();
    for row in rows {
        let mut mapped_row = Vec::new();
        
        for field in &requested_fields {
            let value = match field.as_str() {
                "JOB_ID" => job_id_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "LABEL" => label_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "STATE" => state_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "PROGRESS" => progress_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "TYPE" => type_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "PRIORITY" => "NORMAL".to_string(),
                "SCAN_ROWS" => {
                    job_details_idx.and_then(|i| row.get(i))
                        .and_then(|json_str| {
                            serde_json::from_str::<serde_json::Value>(json_str).ok()
                                .and_then(|v| v.get("ScannedRows").and_then(|n| n.as_u64()))
                        })
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                },
                "FILTERED_ROWS" => "0".to_string(),
                "SINK_ROWS" => {
                    job_details_idx.and_then(|i| row.get(i))
                        .and_then(|json_str| {
                            serde_json::from_str::<serde_json::Value>(json_str).ok()
                                .and_then(|v| v.get("LoadRows").and_then(|n| n.as_u64()))
                        })
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                },
                "CREATE_TIME" => create_time_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "LOAD_START_TIME" => load_start_time_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "LOAD_FINISH_TIME" => load_finish_time_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                "ERROR_MSG" => error_msg_idx.and_then(|i| row.get(i)).cloned().unwrap_or_default(),
                _ => String::new(),
            };
            mapped_row.push(value);
        }
        mapped_rows.push(mapped_row);
    }
    
    if sql.to_uppercase().contains("ORDER BY CREATE_TIME DESC") {
    }
    
    let limit = if let Some(caps) = Regex::new(r#"(?i)LIMIT\s+(\d+)"#)
        .ok()
        .and_then(|re| re.captures(sql)) {
        caps.get(1).and_then(|m| m.as_str().parse::<usize>().ok())
    } else {
        None
    };
    
    let final_rows = if let Some(limit_val) = limit {
        mapped_rows.into_iter().take(limit_val).collect()
    } else {
        mapped_rows
    };
    
    Ok((requested_fields, final_rows, 0u128))
}

async fn handle_compactions_query_for_doris(
    _session: &mut crate::services::mysql_client::MySQLSession,
) -> Result<(Vec<String>, Vec<Vec<String>>, u128), crate::utils::ApiError> {
    let columns = vec![
        "Partition".to_string(),
        "TxnID".to_string(),
        "StartTime".to_string(),
        "CommitTime".to_string(),
        "FinishTime".to_string(),
        "Error".to_string(),
        "Profile".to_string(),
    ];
    
    tracing::info!("[Doris] SHOW PROC '/compactions' not supported. Compaction info is tablet-level via BE HTTP API.");
    
    Ok((columns, Vec::new(), 0u128))
}

fn adapt_sql_for_doris(sql: &str) -> String {
    use regex::Regex;
    
    let mut result = sql.to_string();
    
    let re_partitions_meta = Regex::new(r"(?i)information_schema\.partitions_meta").ok();
    if let Some(re) = re_partitions_meta {
        result = re.replace_all(&result, "information_schema.partitions").to_string();
    }
    
    let re_loads = Regex::new(r"(?i)information_schema\.loads").ok();
    if let Some(re) = re_loads {
        result = re.replace_all(&result, "information_schema.loads").to_string();
    }
    
    let field_mappings = vec![
        (r"(?i)\bDB_NAME\b", "TABLE_SCHEMA"),
        (r"(?i)\bROW_COUNT\b", "TABLE_ROWS"),
        (r"(?i)\bDATA_SIZE\b", "DATA_LENGTH"),
        (r"(?i)\bCOMPACT_VERSION\b", "COMMITTED_VERSION"),
    ];
    
    for (pattern, replacement) in field_mappings {
        if let Ok(re) = Regex::new(pattern) {
            result = re.replace_all(&result, replacement).to_string();
        }
    }
    
    result
}

fn normalize_table_names_for_doris(sql: &str) -> String {
    use regex::Regex;
    
    let re = Regex::new(
        r"(?i)\b(?:FROM|JOIN|INTO|UPDATE|TABLE)\s+((?:`?[a-zA-Z0-9_]+`?(?:\s*\.\s*`?[a-zA-Z0-9_]+`?)*))"
    ).ok();
    
    if let Some(re) = re {
        re.replace_all(sql, |caps: &regex::Captures| {
            let keyword_match = caps.get(0).unwrap();
            let table_ref = caps.get(1).unwrap().as_str();
            
            let keyword = &sql[keyword_match.start()..keyword_match.start() + (caps.get(1).unwrap().start() - keyword_match.start())];
            
            let parts: Vec<&str> = table_ref.split('.').collect();
            let normalized_parts: Vec<String> = parts.iter().map(|part| {
                let cleaned = part.trim_matches('`').trim();
                if cleaned.chars().any(|c| c.is_alphanumeric() || c == '_') {
                    format!("`{}`", cleaned.to_lowercase())
                } else {
                    part.to_string()
                }
            }).collect();
            
            let normalized = normalized_parts.join(".");
            format!("{}{}", keyword.trim_end(), normalized)
        }).to_string()
    } else {
        sql.to_string()
    }
}
