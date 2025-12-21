import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';
import {
  PermissionRequestResponse,
  SubmitRequestDto,
  ApprovalDto,
  RequestQueryFilter,
  PaginatedResponse,
  DbAccountDto,
  DbRoleDto,
  DbUserPermissionDto,
} from './permission-request.model';

/**
 * PermissionRequestService
 * 权限申请工作流API服务
 *
 * 职责：
 * - 管理权限申请的提交、审批、拒绝、取消
 * - 查询我的申请和待审批的申请
 * - 查询数据库账户和角色
 * - 生成权限申请预览SQL
 */
@Injectable({
  providedIn: 'root',
})
export class PermissionRequestService {
  constructor(private api: ApiService) {}

  /**
   * 获取我的权限申请列表（分页）
   */
  listMyRequests(filter?: RequestQueryFilter): Observable<PaginatedResponse<PermissionRequestResponse>> {
    const params = this.buildFilterParams(filter);
    return this.api.get<PaginatedResponse<PermissionRequestResponse>>(
      '/permission-requests/my',
      params,
    );
  }

  /**
   * 获取待审批的权限申请列表
   */
  listPendingApprovals(filter?: RequestQueryFilter): Observable<PermissionRequestResponse[]> {
    const params = this.buildFilterParams(filter);
    return this.api.get<PermissionRequestResponse[]>(
      '/permission-requests/pending',
      params,
    );
  }

  /**
   * 获取权限申请详情
   */
  getRequest(requestId: number): Observable<PermissionRequestResponse> {
    return this.api.get<PermissionRequestResponse>(`/permission-requests/${requestId}`);
  }

  /**
   * 提交权限申请
   */
  submitRequest(data: SubmitRequestDto): Observable<number> {
    return this.api.post<number>('/permission-requests', data);
  }

  /**
   * 审批权限申请
   */
  approveRequest(requestId: number, data: ApprovalDto): Observable<any> {
    return this.api.post(`/permission-requests/${requestId}/approve`, data);
  }

  /**
   * 拒绝权限申请
   */
  rejectRequest(requestId: number, data: ApprovalDto): Observable<any> {
    return this.api.post(`/permission-requests/${requestId}/reject`, data);
  }

  /**
   * 取消权限申请（仅申请人可操作）
   */
  cancelRequest(requestId: number): Observable<any> {
    return this.api.post(`/permission-requests/${requestId}/cancel`, {});
  }

  /**
   * 查询数据库账户列表（实时查询）
   * Backend determines active cluster from session context
   * Following the pattern used by node.service.ts
   */
  listDbAccounts(): Observable<DbAccountDto[]> {
    return this.api.get<DbAccountDto[]>(`/clusters/db-auth/accounts`);
  }

  /**
   * 查询数据库角色列表（实时查询）
   * Backend determines active cluster from session context
   * Following the pattern used by node.service.ts
   */
  listDbRoles(): Observable<DbRoleDto[]> {
    return this.api.get<DbRoleDto[]>(`/clusters/db-auth/roles`);
  }

  /**
   * 预览权限申请的SQL
   */
  previewSql(data: SubmitRequestDto): Observable<{ sql: string; request_type: string }> {
    return this.api.post<{ sql: string; request_type: string }>('/db-auth/preview-sql', data);
  }

  /**
   * 查询当前用户的数据库权限列表
   * Backend uses SHOW GRANTS to retrieve actual permissions from database
   */
  listMyDbPermissions(): Observable<DbUserPermissionDto[]> {
    return this.api.get<DbUserPermissionDto[]>(`/clusters/db-auth/my-permissions`);
  }

  /**
   * 构建查询参数对象
   */
  private buildFilterParams(filter?: RequestQueryFilter): any {
    if (!filter) {
      return {};
    }

    const params: any = {};
    if (filter.status) {
      params.status = filter.status;
    }
    if (filter.request_type) {
      params.request_type = filter.request_type;
    }
    if (filter.page) {
      params.page = filter.page;
    }
    if (filter.page_size) {
      params.page_size = filter.page_size;
    }

    return params;
  }
}
