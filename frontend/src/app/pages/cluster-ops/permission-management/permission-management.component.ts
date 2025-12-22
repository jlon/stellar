// @ts-nocheck
import { Component, OnDestroy } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { PermissionService } from '../../../@core/data/permission.service';
import { AuthService } from '../../../@core/data/auth.service';

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
}
