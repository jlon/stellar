// @ts-nocheck
import { Component, OnDestroy } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { PermissionService } from '../../../@core/data/permission.service';
import { AuthService } from '../../../@core/data/auth.service';

/**
 * PermissionManagementComponent
 * Main container for permission management system with 7 tabs
 *
 * Tabs (权限工单 - Permission Requests):
 * 1. 我的权限 - View current user's permissions with revoke capability (all users)
 * 2. 权限申请 - Submit new permission requests (grant role/permission, revoke permission) (all users)
 * 3. 权限审批 - Review and approve/reject pending requests (org admins + super admins only)
 *
 * Tabs (权限配置 - Permission Configuration):
 * 4. 系统用户 - Manage system users and their roles (admins only)
 * 5. 系统角色 - View and manage system roles and permissions (admins only)
 * 6. 数据库账户 - View database accounts from OLAP engines (admins only)
 * 7. 数据库角色 - View database roles from OLAP engines (admins only)
 *
 * Features:
 * - Tab-based navigation with permission-based visibility
 * - Cross-tab communication via event streams
 * - Support for Doris and StarRocks RBAC models
 */
@Component({
  selector: 'ngx-permission-management',
  templateUrl: './permission-management.component.html',
  styleUrls: ['./permission-management.component.scss'],
})
export class PermissionManagementComponent implements OnDestroy {
  activeTabIndex = 0;

  // Event streams for cross-tab communication (Permission Request tabs)
  refreshDashboard$ = new Subject<void>();
  refreshMyRequests$ = new Subject<void>();
  refreshPendingApprovals$ = new Subject<void>();

  // Event stream for configuration data refresh (Permission Config tabs)
  refreshConfigData$ = new Subject<void>();

  private destroy$ = new Subject<void>();

  constructor(
    private permissionService: PermissionService,
    private authService: AuthService,
  ) {}

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Check if current user can view approval tab
   * Only super admins and organization admins can approve requests
   */
  canViewApprovalTab(): boolean {
    const currentUser = this.authService.currentUserValue;

    // Super admin can always view
    if (this.authService.isSuperAdmin()) {
      return true;
    }

    // Check if user has specific admin permissions
    return this.permissionService.hasMenuPermission('cluster-ops:permission-management:approval')
      || this.permissionService.hasPermission('api:permission-requests:approve');
  }

  /**
   * Check if current user can view configuration tabs (system users, roles, db accounts, db roles)
   * Requires: admin privilege or explicit permission
   */
  canViewConfigTabs(): boolean {
    // Super admin can always view
    if (this.authService.isSuperAdmin()) {
      return true;
    }

    return this.permissionService.hasMenuPermission('cluster-ops:permission-management:config')
      || this.permissionService.hasMenuPermission('cluster-ops:permission-management');
  }

  /**
   * Handle tab change
   */
  onTabChange(event: any): void {
    this.activeTabIndex = event.tabId;
  }

  /**
   * Trigger refresh for dashboard (after revoke action)
   */
  triggerRefreshDashboard(): void {
    this.refreshDashboard$.next();
  }

  /**
   * Trigger refresh for my requests list
   */
  triggerRefreshMyRequests(): void {
    this.refreshMyRequests$.next();
  }

  /**
   * Trigger refresh for pending approvals list
   */
  triggerRefreshPendingApprovals(): void {
    this.refreshPendingApprovals$.next();
  }

  /**
   * Handle request submission (from Request tab)
   * - Refresh dashboard to show new request
   * - Refresh my requests list
   */
  onRequestSubmitted(): void {
    this.triggerRefreshMyRequests();
    // Note: Dashboard will refresh when request is completed/executed
  }

  /**
   * Handle approval/rejection (from Approval tab)
   * - Refresh pending approvals list
   * - Refresh my requests list (status changes)
   */
  onRequestProcessed(): void {
    this.triggerRefreshPendingApprovals();
    this.triggerRefreshMyRequests();
  }
}
