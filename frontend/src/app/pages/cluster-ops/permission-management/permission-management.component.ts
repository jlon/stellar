// @ts-nocheck
import { Component, OnDestroy, ViewChild } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { PermissionService } from '../../../@core/data/permission.service';
import { AuthService } from '../../../@core/data/auth.service';
import { PermissionRequestComponent } from './request/permission-request.component';

@Component({
  selector: 'ngx-permission-management',
  templateUrl: './permission-management.component.html',
  styleUrls: ['./permission-management.component.scss'],
})
export class PermissionManagementComponent implements OnDestroy {
  activeTabIndex = 0;

  // 预填的撤销权限信息
  prefillRevokeData: any = null;

  // Event streams for cross-tab communication (Permission Request tabs)
  refreshDashboard$ = new Subject<void>();
  refreshMyRequests$ = new Subject<void>();
  refreshPendingApprovals$ = new Subject<void>();

  @ViewChild(PermissionRequestComponent) requestComponent: PermissionRequestComponent;

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
    this.activeTabIndex = 1; // Switch to request tab

    if (event.type === 'revoke_permission' && event.permission) {
      // Store prefill data for request component
      this.prefillRevokeData = event.permission;

      // If request component is already loaded, prefill it
      setTimeout(() => {
        if (this.requestComponent) {
          this.requestComponent.prefillRevokeRequest(event.permission);
        }
      }, 100);
    }
  }
}
