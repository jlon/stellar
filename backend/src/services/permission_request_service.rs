use chrono::Utc;
use serde_json;
use sqlx::{SqlitePool, Row};

use crate::models::{
    PermissionRequest, PermissionRequestResponse, SubmitRequestDto, ApprovalDto,
    RequestDetails, RequestQueryFilter, PaginatedResponse,
};
use crate::utils::{ApiError, ApiResult};

/// Service for managing permission request workflow (submission, approval, execution)
#[derive(Clone)]
pub struct PermissionRequestService {
    pool: SqlitePool,
}

impl PermissionRequestService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Submit a new permission request
    /// Automatically assigns approvers from the applicant's organization
    pub async fn submit_request(
        &self,
        applicant_id: i64,
        req: SubmitRequestDto,
    ) -> ApiResult<i64> {
        // Get applicant's organization from database
        let applicant = sqlx::query(
            "SELECT organization_id FROM users WHERE id = ?"
        )
        .bind(applicant_id)
        .fetch_one(&self.pool)
        .await?;

        let org_id: Option<i64> = applicant.get("organization_id");
        let org_id = org_id
            .ok_or_else(|| ApiError::InvalidInput("User must belong to an organization".to_string()))?;

        // Generate preview SQL
        let preview_sql = Self::generate_preview_sql(&req.request_type, &req.request_details)?;

        // Insert request record
        let now = Utc::now();
        let request_id: i64 = sqlx::query_scalar(
            "INSERT INTO permission_requests (
                cluster_id, applicant_id, applicant_org_id, request_type,
                request_details, reason, valid_until, status, executed_sql, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)
            RETURNING id"
        )
        .bind(req.cluster_id)
        .bind(applicant_id)
        .bind(org_id)
        .bind(&req.request_type)
        .bind(serde_json::to_string(&req.request_details)?)
        .bind(&req.reason)
        .bind(&req.valid_until)
        .bind(&preview_sql)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(request_id)
    }

    /// List my requests (as applicant)
    pub async fn list_my_requests(
        &self,
        applicant_id: i64,
        filter: RequestQueryFilter,
    ) -> ApiResult<PaginatedResponse<PermissionRequestResponse>> {
        let page = filter.page.unwrap_or(1);
        let page_size = filter.page_size.unwrap_or(10);
        let offset = (page - 1) * page_size;

        let mut query = String::from(
            "SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name
            FROM permission_requests pr
            JOIN users u ON pr.applicant_id = u.id
            JOIN clusters c ON pr.cluster_id = c.id
            LEFT JOIN users approver ON pr.approver_id = approver.id
            WHERE pr.applicant_id = ?"
        );

        if let Some(status) = &filter.status {
            query.push_str(&format!(" AND pr.status = '{}'", status.replace("'", "\\'")));
        }

        if let Some(request_type) = &filter.request_type {
            query.push_str(&format!(" AND pr.request_type = '{}'", request_type.replace("'", "\\'")));
        }

        let total: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM ({}) as t",
            query.replace("SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name",
                         "SELECT 1")
        ))
        .bind(applicant_id)
        .fetch_one(&self.pool)
        .await?;

        query.push_str(" ORDER BY pr.created_at DESC LIMIT ? OFFSET ?");

        let rows = sqlx::query(&query)
            .bind(applicant_id)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let data = rows.into_iter()
            .map(|row| self.row_to_response(row))
            .collect::<Result<Vec<_>, _>>()?;

        let total_pages = (total + page_size - 1) / page_size;

        Ok(PaginatedResponse {
            data,
            total,
            page,
            page_size,
            total_pages,
        })
    }

    /// List pending requests for approval (as approver)
    pub async fn list_pending_approvals(
        &self,
        approver_org_id: i64,
        is_super_admin: bool,
        filter: RequestQueryFilter,
    ) -> ApiResult<Vec<PermissionRequestResponse>> {
        let page = filter.page.unwrap_or(1);
        let page_size = filter.page_size.unwrap_or(10);
        let offset = (page - 1) * page_size;

        let mut query = String::from(
            "SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name
            FROM permission_requests pr
            JOIN users u ON pr.applicant_id = u.id
            JOIN clusters c ON pr.cluster_id = c.id
            LEFT JOIN users approver ON pr.approver_id = approver.id
            WHERE pr.status = 'pending'"
        );

        // Org admin can only see pending requests from their organization
        if !is_super_admin {
            query.push_str(&format!(" AND pr.applicant_org_id = {}", approver_org_id));
        }

        if let Some(status) = &filter.status {
            query.push_str(&format!(" AND pr.status = '{}'", status.replace("'", "\\'")));
        }

        if let Some(request_type) = &filter.request_type {
            query.push_str(&format!(" AND pr.request_type = '{}'", request_type.replace("'", "\\'")));
        }

        query.push_str(" ORDER BY pr.created_at DESC LIMIT ? OFFSET ?");

        let rows = sqlx::query(&query)
            .bind(page_size)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let data = rows.into_iter()
            .map(|row| self.row_to_response(row))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(data)
    }

    /// Approve a request
    pub async fn approve_request(
        &self,
        request_id: i64,
        approver_id: i64,
        dto: ApprovalDto,
    ) -> ApiResult<()> {
        let now = Utc::now();

        sqlx::query(
            "UPDATE permission_requests SET status = 'approved', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(approver_id)
        .bind(&dto.comment)
        .bind(&now)
        .bind(&now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;

        // Async execute SQL in background
        let pool = self.pool.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::execute_request_internal(&pool, request_id).await {
                tracing::error!("Failed to execute permission request {}: {}", request_id, e);
            }
        });

        Ok(())
    }

    /// Reject a request
    pub async fn reject_request(
        &self,
        request_id: i64,
        approver_id: i64,
        dto: ApprovalDto,
    ) -> ApiResult<()> {
        let now = Utc::now();

        sqlx::query(
            "UPDATE permission_requests SET status = 'rejected', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(approver_id)
        .bind(&dto.comment)
        .bind(&now)
        .bind(&now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Cancel a pending request (only by applicant)
    pub async fn cancel_request(
        &self,
        request_id: i64,
        applicant_id: i64,
    ) -> ApiResult<()> {
        let request = sqlx::query(
            "SELECT applicant_id, status FROM permission_requests WHERE id = ?"
        )
        .bind(request_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let req_applicant_id: i64 = request.get("applicant_id");
        let status: String = request.get("status");

        if req_applicant_id != applicant_id {
            return Err(ApiError::Unauthorized("Only applicant can cancel the request".to_string()));
        }

        if status != "pending" {
            return Err(ApiError::ValidationError("Can only cancel pending requests".to_string()));
        }

        let now = Utc::now();
        sqlx::query(
            "UPDATE permission_requests SET status = 'rejected', updated_at = ? WHERE id = ?"
        )
        .bind(&now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get request detail by ID
    pub async fn get_request_detail(&self, request_id: i64) -> ApiResult<PermissionRequestResponse> {
        let row = sqlx::query(
            "SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name
            FROM permission_requests pr
            JOIN users u ON pr.applicant_id = u.id
            JOIN clusters c ON pr.cluster_id = c.id
            LEFT JOIN users approver ON pr.approver_id = approver.id
            WHERE pr.id = ?"
        )
        .bind(request_id)
        .fetch_one(&self.pool)
        .await?;

        self.row_to_response(row)
    }

    // Private helper methods

    // Public static method for generating preview SQL (used by handlers and services)
    pub fn generate_preview_sql_static(
        request_type: &str,
        details: &RequestDetails,
    ) -> ApiResult<String> {
        Self::generate_preview_sql(request_type, details)
    }

    fn generate_preview_sql(
        request_type: &str,
        details: &RequestDetails,
    ) -> ApiResult<String> {
        let sql = match request_type {
            "create_account" => {
                let account = details.target_account.as_ref()
                    .ok_or(ApiError::ValidationError("Missing target_account".to_string()))?;
                format!("CREATE USER '{}' IDENTIFIED BY 'password';", account)
            },
            "grant_role" => {
                let account = details.target_account.as_ref()
                    .ok_or(ApiError::ValidationError("Missing target_account".to_string()))?;
                let role = details.target_role.as_ref()
                    .ok_or(ApiError::ValidationError("Missing target_role".to_string()))?;
                format!("GRANT {} TO '{}';", role, account)
            },
            "grant_permission" => {
                let account = details.target_account.as_ref()
                    .ok_or(ApiError::ValidationError("Missing target_account".to_string()))?;
                let perms = details.permissions.as_ref()
                    .ok_or(ApiError::ValidationError("Missing permissions".to_string()))?;
                let perm_str = perms.join(", ");

                let object = match details.scope.as_deref() {
                    Some("table") => {
                        let db = details.database.as_ref()
                            .ok_or(ApiError::ValidationError("Missing database".to_string()))?;
                        let tbl = details.table.as_ref()
                            .ok_or(ApiError::ValidationError("Missing table".to_string()))?;
                        format!("{}.{}", db, tbl)
                    },
                    Some("database") => {
                        let db = details.database.as_ref()
                            .ok_or(ApiError::ValidationError("Missing database".to_string()))?;
                        format!("{}.*", db)
                    },
                    _ => "*.*".to_string(),
                };

                format!("GRANT {} ON {} TO '{}';", perm_str, object, account)
            },
            _ => return Err(ApiError::ValidationError(format!("Unknown request_type: {}", request_type))),
        };

        Ok(sql)
    }

    async fn execute_request_internal(pool: &SqlitePool, request_id: i64) -> ApiResult<()> {
        // Query for executed_sql field
        let request = sqlx::query(
            "SELECT executed_sql FROM permission_requests WHERE id = ?"
        )
        .bind(request_id)
        .fetch_one(pool)
        .await
        .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let executed_sql: Option<String> = request.get("executed_sql");

        if executed_sql.is_none() {
            return Err(ApiError::ValidationError("No SQL to execute".to_string()));
        }

        // For now, just mark as completed - actual execution would need cluster connection
        let now = Utc::now();
        sqlx::query(
            "UPDATE permission_requests SET status = 'completed', executed_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&now)
        .bind(&now)
        .bind(request_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    fn row_to_response(&self, row: sqlx::sqlite::SqliteRow) -> ApiResult<PermissionRequestResponse> {
        use sqlx::Row;

        let request_details_str: String = row.get("request_details");
        let request_details: RequestDetails = serde_json::from_str(&request_details_str)?;

        Ok(PermissionRequestResponse {
            id: row.get("id"),
            cluster_id: row.get("cluster_id"),
            cluster_name: row.get("cluster_name"),
            applicant_id: row.get("applicant_id"),
            applicant_name: row.get("applicant_name"),
            applicant_org_id: row.get("applicant_org_id"),
            request_type: row.get("request_type"),
            request_details,
            reason: row.get("reason"),
            valid_until: row.get("valid_until"),
            status: row.get("status"),
            approver_id: row.get("approver_id"),
            approver_name: row.get("approver_name"),
            approval_comment: row.get("approval_comment"),
            approved_at: row.get("approved_at"),
            executed_sql: row.get("executed_sql"),
            execution_result: row.get("execution_result"),
            executed_at: row.get("executed_at"),
            preview_sql: row.get::<Option<String>, _>("executed_sql"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}
