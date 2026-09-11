use crate::db::AppDb;
use crate::db::query as db_query;
use chrono::Utc;
use serde_json;
use sqlx::{Pool, Row};
use std::sync::Arc;
use stellar_macros::app_impl;

use crate::models::{
    ApprovalDto, Cluster, PaginatedResponse, PermissionRequestResponse, RequestDetails,
    RequestQueryFilter, SubmitRequestDto,
};
use crate::services::{ClusterService, MySQLPoolManager, create_adapter};
use crate::utils::{ApiError, ApiResult};

/// Service for managing permission request workflow (submission, approval, execution)
#[derive(Clone)]
pub struct PermissionRequestService<DB: AppDb> {
    pool: Pool<DB>,
}

#[app_impl]
impl<DB: AppDb> PermissionRequestService<DB> {
    pub fn new(pool: Pool<DB>) -> Self {
        // NOTE: cluster-related logic is handled elsewhere; only the metadata pool is needed here.
        Self { pool }
    }

    /// Submit a new permission request
    /// Automatically assigns approvers from the applicant's organization
    pub async fn submit_request(
        &self,
        applicant_id: i64,
        req: SubmitRequestDto,
        cluster_service: &ClusterService<DB>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<i64> {
        // Get applicant's organization from database
        let applicant = db_query::query("SELECT organization_id FROM users WHERE id = ?")
            .bind(applicant_id)
            .fetch_one(&self.pool)
            .await?;

        let org_id: Option<i64> = applicant.get("organization_id");
        let org_id = org_id.ok_or_else(|| {
            ApiError::InvalidInput("User must belong to an organization".to_string())
        })?;

        // Get cluster for SQL generation
        let cluster = cluster_service.get_cluster(req.cluster_id).await?;

        // Generate preview SQL using cluster-specific adapter
        let preview_sql = Self::generate_preview_sql(
            &cluster,
            &req.request_type,
            &req.request_details,
            mysql_pool_manager,
        )
        .await?;

        // Insert request record
        let now = Utc::now();
        let request_id = db_query::query(
            "INSERT INTO permission_requests (
                cluster_id, applicant_id, applicant_org_id, request_type,
                request_details, reason, valid_until, status, executed_sql, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
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
        .insert_id(&self.pool)
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

        // Build query with parameterized filters to prevent SQL injection
        let base_query = "SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name
            FROM permission_requests pr
            JOIN users u ON pr.applicant_id = u.id
            JOIN clusters c ON pr.cluster_id = c.id
            LEFT JOIN users approver ON pr.approver_id = approver.id
            WHERE pr.applicant_id = ?";

        // Build dynamic WHERE clause with parameter placeholders
        let mut conditions = Vec::new();
        if filter.status.is_some() {
            conditions.push("pr.status = ?".to_string());
        }

        if filter.request_type.is_some() {
            conditions.push("pr.request_type = ?".to_string());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" AND {}", conditions.join(" AND "))
        };

        // Count query
        let count_query = format!(
            "SELECT COUNT(*) FROM permission_requests pr WHERE pr.applicant_id = ?{}",
            where_clause
        );

        // Build count query with bindings
        let mut count_builder = db_query::query_scalar::<_, i64>(&count_query).bind(applicant_id);

        if let Some(ref status) = filter.status {
            count_builder = count_builder.bind(status);
        }
        if let Some(ref request_type) = filter.request_type {
            count_builder = count_builder.bind(request_type);
        }

        let total: i64 = count_builder.fetch_one(&self.pool).await?;

        // Data query with pagination
        let data_query =
            format!("{}{} ORDER BY pr.created_at DESC LIMIT ? OFFSET ?", base_query, where_clause,);

        let mut data_builder = db_query::query(&data_query).bind(applicant_id);

        if let Some(ref status) = filter.status {
            data_builder = data_builder.bind(status);
        }
        if let Some(ref request_type) = filter.request_type {
            data_builder = data_builder.bind(request_type);
        }

        data_builder = data_builder.bind(page_size).bind(offset);

        let rows = data_builder.fetch_all(&self.pool).await?;

        let data = rows
            .into_iter()
            .map(|row| self.row_to_response(row))
            .collect::<Result<Vec<_>, _>>()?;

        let total_pages = (total + page_size - 1) / page_size;

        Ok(PaginatedResponse { data, total, page, page_size, total_pages })
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

        // Build query with parameterized filters to prevent SQL injection
        let base_query = "SELECT pr.*, u.username as applicant_name, c.name as cluster_name, approver.username as approver_name
            FROM permission_requests pr
            JOIN users u ON pr.applicant_id = u.id
            JOIN clusters c ON pr.cluster_id = c.id
            LEFT JOIN users approver ON pr.approver_id = approver.id
            WHERE pr.status = 'pending'";

        // Build dynamic WHERE clause with parameter placeholders
        let mut conditions = Vec::new();
        let mut param_index = 1;

        // Org admin can only see pending requests from their organization
        if !is_super_admin {
            conditions.push(format!("pr.applicant_org_id = ${}", param_index));
            param_index += 1;
        }

        if filter.status.is_some() {
            conditions.push(format!("pr.status = ${}", param_index));
            param_index += 1;
        }

        if filter.request_type.is_some() {
            conditions.push(format!("pr.request_type = ${}", param_index));
            param_index += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" AND {}", conditions.join(" AND "))
        };

        let data_query = format!(
            "{}{} ORDER BY pr.created_at DESC LIMIT ${} OFFSET ${}",
            base_query,
            where_clause,
            param_index,
            param_index + 1
        );

        let mut data_builder = db_query::query(&data_query);

        if !is_super_admin {
            data_builder = data_builder.bind(approver_org_id);
        }
        if let Some(ref status) = filter.status {
            data_builder = data_builder.bind(status);
        }
        if let Some(ref request_type) = filter.request_type {
            data_builder = data_builder.bind(request_type);
        }

        data_builder = data_builder.bind(page_size).bind(offset);

        let rows = data_builder.fetch_all(&self.pool).await?;

        let data = rows
            .into_iter()
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
        cluster_service: &ClusterService<DB>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        // Check if approver has permission to approve this request
        self.check_approval_permission(request_id, approver_id)
            .await?;

        // Get cluster_id from request to check admin user configuration before approval
        let request = db_query::query("SELECT cluster_id FROM permission_requests WHERE id = ?")
            .bind(request_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let cluster_id: i64 = request.get("cluster_id");
        let cluster = cluster_service.get_cluster(cluster_id).await?;

        // Check if admin user is configured before approval
        if cluster.admin_user.is_none() || cluster.admin_password_encrypted.is_none() {
            let error_msg = format!(
                "集群 '{}' 未配置管理用户，无法执行权限授权操作。请组织管理员或超级管理员配置管理用户。",
                cluster.name
            );
            tracing::error!("{}", error_msg);
            return Err(ApiError::ValidationError(error_msg));
        }

        let now = Utc::now();

        // Update status to approved
        db_query::query(
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

        // Execute SQL synchronously (not in background)
        Self::execute_request_internal(&self.pool, request_id, cluster_service, mysql_pool_manager)
            .await?;

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
        self.check_approval_permission(request_id, approver_id)
            .await?;

        let now = Utc::now();

        db_query::query(
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
    pub async fn cancel_request(&self, request_id: i64, applicant_id: i64) -> ApiResult<()> {
        let request =
            db_query::query("SELECT applicant_id, status FROM permission_requests WHERE id = ?")
                .bind(request_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let req_applicant_id: i64 = request.get("applicant_id");
        let status: String = request.get("status");

        if req_applicant_id != applicant_id {
            return Err(ApiError::Unauthorized(
                "Only applicant can cancel the request".to_string(),
            ));
        }

        if status != "pending" {
            return Err(ApiError::ValidationError("Can only cancel pending requests".to_string()));
        }

        let now = Utc::now();
        db_query::query(
            "UPDATE permission_requests SET status = 'rejected', updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get request detail by ID
    pub async fn get_request_detail(
        &self,
        request_id: i64,
    ) -> ApiResult<PermissionRequestResponse> {
        let row = db_query::query(
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

                // For catalog-level permissions, database is not required
                // For database/table level, database is required (can be "*" for all)
                let database = if resource_type.to_lowercase() == "catalog" {
                    // For catalog level, use catalog name or "*" for all catalogs
                    details.catalog.as_deref().unwrap_or("*").to_string()
                } else {
                    details
                        .database
                        .as_ref()
                        .ok_or(ApiError::ValidationError(
                            "Missing database for grant_permission".to_string(),
                        ))?
                        .clone()
                };

                let table = details.table.as_deref();

                // 3. WITH GRANT OPTION（可选）
                let with_grant_option = details.with_grant_option.unwrap_or(false);

                // 4. 场景分支：使用集群适配器生成 SQL
                if let Some(new_user) = &details.new_user_name {
                    // 场景 C：新建用户 + 授权
                    let password = details.new_user_password.as_deref().unwrap_or("");

                    let mut sqls = Vec::new();
                    sqls.push(adapter.create_user(new_user, password).await?);

                    let grant_sql = adapter
                        .grant_permissions(
                            "USER",
                            new_user,
                            &perms_ref,
                            resource_type,
                            &database,
                            table,
                            with_grant_option,
                        )
                        .await?;
                    sqls.push(grant_sql);

                    Ok(sqls.join(" "))
                } else if let Some(new_role) = &details.new_role_name {
                    // 场景 B：新建角色 + 授权角色 + 把角色授予用户
                    let target_user =
                        details
                            .target_user
                            .as_ref()
                            .ok_or(ApiError::ValidationError(
                                "Missing target_user for grant_permission with new_role"
                                    .to_string(),
                            ))?;

                    let mut sqls = Vec::new();
                    sqls.push(adapter.create_role(new_role).await?);

                    let grant_sql = adapter
                        .grant_permissions(
                            "ROLE",
                            new_role,
                            &perms_ref,
                            resource_type,
                            &database,
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
                            &database,
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
                            &database,
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
            },
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
                // Use adapter for consistent SQL generation across different cluster types
                adapter.grant_role(target_role, target_user).await
            },
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

                let perms_ref: Vec<&str> = perms.iter().map(|s| s.as_str()).collect();

                let resource_type = details
                    .resource_type
                    .as_deref()
                    .or(details.scope.as_deref())
                    .unwrap_or("database");

                // For catalog-level permissions, database is not required
                let database = if resource_type.to_lowercase() == "catalog" {
                    details.catalog.as_deref().unwrap_or("*").to_string()
                } else {
                    details
                        .database
                        .as_ref()
                        .ok_or(ApiError::ValidationError(
                            "Missing database for revoke_permission".to_string(),
                        ))?
                        .clone()
                };

                let table = details.table.as_deref();

                let user = details
                    .target_user
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing target_user for revoke_permission".to_string(),
                    ))?;

                let revoke_sql = adapter
                    .revoke_permissions("USER", user, &perms_ref, resource_type, &database, table)
                    .await?;
                Ok(revoke_sql)
            },
            _ => Err(ApiError::ValidationError(format!("Unknown request_type: {}", request_type))),
        }
    }

    /// Execute request synchronously using admin user credentials
    async fn execute_request_internal(
        pool: &Pool<DB>,
        request_id: i64,
        cluster_service: &ClusterService<DB>,
        _mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        // Query for request details including cluster_id and executed_sql
        let request = db_query::query(
            "SELECT cluster_id, executed_sql FROM permission_requests WHERE id = ?",
        )
        .bind(request_id)
        .fetch_one(pool)
        .await
        .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let cluster_id: i64 = request.get("cluster_id");
        let executed_sql: Option<String> = request.get("executed_sql");

        if executed_sql.is_none() {
            return Err(ApiError::ValidationError("No SQL to execute".to_string()));
        }

        let sql = executed_sql.unwrap();

        // Update status to executing
        let now = Utc::now();
        db_query::query(
            "UPDATE permission_requests SET status = 'executing', updated_at = ? WHERE id = ?",
        )
        .bind(&now)
        .bind(request_id)
        .execute(pool)
        .await?;

        // Get cluster (admin user already checked in approve_request)
        let cluster = cluster_service.get_cluster(cluster_id).await?;
        let (exec_user, exec_pass) = cluster.get_execution_credentials();

        // Create temporary MySQL connection using admin user credentials
        tracing::info!("Executing permission SQL using admin user: {}", exec_user);

        let exec_result =
            Self::execute_sql_with_admin_user(&cluster, exec_user, exec_pass, &sql).await;

        let now = Utc::now();
        match exec_result {
            Ok(_) => {
                tracing::info!("Permission request {} executed successfully", request_id);
                db_query::query(
                    "UPDATE permission_requests SET status = 'completed', execution_result = ?, executed_at = ?, updated_at = ? WHERE id = ?"
                )
                .bind("执行成功")
                .bind(&now)
                .bind(&now)
                .bind(request_id)
                .execute(pool)
                .await?;
                Ok(())
            },
            Err(e) => {
                let error_msg = format!("SQL执行失败: {}", e);
                tracing::error!(
                    "Permission request {} execution failed: {}",
                    request_id,
                    error_msg
                );
                db_query::query(
                    "UPDATE permission_requests SET status = 'failed', execution_result = ?, executed_at = ?, updated_at = ? WHERE id = ?"
                )
                .bind(&error_msg)
                .bind(&now)
                .bind(&now)
                .bind(request_id)
                .execute(pool)
                .await?;
                Err(e)
            },
        }
    }

    /// Execute SQL using admin user credentials with temporary connection
    async fn execute_sql_with_admin_user(
        cluster: &Cluster,
        admin_user: &str,
        admin_password: Option<&str>,
        sql: &str,
    ) -> ApiResult<()> {
        use mysql_async::{Conn, OptsBuilder, SslOpts, prelude::Queryable};

        // Create temporary connection using admin user credentials
        let opts = OptsBuilder::default()
            .ip_or_hostname(&cluster.fe_host)
            .tcp_port(cluster.fe_query_port as u16)
            .user(Some(admin_user))
            .pass(admin_password)
            .db_name(None::<String>)
            .prefer_socket(false)
            .ssl_opts(None::<SslOpts>)
            .tcp_keepalive(Some(30_000_u32))
            .tcp_nodelay(true);

        let mut conn = Conn::new(opts).await.map_err(|e| {
            tracing::error!("Failed to create admin user connection: {}", e);
            ApiError::cluster_connection_failed(format!("无法使用管理用户连接集群: {}", e))
        })?;

        // Execute SQL
        conn.query_drop(sql).await.map_err(|e| {
            tracing::error!("Failed to execute SQL with admin user: {}", e);
            ApiError::cluster_connection_failed(format!("SQL执行失败: {}", e))
        })?;

        // Connection will be dropped automatically when it goes out of scope
        tracing::info!("SQL executed successfully using admin user: {}", admin_user);
        Ok(())
    }

    fn row_to_response(&self, row: DB::Row) -> ApiResult<PermissionRequestResponse> {
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
        // Get the request and approver information including roles
        let approval_query = format!(
            r#"
            SELECT 
                pr.applicant_org_id,
                pr.applicant_id,
                u.username as approver_name,
                u.organization_id as approver_org_id,
                {} as role_codes
            FROM permission_requests pr
            JOIN users u ON u.id = ?
            LEFT JOIN user_roles ur ON ur.user_id = u.id
            LEFT JOIN roles r ON r.id = ur.role_id
            WHERE pr.id = ?
            GROUP BY pr.id, u.id
            "#,
            DB::string_aggregate("r.code", "','"),
        );
        let result = db_query::query(&approval_query)
            .bind(approver_id)
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?;

        let row = match result {
            Some(row) => row,
            None => return Err(ApiError::not_found("Permission request not found".to_string())),
        };

        let applicant_org_id: Option<i64> = row.get("applicant_org_id");
        let applicant_id: i64 = row.get("applicant_id");
        let approver_org_id: Option<i64> = row.get("approver_org_id");
        let role_codes: Option<String> = row.get("role_codes");

        // Parse role codes
        let roles: Vec<&str> = role_codes
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .collect();

        // Check if approver is super_admin (can approve any request)
        let is_super_admin = roles.iter().any(|r| *r == "super_admin");

        // Check if approver is org_admin
        let is_org_admin = roles.iter().any(|r| *r == "org_admin");

        // Super admin can approve any request
        if is_super_admin {
            return Ok(());
        }

        // Org admin can only approve requests from their organization
        if is_org_admin {
            if applicant_org_id == approver_org_id {
                // Cannot approve own request
                if applicant_id == approver_id {
                    return Err(ApiError::forbidden(
                        "You cannot approve your own request".to_string(),
                    ));
                }
                return Ok(());
            } else {
                return Err(ApiError::forbidden(
                    "You can only approve requests from users in your organization".to_string(),
                ));
            }
        }

        // Regular users cannot approve requests
        Err(ApiError::forbidden(
            "Only organization admins or super admins can approve permission requests".to_string(),
        ))
    }
}
