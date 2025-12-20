/**
 * Permission Request Domain Models
 * 权限申请相关的数据模型定义
 */

export interface RequestDetails {
  target_account?: string;
  target_role?: string;
  permissions?: string[];
  scope?: 'global' | 'database' | 'table';
  database?: string;
  table?: string;
  with_grant_option?: boolean;
  valid_until?: string;
}

export interface SubmitRequestDto {
  cluster_id: number;
  request_type: 'create_account' | 'grant_role' | 'grant_permission';
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
  request_type: 'create_account' | 'grant_role' | 'grant_permission';
  request_details: RequestDetails;
  reason: string;
  valid_until?: string;
  status: 'pending' | 'approved' | 'rejected' | 'completed';
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
  status: 'pending' | 'approved' | 'rejected' | 'completed';
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
