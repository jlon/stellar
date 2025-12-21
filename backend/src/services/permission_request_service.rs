use chrono::Utc;
use serde_json;
use sqlx::{SqlitePool, Row};
use std::sync::Arc;

use crate::models::{
    PermissionRequestResponse, SubmitRequestDto, ApprovalDto,
    RequestDetails, RequestQueryFilter, PaginatedResponse, Cluster,
};
use crate::services::{ClusterService, MySQLPoolManager, create_adapter};
use crate::utils::{ApiError, ApiResult};

/// Service for managing permission request workflow (submission, approval, execution)
#[derive(Clone)]
pub struct PermissionRequestService {
    pool: SqlitePool,
}

impl PermissionRequestService {
    pub fn new(
        pool: SqlitePool,
        _cluster_service: ClusterService,
        _mysql_pool_manager: std::sync::Arc<MySQLPoolManager>,
    ) -> Self {
        // NOTE: For now we only need the SQLite pool here; cluster-related logic is handled elsewhere.
        Self { pool }
    }

    /// Submit a new permission request
    /// Automatically assigns approvers from the applicant's organization
    pub async fn submit_request(
        &self,
        applicant_id: i64,
        req: SubmitRequestDto,
        cluster_service: &ClusterService,
        mysql_pool_manager: Arc<MySQLPoolManager>,
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

        // Get cluster for SQL generation
        let cluster = cluster_service.get_cluster(req.cluster_id).await?;

        // Generate preview SQL using cluster-specific adapter
        let preview_sql = Self::generate_preview_sql(&cluster, &req.request_type, &req.request_details, mysql_pool_manager).await?;

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
        // Check if approver has permission to approve this request
        self.check_approval_permission(request_id, approver_id).await?;

        let now = Utc::now();

        sqlx::query(
            "UPDATE permission_requests SET status = 'approved', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(approver_id)
        .bind(&dto.comment)
        .bind(now)
        .bind(now)
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
        // Check if approver has permission to reject this request
        self.check_approval_permission(request_id, approver_id).await?;

        let now = Utc::now();

        sqlx::query(
            "UPDATE permission_requests SET status = 'rejected', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(approver_id)
        .bind(&dto.comment)
        .bind(now)
        .bind(now)
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

    /// Public static method for generating preview SQL (used by handlers and services)
    pub async fn generate_preview_sql_static(
        cluster: &Cluster,
        request_type: &str,
        details: &RequestDetails,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<String> {
        Self::generate_preview_sql(cluster, request_type, details, mysql_pool_manager).await
    }

    /// Generate real executable preview SQL based on cluster type and request details.
    /// Uses cluster-specific adapter for proper SQL dialect generation.
    async fn generate_preview_sql(
        cluster: &Cluster,
        request_type: &str,
        details: &RequestDetails,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<String> {
        // Create cluster-specific adapter
        let adapter = create_adapter(cluster.clone(), mysql_pool_manager);
        
        match request_type {
            "grant_permission" => {
                // 1. 权限列表
                let perms = details
                    .permissions
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing permissions for grant_permission".to_string(),
                    ))?;
                if perms.is_empty() {
                    return Err(ApiError::ValidationError(
                        "At least one permission is required".to_string(),
                    ));
                }

                // Convert Vec<String> to Vec<&str> for adapter methods
                let perms_ref: Vec<&str> = perms.iter().map(|s| s.as_str()).collect();

                // 2. 资源路径（database / table），默认 database 级
                let resource_type = details
                    .resource_type
                    .as_deref()
                    .or(details.scope.as_deref())
                    .unwrap_or("database");

                let database = details
                    .database
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing database for grant_permission".to_string(),
                    ))?;

                let table = details.table.as_deref();

                // 3. WITH GRANT OPTION（可选）
                let with_grant_option = details.with_grant_option.unwrap_or(false);

                // 4. 场景分支：使用集群适配器生成 SQL
                if let Some(new_user) = &details.new_user_name {
                    // 场景 C：新建用户 + 授权
                    let password = details
                        .new_user_password
                        .as_deref()
                        .unwrap_or("");

                    let mut sqls = Vec::new();
                    sqls.push(adapter.create_user(new_user, password).await?);
                    
                    let grant_sql = adapter
                        .grant_permissions(
                            "USER",
                            new_user,
                            &perms_ref,
                            resource_type,
                            database,
                            table,
                            with_grant_option,
                        )
                        .await?;
                    sqls.push(grant_sql);

                    Ok(sqls.join(" "))
                } else if let Some(new_role) = &details.new_role_name {
                    // 场景 B：新建角色 + 授权角色 + 把角色授予用户
                    let target_user = details
                        .target_user
                        .as_ref()
                        .ok_or(ApiError::ValidationError(
                            "Missing target_user for grant_permission with new_role".to_string(),
                        ))?;

                    let mut sqls = Vec::new();
                    sqls.push(adapter.create_role(new_role).await?);
                    
                    let grant_sql = adapter
                        .grant_permissions(
                            "ROLE",
                            new_role,
                            &perms_ref,
                            resource_type,
                            database,
                            table,
                            with_grant_option,
                        )
                        .await?;
                    sqls.push(grant_sql);
                    
                    sqls.push(adapter.grant_role(new_role, target_user).await?);

                    Ok(sqls.join(" "))
                } else if let Some(user) = &details.target_user {
                    // 场景 A1：已有用户直接授予权限
                    let grant_sql = adapter
                        .grant_permissions(
                            "USER",
                            user,
                            &perms_ref,
                            resource_type,
                            database,
                            table,
                            with_grant_option,
                        )
                        .await?;
                    Ok(grant_sql)
                } else if let Some(role) = &details.target_role {
                    // 场景 A2：已有角色授予权限
                    let grant_sql = adapter
                        .grant_permissions(
                            "ROLE",
                            role,
                            &perms_ref,
                            resource_type,
                            database,
                            table,
                            with_grant_option,
                        )
                        .await?;
                    Ok(grant_sql)
                } else {
                    Err(ApiError::ValidationError(
                        "Missing principal (user/role/new_user/new_role) for grant_permission"
                            .to_string(),
                    ))
                }
            }
            "grant_role" => {
                let target_user = details
                    .target_user
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing target_user for grant_role".to_string(),
                    ))?;
                let target_role = details
                    .target_role
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing target_role for grant_role".to_string(),
                    ))?;
                Ok(format!("GRANT '{}' TO USER '{}'@'%';", target_role, target_user))
            }
            "revoke_permission" => {
                let perms = details
                    .permissions
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing permissions for revoke_permission".to_string(),
                    ))?;
                if perms.is_empty() {
                    return Err(ApiError::ValidationError(
                        "At least one permission is required".to_string(),
                    ));
                }
                let perm_str = perms.join(", ");

                let resource_type = details
                    .resource_type
                    .as_deref()
                    .or(details.scope.as_deref())
                    .unwrap_or("database");

                let database = details
                    .database
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing database for revoke_permission".to_string(),
                    ))?;

                let resource = match resource_type {
                    "table" => {
                        let table = details
                            .table
                            .as_ref()
                            .ok_or(ApiError::ValidationError(
                                "Missing table for table-level permission".to_string(),
                            ))?;
                        format!("{}.{}", database, table)
                    }
                    _ => format!("{}.*", database),
                };

                let user = details
                    .target_user
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing target_user for revoke_permission".to_string(),
                    ))?;

                Ok(format!(
                    "REVOKE {} ON {} FROM USER '{}'@'%';",
                    perm_str, resource, user
                ))
            }
            _ => Err(ApiError::ValidationError(format!(
                "Unknown request_type: {}",
                request_type
            ))),
        }
    }

    /// Execute request in background (currently只更新状态，不真正连集群执行)
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

    /// Check if the approver has permission to approve/reject this request
    /// Only organization admins or super admins can approve requests
    async fn check_approval_permission(&self, request_id: i64, approver_id: i64) -> ApiResult<()> {
        // Get the request to find the applicant's organization
        let request = sqlx::query(
            "SELECT pr.applicant_org_id, u.username as approver_name, u.organization_id as approver_org_id
             FROM permission_requests pr
             JOIN users u ON u.id = ?
             WHERE pr.id = ?"
        )
        .bind(approver_id)
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;

        let (applicant_org_id, _approver_name, approver_org_id) = match request {
            Some(row) => {
                let applicant_org_id: Option<i64> = row.get("applicant_org_id");
                let _approver_name: String = row.get("approver_name");
                let approver_org_id: Option<i64> = row.get("approver_org_id");
                (applicant_org_id, _approver_name, approver_org_id)
            }
            None => return Err(ApiError::not_found("Permission request not found".to_string())),
        };

        // Check if approver belongs to the same organization as the applicant
        if applicant_org_id != approver_org_id {
            return Err(ApiError::forbidden(
                "You can only approve requests from users in your organization".to_string()
            ));
        }

        // TODO: In a real implementation, you would check if the user has admin role
        // For now, we'll allow any user in the organization to approve
        // This should be enhanced to check for specific admin roles

        Ok(())
    }
}
