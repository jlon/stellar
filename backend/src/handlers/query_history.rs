use crate::AppState;
use axum::{Json, extract::State};
use serde::Deserialize;
use std::sync::Arc;
use stellar_macros::app_db;

use crate::models::cluster::ClusterType;
use crate::models::starrocks::{QueryHistoryItem, QueryHistoryResponse};
use crate::services::cluster_timeout;
use crate::services::mysql_client::MySQLClient;
use crate::utils::error::ApiResult;

struct AuditHistoryColumns {
    audit_table: String,
    time_field: &'static str,
    query_id_field: &'static str,
    db_field: &'static str,
    is_query_field: &'static str,
    query_type_field: &'static str,
    query_time_field: &'static str,
    warehouse_field: &'static str,
}

fn audit_history_columns(
    cluster_type: ClusterType,
    starrocks_table: String,
) -> AuditHistoryColumns {
    match cluster_type {
        ClusterType::StarRocks => AuditHistoryColumns {
            audit_table: starrocks_table,
            time_field: "timestamp",
            query_id_field: "queryId",
            db_field: "db",
            is_query_field: "isQuery",
            query_type_field: "queryType",
            query_time_field: "queryTime",
            warehouse_field: "resourceGroup",
        },
        ClusterType::Doris => AuditHistoryColumns {
            audit_table: "__internal_schema.audit_log".to_string(),
            time_field: "time",
            query_id_field: "query_id",
            db_field: "db",
            is_query_field: "is_query",
            query_type_field: "stmt_type",
            query_time_field: "query_time",
            warehouse_field: "workload_group",
        },
    }
}

fn audit_history_select_sql(
    columns: &AuditHistoryColumns,
    where_clause: &str,
    limit: i64,
    offset: i64,
) -> String {
    format!(
        r#"
        SELECT
            `{query_id}` as queryId,
            `user`,
            COALESCE(`{db}`, '') AS db,
            `stmt`,
            COALESCE(`{query_type}`, '') AS queryType,
            `{time}` AS start_time,
            `{query_time}` AS total_ms,
            `state`,
            COALESCE(`{warehouse}`, '') AS warehouse
        FROM {audit_table}
        WHERE {where_clause}
        ORDER BY `{time}` DESC
        LIMIT {limit} OFFSET {offset}
    "#,
        query_id = columns.query_id_field,
        db = columns.db_field,
        query_type = columns.query_type_field,
        time = columns.time_field,
        query_time = columns.query_time_field,
        warehouse = columns.warehouse_field,
        audit_table = columns.audit_table,
        where_clause = where_clause,
        limit = limit,
        offset = offset,
    )
}

#[derive(Deserialize)]
pub struct HistoryQueryParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// offset for pagination
    #[serde(default = "default_offset")]
    pub offset: i64,
    /// search keyword for query_id, sql_statement, or user
    pub keyword: Option<String>,
    /// start time filter
    pub start_time: Option<String>,
    /// end time filter
    pub end_time: Option<String>,
}

fn default_limit() -> i64 {
    10
}
fn default_offset() -> i64 {
    0
}

#[utoipa::path(
    get,
    path = "/api/clusters/queries/history",
    responses((status = 200, description = "Finished query list with pagination", body = QueryHistoryResponse)),
    security(("bearer_auth" = [])),
    tag = "Queries"
)]
#[app_db]
pub async fn list_query_history(
    State(state): State<Arc<AppState<DB>>>,
    axum::extract::Extension(org_ctx): axum::extract::Extension<crate::middleware::OrgContext>,
    axum::extract::Query(params): axum::extract::Query<HistoryQueryParams>,
) -> ApiResult<Json<QueryHistoryResponse>> {
    let cluster = if org_ctx.is_super_admin {
        state.cluster_service.get_active_cluster().await?
    } else {
        state
            .cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await?
    };

    let pool = state.mysql_pool_manager.get_pool(&cluster).await?;
    let mysql = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));

    let limit = params.limit;
    let offset = params.offset;
    let keyword = params.keyword.as_deref().unwrap_or("");
    let start_time = params.start_time.as_deref();
    let end_time = params.end_time.as_deref();

    let columns = audit_history_columns(cluster.cluster_type, state.audit_config.full_table_name());

    let mut where_conditions = vec![
        format!("{} = 1", columns.is_query_field),
        format!("`{}` >= DATE_SUB(NOW(), INTERVAL 7 DAY)", columns.time_field),
    ];

    if !keyword.is_empty() {
        let escaped = keyword.replace('\'', "''");
        where_conditions.push(format!(
            "(`{}` LIKE '%{}%' OR `stmt` LIKE '%{}%' OR `user` LIKE '%{}%')",
            columns.query_id_field, escaped, escaped, escaped
        ));
    }

    if let Some(start) = start_time {
        where_conditions.push(format!("`{}` >= '{}'", columns.time_field, start));
    }
    if let Some(end) = end_time {
        where_conditions.push(format!("`{}` <= '{}'", columns.time_field, end));
    }

    let where_clause = where_conditions.join(" AND ");

    let count_sql = format!(
        r#"
        SELECT COUNT(*) as total
        FROM {}
        WHERE {}
    "#,
        columns.audit_table, where_clause
    );

    tracing::info!("Fetching total count for cluster {}", cluster.id);
    let (_, count_rows) = mysql.query_raw(&count_sql).await.map_err(|e| {
        tracing::error!("Failed to query count: {:?}", e);
        e
    })?;

    let total: i64 = if let Some(row) = count_rows.first() {
        if let Some(count_str) = row.first() {
            count_str.parse::<i64>().unwrap_or_else(|_| {
                tracing::warn!("Could not parse count result, defaulting to 0");
                0i64
            })
        } else {
            0i64
        }
    } else {
        0i64
    };

    tracing::info!("Total history records: {}", total);

    let sql = audit_history_select_sql(&columns, &where_clause, limit, offset);

    tracing::info!(
        "Fetching query history for cluster {} (limit: {}, offset: {})",
        cluster.id,
        limit,
        offset
    );
    let (columns, rows) = mysql.query_raw(&sql).await.map_err(|e| {
        tracing::error!("Failed to query audit table: {:?}", e);
        e
    })?;
    tracing::info!("Fetched {} history records", rows.len());

    let mut col_idx = std::collections::HashMap::new();
    for (i, col) in columns.iter().enumerate() {
        col_idx.insert(col.clone(), i);
    }

    let mut items: Vec<QueryHistoryItem> = Vec::with_capacity(rows.len());
    for row in &rows {
        let query_id = col_idx
            .get("queryId")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let user = col_idx
            .get("user")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let db = col_idx
            .get("db")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let stmt = col_idx
            .get("stmt")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let qtype = col_idx
            .get("queryType")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_else(|| "Query".to_string());
        let start_time = col_idx
            .get("start_time")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let total_ms_raw = col_idx
            .get("total_ms")
            .and_then(|&i| row.get(i))
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let state = col_idx
            .get("state")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();
        let warehouse = col_idx
            .get("warehouse")
            .and_then(|&i| row.get(i))
            .cloned()
            .unwrap_or_default();

        items.push(QueryHistoryItem {
            query_id,
            user,
            default_db: db,
            sql_statement: stmt,
            query_type: qtype,
            start_time,
            end_time: String::new(), // Can be calculated on frontend if needed
            total_ms: total_ms_raw,
            query_state: state,
            warehouse,
        });
    }

    let page = (offset / limit) + 1;

    Ok(Json(QueryHistoryResponse { data: items, total, page, page_size: limit }))
}

#[cfg(test)]
mod tests {
    use super::{audit_history_columns, audit_history_select_sql};
    use crate::models::cluster::ClusterType;

    #[test]
    fn starrocks_history_sql_uses_native_columns() {
        let columns = audit_history_columns(
            ClusterType::StarRocks,
            "starrocks_audit_db__.starrocks_audit_tbl__".to_string(),
        );
        let sql = audit_history_select_sql(&columns, "isQuery = 1", 10, 0);
        assert!(sql.contains("`queryType`"));
        assert!(sql.contains("`queryTime`"));
        assert!(sql.contains("`resourceGroup`"));
        assert!(!sql.contains("`stmt_type`"));
        assert!(!sql.contains("`query_time`"));
        assert!(!sql.contains("`workload_group`"));
    }

    #[test]
    fn doris_history_sql_uses_native_columns() {
        let columns = audit_history_columns(ClusterType::Doris, String::new());
        let sql = audit_history_select_sql(&columns, "is_query = 1", 10, 0);
        assert!(sql.contains("COALESCE(`stmt_type`, '') AS queryType"));
        assert!(sql.contains("`query_time` AS total_ms"));
        assert!(sql.contains("COALESCE(`workload_group`, '') AS warehouse"));
        assert_eq!(columns.audit_table, "__internal_schema.audit_log");
    }
}
