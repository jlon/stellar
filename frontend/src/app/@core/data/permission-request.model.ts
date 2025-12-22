/**
 * Permission Request Domain Models
 * 权限申请相关的数据模型定义
 */

export interface RequestDetails {
  action?: 'grant_role' | 'grant_permission' | 'revoke_permission';
  target_user?: string;
  target_role?: string;
  target_account?: string;
  permissions?: string[];
  scope?: 'global' | 'database' | 'table';
  database?: string;
  table?: string;
  resource_type?: 'catalog' | 'database' | 'table' | 'column';
  catalog?: string;
  with_grant_option?: boolean;
  valid_until?: string;
  new_user_name?: string;
  new_user_password?: string;
  new_role_name?: string;
}

export interface SubmitRequestDto {
  cluster_id: number;
  request_type: 'create_account' | 'grant_role' | 'grant_permission' | 'revoke_permission';
  request_details: RequestDetails;
  reason: string;
  valid_until?: string;
}

export interface ApprovalDto {
  comment?: string;
}

export interface PermissionRequest {
  id: number;
  cluster_id: number;
  cluster_name?: string;
  applicant_id: number;
  applicant_name?: string;
  applicant_org_id: number;
  request_type: 'create_account' | 'grant_role' | 'grant_permission' | 'revoke_permission';
  request_details: RequestDetails;
  reason: string;
  valid_until?: string;
  status: 'pending' | 'approved' | 'rejected' | 'executing' | 'completed' | 'failed';
  approver_id?: number;
  approver_name?: string;
  approval_comment?: string;
  approved_at?: string;
  executed_sql?: string;
  execution_result?: string;
  executed_at?: string;
  preview_sql?: string;
  created_at: string;
  updated_at: string;
}

export interface PermissionRequestResponse {
  id: number;
  cluster_id: number;
  cluster_name?: string;
  applicant_id: number;
  applicant_name?: string;
  applicant_org_id: number;
  request_type: string;
  request_details: RequestDetails;
  reason: string;
  valid_until?: string;
  status: 'pending' | 'approved' | 'rejected' | 'executing' | 'completed' | 'failed';
  approver_id?: number;
  approver_name?: string;
  approval_comment?: string;
  approved_at?: string;
  executed_sql?: string;
  execution_result?: string;
  executed_at?: string;
  preview_sql?: string;
  created_at: string;
  updated_at: string;
}

export interface RequestQueryFilter {
  status?: string;
  request_type?: string;
  page?: number;
  page_size?: number;
}

export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export interface DbAccountDto {
  account_name: string;
  host: string;
  roles: string[];
}

export interface DbRoleDto {
  role_name: string;
  role_type: 'built-in' | 'custom';
  permissions_count?: number;
}

export interface DbUserPermissionDto {
  id: number;
  privilege_type: string;
  resource_type: string;  // ROLE, GLOBAL, SYSTEM, CATALOG, DATABASE, TABLE
  resource_path: string;
  granted_role?: string;
  granted_at?: string;
}
