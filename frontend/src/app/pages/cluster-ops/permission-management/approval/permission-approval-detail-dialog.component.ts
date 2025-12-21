import { Component, Input } from '@angular/core';
import { NbDialogRef } from '@nebular/theme';
import { PermissionRequestResponse } from '../../../../@core/data/permission-request.model';

/**
 * Permission Approval Detail Dialog Component
 * Using ngx-admin native NbDialogService for proper modal implementation
 */
@Component({
  selector: 'ngx-permission-approval-detail-dialog',
  template: `
    <nb-card class="permission-detail-card">
      <nb-card-header>
        <div class="d-flex justify-content-between align-items-center">
          <h5 class="mb-0">申请详情 #{{ request.id }}</h5>
          <button nbButton ghost status="basic" size="tiny" (click)="close()">
            <nb-icon icon="close-outline"></nb-icon>
          </button>
        </div>
      </nb-card-header>

      <nb-card-body>
        <!-- Basic Information -->
        <div class="detail-section">
          <h6 class="section-title">基本信息</h6>
          <div class="info-grid">
            <div class="info-item">
              <label>申请人</label>
              <span>{{ request.applicant_name || '-' }}</span>
            </div>
            <div class="info-item">
              <label>申请类型</label>
              <span>
                <nb-badge [status]="getRequestTypeStatus(request.request_type)"
                         [text]="getRequestTypeLabel(request.request_type)">
                </nb-badge>
              </span>
            </div>
            <div class="info-item">
              <label>状态</label>
              <span>
                <nb-badge [status]="getStatusBadge(request.status)"
                         [text]="getStatusLabel(request.status)">
                </nb-badge>
              </span>
            </div>
            <div class="info-item">
              <label>集群</label>
              <span>{{ request.cluster_name || '-' }}</span>
            </div>
            <div class="info-item">
              <label>创建时间</label>
              <span>{{ request.created_at | date: 'yyyy-MM-dd HH:mm:ss' }}</span>
            </div>
            <div class="info-item" *ngIf="request.valid_until">
              <label>有效期至</label>
              <span>{{ request.valid_until | date: 'yyyy-MM-dd HH:mm:ss' }}</span>
            </div>
          </div>
        </div>

        <!-- Grant Details -->
        <div class="detail-section" *ngIf="request.request_details">
          <h6 class="section-title">授权详情</h6>
          <div class="info-grid">
            <div class="info-item">
              <label>目标用户</label>
              <span>{{ request.request_details.target_user || '-' }}</span>
            </div>
            <div class="info-item" *ngIf="request.request_details.target_role">
              <label>角色</label>
              <span>{{ request.request_details.target_role }}</span>
            </div>
            <div class="info-item" *ngIf="request.request_details.permissions?.length">
              <label>权限</label>
              <div class="privilege-list">
                <nb-tag *ngFor="let priv of request.request_details.permissions"
                       status="info" appearance="outline">
                  {{ priv }}
                </nb-tag>
              </div>
            </div>
            <div class="info-item full-width" *ngIf="request.request_details.catalog || request.request_details.database || request.request_details.table">
              <label>资源信息</label>
              <div class="resource-info">
                <span *ngIf="request.request_details.catalog">
                  Catalog: {{ request.request_details.catalog }}
                </span>
                <span *ngIf="request.request_details.database">
                  Database: {{ request.request_details.database }}
                </span>
                <span *ngIf="request.request_details.table">
                  Table: {{ request.request_details.table }}
                </span>
              </div>
            </div>
          </div>
        </div>

        <!-- Reason -->
        <div class="detail-section" *ngIf="request.reason">
          <h6 class="section-title">申请理由</h6>
          <nb-alert status="info" appearance="outline">
            {{ request.reason }}
          </nb-alert>
        </div>

        <!-- Approval Info -->
        <div class="detail-section" *ngIf="request.status !== 'pending'">
          <h6 class="section-title">审批信息</h6>
          <div class="info-grid">
            <div class="info-item">
              <label>审批人</label>
              <span>{{ request.approver_name || '-' }}</span>
            </div>
            <div class="info-item">
              <label>审批时间</label>
              <span>{{ request.approved_at | date: 'yyyy-MM-dd HH:mm:ss' }}</span>
            </div>
            <div class="info-item full-width" *ngIf="request.approval_comment">
              <label>审批备注</label>
              <span>{{ request.approval_comment }}</span>
            </div>
          </div>
        </div>

        <!-- Execution Info -->
        <div class="detail-section" *ngIf="request.status === 'completed' || request.status === 'failed'">
          <h6 class="section-title">执行信息</h6>
          <div class="info-grid">
            <div class="info-item">
              <label>执行状态</label>
              <span>
                <nb-badge [status]="getExecutionStatusBadge(request.status)"
                         [text]="getStatusLabel(request.status)">
                </nb-badge>
              </span>
            </div>
            <div class="info-item" *ngIf="request.executed_at">
              <label>执行时间</label>
              <span>{{ request.executed_at | date: 'yyyy-MM-dd HH:mm:ss' }}</span>
            </div>
            <div class="info-item full-width" *ngIf="request.execution_result">
              <label>执行结果</label>
              <nb-alert [status]="request.status === 'completed' ? 'success' : 'danger'"
                       appearance="outline">
                {{ request.execution_result }}
              </nb-alert>
            </div>
          </div>
        </div>
      </nb-card-body>

      <nb-card-footer *ngIf="showActions">
        <div class="d-flex justify-content-end gap-2">
          <button nbButton status="success" (click)="approve()"
                  [disabled]="request.status !== 'pending'">
            <nb-icon icon="checkmark-outline"></nb-icon> 批准
          </button>
          <button nbButton status="danger" (click)="reject()"
                  [disabled]="request.status !== 'pending'">
            <nb-icon icon="close-outline"></nb-icon> 拒绝
          </button>
          <button nbButton status="basic" (click)="close()">
            关闭
          </button>
        </div>
      </nb-card-footer>
    </nb-card>
  `,
  styles: [`
    .permission-detail-card {
      min-width: 600px;
      max-width: 800px;
    }

    .detail-section {
      margin-bottom: 1.5rem;

      &:last-child {
        margin-bottom: 0;
      }
    }

    .section-title {
      color: var(--text-hint-color);
      margin-bottom: 1rem;
      font-weight: 600;
    }

    .info-grid {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 1rem;
    }

    .info-item {
      display: flex;
      flex-direction: column;
      gap: 0.25rem;

      &.full-width {
        grid-column: span 2;
      }

      label {
        font-size: 0.875rem;
        color: var(--text-hint-color);
      }

      span {
        color: var(--text-basic-color);
      }
    }

    .privilege-list {
      display: flex;
      flex-wrap: wrap;
      gap: 0.5rem;
      margin-top: 0.25rem;
    }

    .resource-info {
      display: flex;
      flex-direction: column;
      gap: 0.25rem;

      span {
        padding-left: 1rem;
      }
    }

    nb-card-footer {
      padding: 1rem;
      border-top: 1px solid var(--border-basic-color);
    }
  `],
})
export class PermissionApprovalDetailDialogComponent {
  @Input() request: PermissionRequestResponse;
  @Input() showActions: boolean = false;

  constructor(protected dialogRef: NbDialogRef<PermissionApprovalDetailDialogComponent>) {}

  close() {
    this.dialogRef.close();
  }

  approve() {
    this.dialogRef.close({ action: 'approve' });
  }

  reject() {
    this.dialogRef.close({ action: 'reject' });
  }

  getRequestTypeLabel(type: string): string {
    const labels = {
      grant_role: '授予角色',
      grant_privilege: '授予权限',
      revoke_role: '撤销角色',
      revoke_privilege: '撤销权限',
    };
    return labels[type] || type;
  }

  getRequestTypeStatus(type: string): string {
    return type?.startsWith('grant') ? 'success' : 'warning';
  }

  getStatusLabel(status: string): string {
    const labels = {
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
    const badges = {
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

  getExecutionStatusBadge(status: string): string {
    return status === 'completed' ? 'success' :
           status === 'failed' ? 'danger' :
           status === 'executing' ? 'info' : 'basic';
  }
}