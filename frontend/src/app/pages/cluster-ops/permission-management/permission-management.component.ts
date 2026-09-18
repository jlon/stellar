import { TranslatePipe } from '@ngx-translate/core';
// @ts-nocheck
import { Component, OnDestroy, ViewChild, inject } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { PermissionService } from '../../../@core/data/permission.service';
import { AuthService } from '../../../@core/data/auth.service';
import { PermissionRequestComponent } from './request/permission-request.component';
import { NbTabsetModule, NbTabsetComponent } from '@nebular/theme';
import { PermissionDashboardStandardComponent } from './dashboard/permission-dashboard-standard.component';

import { PermissionApprovalComponent } from './approval/permission-approval.component';

@Component({
    selector: 'ngx-permission-management',
    templateUrl: './permission-management.component.html',
    styleUrls: ['./permission-management.component.scss'],
    imports: [
    NbTabsetModule,
    TranslatePipe,
    PermissionDashboardStandardComponent,
    PermissionRequestComponent,
    PermissionApprovalComponent
],
})
export class PermissionManagementComponent implements OnDestroy {
  private permissionService = inject(PermissionService);
  private authService = inject(AuthService);

  activeTabIndex = 0;

  // 预填的撤销权限信息
  prefillRevokeData: any = null;

  // Event streams for cross-tab communication (Permission Request tabs)
  refreshDashboard$ = new Subject<void>();
  refreshMyRequests$ = new Subject<void>();
  refreshPendingApprovals$ = new Subject<void>();

  @ViewChild(PermissionRequestComponent) requestComponent: PermissionRequestComponent;
  // 本版 NbTabset 无 selectedTab 双向绑定，编程式切 tab 只能调 selectTab()。
  @ViewChild(NbTabsetComponent) tabsetRef: NbTabsetComponent;

  private destroy$ = new Subject<void>();

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
   * Handle tab change
   */
  onTabChange(tabId: number): void {
    this.activeTabIndex = tabId;
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

  /**
   * Handle switch to request tab (from Dashboard)
   * - Switch to request tab
   * - Prefill revoke data if provided
   */
  onSwitchToRequest(event: {type: string, permission?: any}): void {
    if (this.activeTabIndex !== 1) {
      this.activeTabIndex = 1;
      // tab 内容常驻渲染，直接通过 tabset 选中「权限申请」tab（静态绑定的 tabId 是字符串）。
      const target = this.tabsetRef?.tabs?.find(t => String(t.tabId) === '1');
      if (target) {
        this.tabsetRef.selectTab(target);
      }
    }

    if (event.type === 'revoke_permission' && event.permission) {
      this.prefillRevokeData = event.permission;
      this.requestComponent?.prefillRevokeRequest(event.permission);
    }
  }
}
