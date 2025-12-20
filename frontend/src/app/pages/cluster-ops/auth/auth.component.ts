// @ts-nocheck
import { Component, OnInit, OnDestroy } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';

/**
 * AuthComponent
 * 权限管控模块主容器组件
 *
 * 功能：
 * - 管理4个Tab页面（我的申请、待审批、账户列表、角色列表）
 * - 处理Tab切换逻辑
 * - 提供跨Tab的事件通信（如刷新列表）
 */
@Component({
  selector: 'ngx-cluster-ops-auth',
  templateUrl: './auth.component.html',
  styleUrls: ['./auth.component.scss'],
})
export class AuthComponent implements OnInit, OnDestroy {
  activeTabIndex = 0;

  // 用于跨Tab通信的事件流
  refreshMyRequests$ = new Subject<void>();
  refreshPendingApprovals$ = new Subject<void>();
  refreshAccounts$ = new Subject<void>();
  refreshRoles$ = new Subject<void>();

  private destroy$ = new Subject<void>();

  constructor() {}

  ngOnInit(): void {
    // 初始化逻辑（如果需要）
  }

  ngOnDestroy(): void {
    // 清理资源
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Tab变化处理
   */
  onTabChange(index: number): void {
    this.activeTabIndex = index;
  }

  /**
   * 触发刷新我的申请列表
   */
  triggerRefreshMyRequests(): void {
    this.refreshMyRequests$.next();
  }

  /**
   * 触发刷新待审批列表
   */
  triggerRefreshPendingApprovals(): void {
    this.refreshPendingApprovals$.next();
  }

  /**
   * 触发刷新账户列表
   */
  triggerRefreshAccounts(): void {
    this.refreshAccounts$.next();
  }

  /**
   * 触发刷新角色列表
   */
  triggerRefreshRoles(): void {
    this.refreshRoles$.next();
  }

  /**
   * 权限申请成功后的处理（触发刷新）
   */
  onRequestSubmitted(): void {
    this.triggerRefreshMyRequests();
  }

  /**
   * 审批或拒绝申请后的处理
   */
  onRequestProcessed(): void {
    this.triggerRefreshPendingApprovals();
    this.triggerRefreshMyRequests();
  }
}
