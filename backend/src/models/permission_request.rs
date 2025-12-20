use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

/// Database Permission Request Entity (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, ToSchema)]
pub struct PermissionRequest {
    pub id: i64,
    pub cluster_id: i64,
    pub applicant_id: i64,
    pub applicant_org_id: i64,
    pub request_type: String,  // 'create_account' | 'grant_role' | 'grant_permission'
    pub request_details: String,  // JSON string
    pub reason: String,
    pub valid_until: Option<DateTime<Utc>>,
    pub status: String,  // 'pending' | 'approved' | 'rejected' | 'executing' | 'completed' | 'failed'
    pub approver_id: Option<i64>,
    pub approval_comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub executed_sql: Option<String>,
    pub execution_result: Option<String>,
    pub executed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request Details JSON structure
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RequestDetails {
    pub action: String,  // 'create_account' | 'grant_role' | 'grant_permission'
    pub target_account: Option<String>,  // e.g., "user@'%'"
    pub target_role: Option<String>,
    pub scope: Option<String>,  // 'global' | 'database' | 'table'
    pub database: Option<String>,
    pub table: Option<String>,
    pub permissions: Option<Vec<String>>,  // ['SELECT', 'INSERT']
    pub with_grant_option: Option<bool>,
}

/// Request submission DTO
#[derive(Debug, Deserialize, ToSchema)]
pub struct SubmitRequestDto {
    pub cluster_id: i64,
    pub request_type: String,
    pub request_details: RequestDetails,
    pub reason: String,
    pub valid_until: Option<DateTime<Utc>>,
}

/// Response DTO for permission request
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PermissionRequestResponse {
    pub id: i64,
    pub cluster_id: i64,
    pub cluster_name: String,
    pub applicant_id: i64,
    pub applicant_name: String,
    pub applicant_org_id: i64,
    pub request_type: String,
    pub request_details: RequestDetails,
    pub reason: String,
    pub valid_until: Option<DateTime<Utc>>,
    pub status: String,
    pub approver_id: Option<i64>,
    pub approver_name: Option<String>,
    pub approval_comment: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub executed_sql: Option<String>,
    pub execution_result: Option<String>,
    pub executed_at: Option<DateTime<Utc>>,
    pub preview_sql: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Approval DTO
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApprovalDto {
    pub comment: Option<String>,
}

/// Query Filter
#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestQueryFilter {
    pub status: Option<String>,
    pub request_type: Option<String>,
    pub search: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

/// Database account DTO
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DbAccountDto {
    pub account_name: String,
    pub host: String,
    pub roles: Vec<String>,
}

/// Database role DTO
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DbRoleDto {
    pub role_name: String,
    pub role_type: String,  // 'built-in' | 'custom'
    pub permissions_count: Option<i64>,
}

/// Pagination result
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}
