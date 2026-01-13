use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QueryExecutionHistory {
    pub id: i64,
    pub user_id: i64,
    pub cluster_id: i64,
    pub catalog: Option<String>,
    pub database_name: Option<String>,
    pub sql_statement: String,
    pub execution_time_ms: Option<i64>,
    pub row_count: Option<i64>,
    pub success: bool,
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QueryExecutionHistoryResponse {
    pub data: Vec<QueryExecutionHistory>,
    pub total: i64,
}
