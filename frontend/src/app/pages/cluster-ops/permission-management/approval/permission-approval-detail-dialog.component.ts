import { Component, Input, inject } from '@angular/core';
import { NbDialogRef, NbCardModule, NbBadgeModule, NbIconModule, NbListModule, NbButtonModule, NbTooltipModule } from '@nebular/theme';
import { PermissionRequestResponse } from '../../../../@core/data/permission-request.model';
import { NgClass } from '@angular/common';

/**
 * Permission Approval Detail Dialog Component
 * 使用 ngx-admin 原生 NbDialogService 实现
 * 设计风格与 user-form-dialog 保持一致
 */
@Component({
    selector: 'ngx-permission-approval-detail-dialog',
    template: `
    <nb-card class="approval-detail-dialog">
      <nb-card-header>
        <div class="dialog-header">
          <h6>申请详情</h6>
          <div class="dialog-status">
            <nb-badge [text]="getRequestTypeLabel(request.request_type)" [status]="getRequestTypeStatus(request.request_type)"></nb-badge>
            <nb-badge [text]="getStatusLabel(request.status)" [status]="getStatusBadge(request.status)"></nb-badge>
          </div>
        </div>
      </nb-card-header>
    
      <nb-card-body>
        <!-- 基本信息 - 使用 nb-list -->
        <div class="section">
          <div class="section-title">
            <nb-icon icon="person-outline"></nb-icon>
            <span>基本信息</span>
          </div>
          <nb-list class="info-list">
            <nb-list-item>
              <div class="list-item-content">
                <span class="item-label">申请人</span>
                <span class="item-value">{{ request.applicant_name || '-' }}</span>
              </div>
            </nb-list-item>
            <nb-list-item>
              <div class="list-item-content">
                <span class="item-label">目标集群</span>
                <span class="item-value">{{ request.cluster_name || '-' }}</span>
              </div>
            </nb-list-item>
            <nb-list-item>
              <div class="list-item-content">
                <span class="item-label">申请时间</span>
                <span class="item-value">{{ formatDateTime(request.created_at) }}</span>
              </div>
            </nb-list-item>
            @if (request.valid_until) {
              <nb-list-item>
                <div class="list-item-content">
                  <span class="item-label">有效期至</span>
                  <span class="item-value">{{ formatDateTime(request.valid_until) }}</span>
                </div>
              </nb-list-item>
            }
          </nb-list>
        </div>
    
        <!-- 权限详情 - 使用 nb-list -->
        @if (request.request_details) {
          <div class="section">
            <div class="section-title">
              <nb-icon icon="shield-outline"></nb-icon>
              <span>权限详情</span>
            </div>
            <!-- 授予角色类型 -->
            @if (request.request_type === 'grant_role') {
              <nb-list class="info-list">
                <nb-list-item>
                  <div class="list-item-content">
                    <span class="item-label">目标用户</span>
                    <code class="item-code">{{ request.request_details.target_user || '-' }}</code>
                  </div>
                </nb-list-item>
                <nb-list-item>
                  <div class="list-item-content">
                    <span class="item-label">授予角色</span>
                    <nb-badge [text]="request.request_details.target_role || '-'" status="info"></nb-badge>
                  </div>
                </nb-list-item>
              </nb-list>
            }
            <!-- 授予/撤销权限类型 -->
            @if (request.request_type !== 'grant_role') {
              <nb-list class="info-list">
                <nb-list-item>
                  <div class="list-item-content">
                    <span class="item-label">目标用户</span>
                    <code class="item-code">{{ request.request_details.target_user || '-' }}</code>
                  </div>
                </nb-list-item>
                @if (request.request_details.permissions?.length) {
                  <nb-list-item>
                    <div class="list-item-content">
                      <span class="item-label">申请权限</span>
                      <div class="permission-tags">
                        @for (perm of request.request_details.permissions; track perm) {
                          <nb-badge [text]="perm" status="success"></nb-badge>
                        }
                      </div>
                    </div>
                  </nb-list-item>
                }
                <nb-list-item>
                  <div class="list-item-content">
                    <span class="item-label">资源范围</span>
                    <code class="item-code">{{ buildResourcePath() }}</code>
                  </div>
                </nb-list-item>
              </nb-list>
            }
            <!-- 资源详情 -->
            @if (request.request_type !== 'grant_role') {
              <div class="resource-details">
                @if (request.request_details.resource_type) {
                  <span class="resource-item">
                    <nb-icon icon="layers-outline"></nb-icon>
                    类型: {{ request.request_details.resource_type }}
                  </span>
                }
                @if (request.request_details.catalog) {
                  <span class="resource-item">
                    <nb-icon icon="folder-outline"></nb-icon>
                    Catalog: {{ request.request_details.catalog }}
                  </span>
                }
                @if (request.request_details.database) {
                  <span class="resource-item">
                    <nb-icon icon="archive-outline"></nb-icon>
                    Database: {{ request.request_details.database }}
                  </span>
                }
                @if (request.request_details.table) {
                  <span class="resource-item">
                    <nb-icon icon="grid-outline"></nb-icon>
                    Table: {{ request.request_details.table }}
                  </span>
                }
              </div>
            }
          </div>
        }
    
        <!-- 申请理由 -->
        @if (request.reason) {
          <div class="section">
            <div class="section-title">
              <nb-icon icon="message-square-outline"></nb-icon>
              <span>申请理由</span>
            </div>
            <div class="reason-box">
              {{ request.reason }}
            </div>
          </div>
        }
    
        <!-- 审批信息 - 使用 nb-list -->
        @if (request.status !== 'pending') {
          <div class="section">
            <div class="section-title">
              <nb-icon icon="checkmark-circle-outline"></nb-icon>
              <span>审批信息</span>
            </div>
            <nb-list class="info-list">
              <nb-list-item>
                <div class="list-item-content">
                  <span class="item-label">审批人</span>
                  <span class="item-value">{{ request.approver_name || '-' }}</span>
                </div>
              </nb-list-item>
              <nb-list-item>
                <div class="list-item-content">
                  <span class="item-label">审批时间</span>
                  <span class="item-value">{{ formatDateTime(request.approved_at) }}</span>
                </div>
              </nb-list-item>
            </nb-list>
            @if (request.approval_comment) {
              <div class="reason-box mt-2">
                <label>审批备注</label>
                {{ request.approval_comment }}
              </div>
            }
          </div>
        }
    
        <!-- 执行信息 -->
        @if (request.status === 'completed' || request.status === 'failed') {
          <div class="section">
            <div class="section-title">
              <nb-icon icon="flash-outline"></nb-icon>
              <span>执行结果</span>
            </div>
            <div class="execution-result" [ngClass]="request.status">
              <nb-icon [icon]="request.status === 'completed' ? 'checkmark-circle-2-outline' : 'alert-circle-outline'"></nb-icon>
              <div class="result-content">
                <span class="result-status">{{ request.status === 'completed' ? '执行成功' : '执行失败' }}</span>
                @if (request.executed_at) {
                  <span class="result-time">{{ formatDateTime(request.executed_at) }}</span>
                }
                @if (request.execution_result) {
                  <p class="result-message">{{ request.execution_result }}</p>
                }
              </div>
            </div>
          </div>
        }
      </nb-card-body>
    
      <nb-card-footer>
        <button type="button" nbButton ghost size="small" status="basic" (click)="close()">关闭</button>
        @if (showActions && request.status === 'pending') {
          <button type="button" nbButton size="small" status="danger" outline (click)="reject()">
            <nb-icon icon="close-outline"></nb-icon>
            拒绝
          </button>
          <button type="button" nbButton size="small" status="success" (click)="approve()">
            <nb-icon icon="checkmark-outline"></nb-icon>
            批准
          </button>
        }
      </nb-card-footer>
    </nb-card>
    `,
    styles: [`
    :host {
      display: block;
      width: 32rem;
      max-width: calc(100vw - 2rem);
    }

    .approval-detail-dialog {
      margin: 0;
      
      nb-card-header {
        padding: 0.75rem 1rem !important;
      }
      
      nb-card-body {
        padding: 1rem !important;
        max-height: calc(80vh - 120px);
        overflow-y: auto;
      }
    }

    .dialog-header {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 0.75rem;

      h6 {
        margin: 0;
        font-size: 0.9375rem;
      }
    }

    .dialog-status {
      display: flex;
      flex-wrap: wrap;
      justify-content: flex-end;
      gap: 0.25rem;
    }

    .section {
      margin-bottom: 1rem;

      &:last-child {
        margin-bottom: 0;
      }
    }

    .section-title {
      display: flex;
      align-items: center;
      gap: 0.375rem;
      margin-bottom: 0.5rem;
      font-size: 0.8125rem;
      font-weight: 600;
      color: var(--text-basic-color);

      nb-icon {
        font-size: 0.875rem;
      }
    }

    /* nb-list 样式 */
    .info-list {
      border: 1px solid var(--border-basic-color-3);
      border-radius: 0.375rem;
      overflow: hidden;
    }

    :host ::ng-deep .info-list nb-list-item {
      padding: 0.5rem 0.75rem !important;
    }

    .list-item-content {
      display: flex;
      justify-content: space-between;
      align-items: center;
      width: 100%;
    }

    .item-label {
      font-size: 0.8125rem;
      color: var(--text-hint-color);
    }

    .item-value {
      font-size: 0.8125rem;
      color: var(--text-basic-color);
    }

    .item-code {
      font-size: 0.75rem;
      padding: 0.125rem 0.5rem;
      background: var(--background-basic-color-3);
      border-radius: 0.25rem;
      font-family: monospace;
    }

    .permission-tags {
      display: flex;
      flex-wrap: wrap;
      gap: 0.25rem;
    }

    .resource-details {
      display: flex;
      flex-wrap: wrap;
      gap: 0.5rem;
      margin-top: 0.5rem;
      padding: 0.5rem 0.75rem;
      background: var(--background-basic-color-2);
      border-radius: 0.375rem;
    }

    .resource-item {
      display: inline-flex;
      align-items: center;
      gap: 0.25rem;
      font-size: 0.6875rem;
      color: var(--text-hint-color);

      nb-icon {
        font-size: 0.75rem;
      }
    }

    .reason-box {
      background: var(--background-basic-color-2);
      border-radius: 0.375rem;
      padding: 0.625rem 0.75rem;
      font-size: 0.8125rem;
      line-height: 1.5;
      color: var(--text-basic-color);
      white-space: pre-wrap;
      border: 1px solid var(--border-basic-color-3);

      label {
        display: block;
        font-size: 0.6875rem;
        color: var(--text-hint-color);
        margin-bottom: 0.25rem;
      }
    }

    .execution-result {
      display: flex;
      align-items: flex-start;
      gap: 0.5rem;
      padding: 0.625rem 0.75rem;
      border-radius: 0.375rem;

      &.completed {
        background: var(--color-success-transparent-100);
        nb-icon { color: var(--color-success-default); }
      }

      &.failed {
        background: var(--color-danger-transparent-100);
        nb-icon { color: var(--color-danger-default); }
      }

      nb-icon {
        font-size: 1rem;
        flex-shrink: 0;
      }

      .result-content {
        flex: 1;
      }

      .result-status {
        font-weight: 600;
        font-size: 0.8125rem;
      }

      .result-time {
        margin-left: 0.375rem;
        font-size: 0.6875rem;
        color: var(--text-hint-color);
      }

      .result-message {
        margin: 0.375rem 0 0;
        font-size: 0.75rem;
        color: var(--text-basic-color);
      }
    }

    nb-card-footer {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
      padding: 0.75rem 1rem !important;
      border-top: 1px solid var(--border-basic-color-3);
    }

    /* Utilities */
    .mr-1 { margin-right: 0.25rem; }
    .mt-2 { margin-top: 0.5rem; }

    @media (max-width: 480px) {
      :host {
        width: calc(100vw - 1rem);
      }

      .list-item-content {
        flex-direction: column;
        align-items: flex-start;
        gap: 0.25rem;
      }
    }
  `],
    imports: [
    NbCardModule,
    NbBadgeModule,
    NbIconModule,
    NbListModule,
    NgClass,
    NbButtonModule,
    NbTooltipModule,],
})
export class PermissionApprovalDetailDialogComponent {
  protected dialogRef = inject<NbDialogRef<PermissionApprovalDetailDialogComponent>>(NbDialogRef);

  @Input() request: PermissionRequestResponse;
  @Input() showActions: boolean = false;

  close() {
    this.dialogRef.close();
  }

  approve() {
    this.dialogRef.close({ action: 'approve' });
  }

  reject() {
    this.dialogRef.close({ action: 'reject' });
  }

  formatDateTime(dateStr: string): string {
    if (!dateStr) return '-';
    return dateStr.replace('T', ' ').substring(0, 19);
  }

  buildResourcePath(): string {
    const details = this.request.request_details;
    if (!details) return '-';

    const parts: string[] = [];
    if (details.catalog) parts.push(details.catalog);
    if (details.database) parts.push(details.database);
    if (details.table) parts.push(details.table);

    if (parts.length === 0) {
      // resource_type 可能是 'global' 或其他值
      const resourceType = (details.resource_type as string)?.toUpperCase();
      if (resourceType === 'GLOBAL') return 'GLOBAL (全局)';
      return details.scope || '-';
    }

    return parts.join('.');
  }

  getRequestTypeLabel(type: string): string {
    const labels: { [key: string]: string } = {
      grant_role: '授予角色',
      grant_permission: '授予权限',
      revoke_role: '撤销角色',
      revoke_permission: '撤销权限',
    };
    return labels[type] || type;
  }

  getRequestTypeStatus(type: string): string {
    if (type?.includes('grant')) return 'success';
    if (type?.includes('revoke')) return 'warning';
    return 'info';
  }

  getStatusLabel(status: string): string {
    const labels: { [key: string]: string } = {
      pending: '待审批',
      approved: '已批准',
      rejected: '已拒绝',
      executing: '执行中',
      completed: '已完成',
      failed: '执行失败',
      expired: '已过期',
    };
    return labels[status] || status;
  }

  getStatusBadge(status: string): string {
    const badges: { [key: string]: string } = {
      pending: 'warning',
      approved: 'success',
      rejected: 'danger',
      executing: 'info',
      completed: 'success',
      failed: 'danger',
      expired: 'basic',
    };
    return badges[status] || 'basic';
  }
}
