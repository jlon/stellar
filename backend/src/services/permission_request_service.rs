use crate::db::AppDb;
use crate::db::dialect::RowsAffected;
use crate::db::query as db_query;
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use sha2::{Digest, Sha256};
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
    credential_key: Option<[u8; 32]>,
}

#[app_impl]
impl<DB: AppDb> PermissionRequestService<DB> {
    pub fn new(pool: Pool<DB>, encryption_key: &str) -> Self {
        let credential_key = (!encryption_key.is_empty()).then(|| {
            let mut hash = Sha256::new();
            hash.update(b"stellar.permission-request.credentials.v1\0");
            hash.update(encryption_key.as_bytes());
            hash.finalize().into()
        });
        Self { pool, credential_key }
    }

    /// Submit a new permission request
    /// Automatically assigns approvers from the applicant's organization
    pub async fn submit_request(
        &self,
        applicant_id: i64,
        mut req: SubmitRequestDto,
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

        let cluster = cluster_service
            .get_active_cluster_by_org(Some(org_id))
            .await?;
        if req.cluster_id != cluster.id {
            return Err(ApiError::validation_error(
                "Selected cluster is no longer active. Refresh the page and try again.",
            ));
        }
        Self::validate_request(&req, &cluster, mysql_pool_manager.clone()).await?;

        let encrypted_password = req
            .request_details
            .new_user_password
            .take()
            .map(|password| self.encrypt_credential(&password))
            .transpose()?;

        // The stored preview must never contain the initial password.
        let mut preview_details = req.request_details.clone();
        if encrypted_password.is_some() {
            preview_details.new_user_password = Some("********".to_string());
        }
        let preview_sql = Self::generate_preview_sql(
            &cluster,
            &req.request_type,
            &preview_details,
            mysql_pool_manager,
        )
        .await?;

        // Insert request record
        let now = Utc::now();
        let request_id = db_query::query(
            "INSERT INTO permission_requests (
                cluster_id, applicant_id, applicant_org_id, request_type,
                request_details, new_user_password_encrypted, reason, valid_until,
                status, executed_sql, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
        )
        .bind(cluster.id)
        .bind(applicant_id)
        .bind(org_id)
        .bind(&req.request_type)
        .bind(serde_json::to_string(&req.request_details)?)
        .bind(encrypted_password)
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
        let page = filter.page.unwrap_or(1).max(1);
        let page_size = filter.page_size.unwrap_or(10).clamp(1, 100);
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
        let page = filter.page.unwrap_or(1).max(1);
        let page_size = filter.page_size.unwrap_or(10).clamp(1, 100);
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
        // Org admin can only see pending requests from their organization
        if !is_super_admin {
            conditions.push("pr.applicant_org_id = ?".to_string());
        }

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

        let data_query =
            format!("{}{} ORDER BY pr.created_at DESC LIMIT ? OFFSET ?", base_query, where_clause);

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

        // Re-read the immutable request context before executing an old work order.
        let request = db_query::query(
            "SELECT cluster_id, applicant_org_id, request_type, request_details, new_user_password_encrypted, valid_until, status
             FROM permission_requests WHERE id = ?",
        )
            .bind(request_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| ApiError::ResourceNotFound("Request not found".to_string()))?;

        let cluster_id: i64 = request.get("cluster_id");
        let applicant_org_id: i64 = request.get("applicant_org_id");
        let status: String = request.get("status");
        if status != "pending" {
            return Err(ApiError::validation_error("Only pending requests can be approved."));
        }

        let cluster = cluster_service
            .get_active_cluster_by_org(Some(applicant_org_id))
            .await?;
        if cluster.id != cluster_id {
            return Err(ApiError::validation_error(
                "The request cluster is no longer the active cluster for its organization.",
            ));
        }

        // Check if admin user is configured before approval
        if cluster.admin_user.is_none() || cluster.admin_password_encrypted.is_none() {
            let error_msg = format!(
                "集群 '{}' 未配置管理用户，无法执行权限授权操作。请组织管理员或超级管理员配置管理用户。",
                cluster.name
            );
            tracing::error!("{}", error_msg);
            return Err(ApiError::ValidationError(error_msg));
        }

        let mut details: RequestDetails =
            serde_json::from_str(&request.get::<String, _>("request_details"))?;
        let mut encrypted_password: Option<String> = request.get("new_user_password_encrypted");
        if encrypted_password.is_none() && details.new_user_password.is_some() {
            encrypted_password = Some(
                self.encrypt_credential(
                    details
                        .new_user_password
                        .as_deref()
                        .expect("checked is_some"),
                )?,
            );
        }
        if let Some(ref encrypted_password) = encrypted_password {
            details.new_user_password = Some(self.decrypt_credential(encrypted_password)?);
        }
        let valid_until: Option<chrono::DateTime<Utc>> = request.get("valid_until");
        let execution_request = SubmitRequestDto {
            cluster_id,
            request_type: request.get("request_type"),
            request_details: details,
            reason: String::new(),
            valid_until,
        };
        if execution_request.valid_until.is_some() {
            return Err(ApiError::validation_error(
                "Temporary permission expiry is not supported yet.",
            ));
        }
        Self::validate_payload(
            &execution_request.request_type,
            &execution_request.request_details,
            &cluster,
            mysql_pool_manager.clone(),
        )
        .await?;

        let now = Utc::now();
        let mut persisted_details = execution_request.request_details.clone();
        persisted_details.new_user_password = None;
        let transition = db_query::query(
            "UPDATE permission_requests SET status = 'executing', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ?, request_details = ?, new_user_password_encrypted = ?
             WHERE id = ? AND status = 'pending'",
        )
        .bind(approver_id)
        .bind(&dto.comment)
        .bind(now)
        .bind(now)
        .bind(serde_json::to_string(&persisted_details)?)
        .bind(encrypted_password)
        .bind(request_id)
        .execute(&self.pool)
        .await?;
        if transition.rows_affected() != 1 {
            return Err(ApiError::validation_error(
                "The request has already been processed by another approver.",
            ));
        }

        self.execute_request_internal(request_id, &cluster, &execution_request, mysql_pool_manager)
            .await
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

        let comment = dto.comment.unwrap_or_default().trim().to_string();
        if comment.is_empty() {
            return Err(ApiError::validation_error("A rejection reason is required."));
        }

        let now = Utc::now();

        let transition = db_query::query(
            "UPDATE permission_requests SET status = 'rejected', approver_id = ?, approval_comment = ?,
             approved_at = ?, updated_at = ? WHERE id = ? AND status = 'pending'"
        )
        .bind(approver_id)
        .bind(comment)
        .bind(now)
        .bind(now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;
        if transition.rows_affected() != 1 {
            return Err(ApiError::validation_error("Only pending requests can be rejected."));
        }

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
        let transition = db_query::query(
            "UPDATE permission_requests SET status = 'cancelled', updated_at = ? WHERE id = ? AND status = 'pending'",
        )
        .bind(&now)
        .bind(request_id)
        .execute(&self.pool)
        .await?;
        if transition.rows_affected() != 1 {
            return Err(ApiError::validation_error("Can only cancel pending requests."));
        }

        Ok(())
    }

    /// Get request detail by ID
    pub async fn get_request_detail_for_user(
        &self,
        request_id: i64,
        requester_id: i64,
    ) -> ApiResult<PermissionRequestResponse> {
        self.check_request_visibility(request_id, requester_id)
            .await?;
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

    /// Validate and generate a redacted SQL preview for the active cluster.
    pub async fn preview_sql(
        &self,
        cluster: &Cluster,
        req: &SubmitRequestDto,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<String> {
        let mut details = req.request_details.clone();
        if req.valid_until.is_some() {
            return Err(ApiError::validation_error(
                "Temporary permission expiry is not supported yet.",
            ));
        }
        Self::validate_payload(
            &req.request_type,
            &req.request_details,
            cluster,
            mysql_pool_manager.clone(),
        )
        .await?;
        if details.new_user_password.is_some() {
            details.new_user_password = Some("********".to_string());
        }
        Self::generate_preview_sql(cluster, &req.request_type, &details, mysql_pool_manager).await
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
                let catalog = details.catalog.as_deref();

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
                            catalog,
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
                            catalog,
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
                            catalog,
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
                            catalog,
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
                let catalog = details.catalog.as_deref();

                let user = details
                    .target_user
                    .as_ref()
                    .ok_or(ApiError::ValidationError(
                        "Missing target_user for revoke_permission".to_string(),
                    ))?;

                let revoke_sql = adapter
                    .revoke_permissions(
                        "USER",
                        user,
                        &perms_ref,
                        resource_type,
                        catalog,
                        &database,
                        table,
                    )
                    .await?;
                Ok(revoke_sql)
            },
            _ => Err(ApiError::ValidationError(format!("Unknown request_type: {}", request_type))),
        }
    }

    /// Execute a validated request immediately. The generated SQL exists only in memory so
    /// a newly supplied password cannot leak through `executed_sql` or logs.
    async fn execute_request_internal(
        &self,
        request_id: i64,
        cluster: &Cluster,
        request: &SubmitRequestDto,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        let sql = Self::generate_preview_sql(
            cluster,
            &request.request_type,
            &request.request_details,
            mysql_pool_manager,
        )
        .await?;
        let audit_sql =
            Self::redact_password(&sql, request.request_details.new_user_password.as_deref());
        db_query::query("UPDATE permission_requests SET executed_sql = ? WHERE id = ?")
            .bind(audit_sql)
            .bind(request_id)
            .execute(&self.pool)
            .await?;

        let (exec_user, exec_pass) = cluster.get_execution_credentials();
        tracing::info!(
            "Executing permission request {} as configured cluster administrator",
            request_id
        );
        let execution =
            Self::execute_sql_with_admin_user(cluster, exec_user, exec_pass, &sql).await;
        let now = Utc::now();
        match execution {
            Ok(()) => {
                db_query::query(
                    "UPDATE permission_requests SET status = 'completed', execution_result = ?, executed_at = ?, updated_at = ? WHERE id = ? AND status = 'executing'",
                )
                .bind("执行成功")
                .bind(now)
                .bind(now)
                .bind(request_id)
                .execute(&self.pool)
                .await?;
                Ok(())
            },
            Err(error) => {
                // Driver errors can include statement text. Persist only a neutral result.
                tracing::warn!(request_id, "Permission request execution failed");
                db_query::query(
                    "UPDATE permission_requests SET status = 'failed', execution_result = ?, executed_at = ?, updated_at = ? WHERE id = ? AND status = 'executing'",
                )
                .bind("执行失败，请检查集群管理用户权限和连接配置。")
                .bind(now)
                .bind(now)
                .bind(request_id)
                .execute(&self.pool)
                .await?;
                Err(error)
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

        for statement in Self::split_sql_statements(sql) {
            conn.query_drop(statement).await.map_err(|e| {
                tracing::warn!("Permission SQL execution failed: {}", e);
                ApiError::cluster_connection_failed("SQL执行失败".to_string())
            })?;
        }

        // Connection will be dropped automatically when it goes out of scope
        tracing::info!("SQL executed successfully using admin user: {}", admin_user);
        Ok(())
    }

    pub(crate) fn encrypt_credential(&self, plaintext: &str) -> ApiResult<String> {
        let key = self.credential_key.ok_or_else(|| {
            ApiError::validation_error(
                "New database users require APP_PERMISSION_REQUEST_ENCRYPTION_KEY so their password can be encrypted.",
            )
        })?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| {
            ApiError::InternalError("Invalid credential encryption key".to_string())
        })?;
        let mut rng = aes_gcm::aead::OsRng;
        let nonce = Aes256Gcm::generate_nonce(&mut rng);
        let ciphertext = cipher.encrypt(&nonce, plaintext.as_bytes()).map_err(|_| {
            ApiError::InternalError("Failed to encrypt request credential".to_string())
        })?;
        let mut payload = nonce.to_vec();
        payload.extend(ciphertext);
        Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(payload)))
    }

    pub(crate) fn decrypt_credential(&self, encrypted: &str) -> ApiResult<String> {
        let key = self.credential_key.ok_or_else(|| {
            ApiError::validation_error(
                "This request cannot be executed because credential encryption is unavailable.",
            )
        })?;
        let encoded = encrypted.strip_prefix("v1:").ok_or_else(|| {
            ApiError::validation_error(
                "Unsupported credential encryption format. Resubmit the request.",
            )
        })?;
        let payload = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
            ApiError::validation_error("Invalid encrypted credential. Resubmit the request.")
        })?;
        if payload.len() <= 12 {
            return Err(ApiError::validation_error(
                "Invalid encrypted credential. Resubmit the request.",
            ));
        }
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| {
            ApiError::InternalError("Invalid credential encryption key".to_string())
        })?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&payload[..12]), &payload[12..])
            .map_err(|_| {
                ApiError::validation_error("Unable to decrypt credential. Resubmit the request.")
            })?;
        String::from_utf8(plaintext).map_err(|_| {
            ApiError::validation_error("Invalid credential encoding. Resubmit the request.")
        })
    }

    fn split_sql_statements(sql: &str) -> Vec<&str> {
        let mut result = Vec::new();
        let mut start = 0;
        let mut quoted = false;
        let mut chars = sql.char_indices().peekable();
        while let Some((index, character)) = chars.next() {
            if character == '\'' {
                if quoted && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    quoted = !quoted;
                }
            } else if character == ';' && !quoted {
                let statement = sql[start..index].trim();
                if !statement.is_empty() {
                    result.push(statement);
                }
                start = index + 1;
            }
        }
        let statement = sql[start..].trim();
        if !statement.is_empty() {
            result.push(statement);
        }
        result
    }

    fn redact_password(sql: &str, password: Option<&str>) -> String {
        password.map_or_else(
            || sql.to_string(),
            |password| {
                let value = password.replace('\'', "''");
                sql.replace(&format!("IDENTIFIED BY '{value}'"), "IDENTIFIED BY '********'")
            },
        )
    }

    async fn validate_request(
        req: &SubmitRequestDto,
        cluster: &Cluster,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        if req.reason.trim().is_empty() || req.reason.chars().count() > 500 {
            return Err(ApiError::validation_error(
                "Reason is required and must not exceed 500 characters.",
            ));
        }
        Self::validate_payload(&req.request_type, &req.request_details, cluster, mysql_pool_manager)
            .await
    }

    /// Payload-only validation shared by submit, preview and approval.
    async fn validate_payload(
        request_type: &str,
        details: &RequestDetails,
        cluster: &Cluster,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        Self::validate_request_details(request_type, details, cluster)?;
        Self::validate_live_principals(cluster, details, mysql_pool_manager).await
    }

    pub(crate) fn validate_request_details(
        request_type: &str,
        details: &RequestDetails,
        cluster: &Cluster,
    ) -> ApiResult<()> {
        if !matches!(request_type, "grant_permission" | "grant_role" | "revoke_permission")
            || details
                .action
                .as_deref()
                .is_some_and(|action| action != request_type)
        {
            return Err(ApiError::validation_error("Invalid permission request action."));
        }
        if details.target_account.is_some() {
            return Err(ApiError::validation_error(
                "Account host is managed by the platform and cannot be specified.",
            ));
        }
        if details.with_grant_option.unwrap_or(false) {
            return Err(ApiError::validation_error(
                "WITH GRANT OPTION is not available through permission requests.",
            ));
        }

        let validate_user = |user: &str| -> ApiResult<()> {
            Self::validate_identifier("user", user)?;
            if user.eq_ignore_ascii_case("root")
                || cluster
                    .admin_user
                    .as_deref()
                    .is_some_and(|admin| admin.eq_ignore_ascii_case(user))
            {
                return Err(ApiError::forbidden(
                    "Protected database accounts cannot be changed through permission requests.",
                ));
            }
            Ok(())
        };
        let validate_role = |role: &str| -> ApiResult<()> {
            Self::validate_identifier("role", role)?;
            if ["root", "admin", "operator", "public"]
                .iter()
                .any(|protected| role.eq_ignore_ascii_case(protected))
            {
                return Err(ApiError::forbidden(
                    "Built-in or protected roles cannot be assigned or modified.",
                ));
            }
            Ok(())
        };

        match request_type {
            "grant_role" => {
                let user = Self::required(&details.target_user, "target user")?;
                let role = Self::required(&details.target_role, "target role")?;
                validate_user(user)?;
                validate_role(role)?;
                if details.new_user_name.is_some()
                    || details.new_user_password.is_some()
                    || details.new_role_name.is_some()
                    || details.permissions.is_some()
                    || details.resource_type.is_some()
                    || details.catalog.is_some()
                    || details.database.is_some()
                    || details.table.is_some()
                {
                    return Err(ApiError::validation_error(
                        "Grant-role requests may only contain one existing user and one existing custom role.",
                    ));
                }
            },
            "grant_permission" | "revoke_permission" => {
                Self::validate_resource_and_permissions(details)?;
                let new_user = details.new_user_name.as_deref();
                let new_role = details.new_role_name.as_deref();
                let target_user = details.target_user.as_deref();
                let target_role = details.target_role.as_deref();
                if request_type == "revoke_permission" {
                    let user = Self::required(&details.target_user, "target user")?;
                    validate_user(user)?;
                    if target_role.is_some()
                        || new_user.is_some()
                        || new_role.is_some()
                        || details.new_user_password.is_some()
                    {
                        return Err(ApiError::validation_error(
                            "Revoke requests may only target one existing user.",
                        ));
                    }
                } else if let Some(name) = new_user {
                    validate_user(name)?;
                    if details
                        .new_user_password
                        .as_deref()
                        .is_none_or(str::is_empty)
                        || details
                            .new_user_password
                            .as_deref()
                            .is_some_and(|password| {
                                password.chars().count() > 256 || password.contains('\0')
                            })
                    {
                        return Err(ApiError::validation_error(
                            "A non-empty new-user password of at most 256 characters is required.",
                        ));
                    }
                    if target_user.is_some() || target_role.is_some() || new_role.is_some() {
                        return Err(ApiError::validation_error(
                            "New-user requests cannot include another target principal.",
                        ));
                    }
                } else if let Some(name) = new_role {
                    validate_role(name)?;
                    let user = Self::required(&details.target_user, "target user")?;
                    validate_user(user)?;
                    if target_role.is_some() || details.new_user_password.is_some() {
                        return Err(ApiError::validation_error(
                            "New-role requests require one existing target user only.",
                        ));
                    }
                } else {
                    match (target_user, target_role) {
                        (Some(user), None) => validate_user(user)?,
                        (None, Some(role)) => validate_role(role)?,
                        _ => {
                            return Err(ApiError::validation_error(
                                "Select exactly one existing user or custom role.",
                            ));
                        },
                    }
                }
            },
            _ => unreachable!(),
        }
        Ok(())
    }

    async fn validate_live_principals(
        cluster: &Cluster,
        details: &RequestDetails,
        mysql_pool_manager: Arc<MySQLPoolManager>,
    ) -> ApiResult<()> {
        let adapter = create_adapter(cluster.clone(), mysql_pool_manager);
        let accounts = adapter.list_db_accounts().await?;
        let roles = adapter.list_db_roles().await?;
        let user_exists = |name: &str| {
            accounts
                .iter()
                .any(|account| account.account_name == name && account.host == "%")
        };
        let custom_role_exists = |name: &str| {
            roles
                .iter()
                .any(|role| role.role_name == name && role.role_type.eq_ignore_ascii_case("custom"))
        };

        if let Some(name) = &details.target_user {
            if !user_exists(name) {
                return Err(ApiError::validation_error(
                    "The selected database user no longer exists.",
                ));
            }
        }
        if let Some(name) = &details.new_user_name {
            if user_exists(name) {
                return Err(ApiError::validation_error("The new database user already exists."));
            }
        }
        if let Some(name) = &details.target_role {
            if !custom_role_exists(name) {
                return Err(ApiError::validation_error(
                    "The selected role is unavailable or protected.",
                ));
            }
        }
        if let Some(name) = &details.new_role_name {
            if roles.iter().any(|role| role.role_name == *name) {
                return Err(ApiError::validation_error("The new role already exists."));
            }
        }
        Ok(())
    }

    fn validate_resource_and_permissions(details: &RequestDetails) -> ApiResult<()> {
        let resource_type = details
            .resource_type
            .as_deref()
            .ok_or_else(|| ApiError::validation_error("Select a resource level."))?;
        let catalog = Self::required(&details.catalog, "catalog")?;
        let allowed: &[&str] = match resource_type {
            "catalog" => {
                Self::validate_resource_identifier("catalog", catalog, true)?;
                if details.database.is_some() || details.table.is_some() {
                    return Err(ApiError::validation_error(
                        "Catalog permissions cannot include database or table.",
                    ));
                }
                &["USAGE", "CREATE DATABASE", "DROP", "ALL"]
            },
            "database" => {
                Self::validate_resource_identifier("catalog", catalog, false)?;
                Self::validate_resource_identifier(
                    "database",
                    Self::required(&details.database, "database")?,
                    true,
                )?;
                if details.table.is_some() {
                    return Err(ApiError::validation_error(
                        "Database permissions cannot include a table.",
                    ));
                }
                &[
                    "ALTER",
                    "DROP",
                    "CREATE TABLE",
                    "CREATE VIEW",
                    "CREATE FUNCTION",
                    "CREATE MATERIALIZED VIEW",
                    "ALL",
                ]
            },
            "table" => {
                Self::validate_resource_identifier("catalog", catalog, false)?;
                let database = Self::required(&details.database, "database")?;
                let table = Self::required(&details.table, "table")?;
                Self::validate_resource_identifier("database", database, true)?;
                Self::validate_resource_identifier("table", table, true)?;
                if database == "*" && table != "*" {
                    return Err(ApiError::validation_error(
                        "A specific table requires a specific database.",
                    ));
                }
                &["SELECT", "INSERT", "UPDATE", "DELETE", "ALTER", "DROP", "EXPORT", "ALL"]
            },
            _ => return Err(ApiError::validation_error("Invalid resource level.")),
        };
        let permissions = details
            .permissions
            .as_deref()
            .filter(|permissions| !permissions.is_empty())
            .ok_or_else(|| ApiError::validation_error("Select at least one permission."))?;
        if permissions
            .iter()
            .any(|permission| !allowed.contains(&permission.as_str()))
            || (permissions.iter().any(|permission| permission == "ALL") && permissions.len() != 1)
        {
            return Err(ApiError::validation_error(
                "The selected permissions are not valid for this resource level.",
            ));
        }
        Ok(())
    }

    fn required<'a>(value: &'a Option<String>, field: &str) -> ApiResult<&'a str> {
        value
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ApiError::validation_error(format!("Missing {field}.")))
    }

    fn validate_identifier(field: &str, value: &str) -> ApiResult<()> {
        let mut characters = value.chars();
        if value.len() > 64
            || !characters
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
            || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(ApiError::validation_error(format!("Invalid {field} identifier.")));
        }
        Ok(())
    }

    fn validate_resource_identifier(
        field: &str,
        value: &str,
        allow_wildcard: bool,
    ) -> ApiResult<()> {
        if allow_wildcard && value == "*" {
            return Ok(());
        }
        Self::validate_identifier(field, value)
    }

    fn row_to_response(&self, row: DB::Row) -> ApiResult<PermissionRequestResponse> {
        let request_details_str: String = row.get("request_details");
        let mut request_details: RequestDetails = serde_json::from_str(&request_details_str)?;
        // Compatibility for records written before the encrypted credential column existed.
        request_details.new_user_password = None;

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
            // SQL is an audit summary, not an API for retrieving executed statements.
            executed_sql: None,
            execution_result: row.get("execution_result"),
            executed_at: row.get("executed_at"),
            preview_sql: row.get::<Option<String>, _>("executed_sql"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// Permission to open the approval queue is independent from page routing.
    pub async fn ensure_can_review_requests(&self, user_id: i64) -> ApiResult<()> {
        let allowed = db_query::query(
            "SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id
             WHERE ur.user_id = ? AND r.code IN (?, ?) LIMIT 1",
        )
        .bind(user_id)
        .bind("super_admin")
        .bind("org_admin")
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        if allowed {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "Only organization administrators or super administrators can review permission requests.",
            ))
        }
    }

    async fn check_request_visibility(&self, request_id: i64, user_id: i64) -> ApiResult<()> {
        let row = db_query::query("SELECT applicant_id FROM permission_requests WHERE id = ?")
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Permission request not found".to_string()))?;
        let applicant_id: i64 = row.get("applicant_id");
        if applicant_id == user_id {
            Ok(())
        } else {
            self.check_approval_permission(request_id, user_id).await
        }
    }

    /// Check if the approver has permission to approve/reject this request
    /// Only organization admins or super admins can approve requests
    async fn check_approval_permission(&self, request_id: i64, approver_id: i64) -> ApiResult<()> {
        let request = db_query::query(
            "SELECT applicant_org_id, applicant_id FROM permission_requests WHERE id = ?",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Permission request not found".to_string()))?;
        let applicant_org_id: Option<i64> = request.get("applicant_org_id");
        let applicant_id: i64 = request.get("applicant_id");
        let approver = db_query::query("SELECT organization_id FROM users WHERE id = ?")
            .bind(approver_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Approver not found".to_string()))?;
        let approver_org_id: Option<i64> = approver.get("organization_id");
        let role_rows = db_query::query(
            "SELECT r.code FROM user_roles ur JOIN roles r ON r.id = ur.role_id WHERE ur.user_id = ?",
        )
        .bind(approver_id)
        .fetch_all(&self.pool)
        .await?;
        let is_super_admin = role_rows
            .iter()
            .any(|row| row.get::<String, _>("code") == "super_admin");
        let is_org_admin = role_rows
            .iter()
            .any(|row| row.get::<String, _>("code") == "org_admin");

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
