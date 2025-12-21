//! Handler 公共辅助函数模块
//!
//! 提取 handlers 中的重复逻辑，提供统一的辅助函数

use std::sync::Arc;

use crate::middleware::OrgContext;
use crate::models::Cluster;
use crate::services::ClusterService;
use crate::utils::ApiResult;

/// 根据组织上下文获取活跃集群
///
/// 统一处理 super_admin 和普通用户的集群获取逻辑
/// 
/// # Example
/// ```ignore
/// let cluster = get_active_cluster_for_org(&state.cluster_service, &org_ctx).await?;
/// ```
pub async fn get_active_cluster_for_org(
    cluster_service: &Arc<ClusterService>,
    org_ctx: &OrgContext,
) -> ApiResult<Cluster> {
    if org_ctx.is_super_admin {
        cluster_service.get_active_cluster().await
    } else {
        cluster_service
            .get_active_cluster_by_org(org_ctx.organization_id)
            .await
    }
}

/// 检查用户是否有权限访问指定组织的资源
///
/// # Returns
/// - `Ok(())` 如果有权限
/// - `Err(ApiError::forbidden(...))` 如果无权限
pub fn check_org_access(
    org_ctx: &OrgContext,
    resource_org_id: Option<i64>,
    action_desc: &str,
) -> ApiResult<()> {
    if org_ctx.is_super_admin {
        return Ok(());
    }
    
    if resource_org_id != org_ctx.organization_id {
        return Err(crate::utils::ApiError::forbidden(format!(
            "You can only {} within your organization",
            action_desc
        )));
    }
    
    Ok(())
}

/// 检查非超级管理员是否尝试修改组织归属
///
/// # Returns
/// - `Ok(())` 如果允许操作
/// - `Err(ApiError::forbidden(...))` 如果非法操作
pub fn check_org_reassignment(
    org_ctx: &OrgContext,
    new_org_id: Option<i64>,
    current_org_id: Option<i64>,
    resource_type: &str,
) -> ApiResult<()> {
    if org_ctx.is_super_admin {
        return Ok(());
    }
    
    if new_org_id.is_some() && new_org_id != current_org_id {
        return Err(crate::utils::ApiError::forbidden(format!(
            "Only super administrators can reassign {} organization",
            resource_type
        )));
    }
    
    Ok(())
}

/// 检查非超级管理员是否尝试覆盖组织分配
pub fn check_org_override(org_ctx: &OrgContext, requested_org: Option<i64>) -> ApiResult<()> {
    if !org_ctx.is_super_admin && requested_org.is_some() {
        return Err(crate::utils::ApiError::forbidden(
            "Organization administrators cannot override organization assignment",
        ));
    }
    Ok(())
}
