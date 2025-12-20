// @ts-nocheck
import { Component, OnInit, OnDestroy, Input, Output, EventEmitter } from '@angular/core';
import { Observable, Subject } from 'rxjs';
import { takeUntil, startWith } from 'rxjs/operators';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { PermissionRequestResponse } from '../../../../@core/data/permission-request.model';
import { ErrorHandler } from '../../../../@core/utils/error-handler';

/**
 * PendingApprovalsComponent
 * 待审批的权限申请列表组件
 *
 * 功能：
 * - 显示待审批的权限申请（组织隔离）
 * - 支持状态和类型过滤
 * - 支持批准或拒绝申请
 * - 显示申请详情和预览SQL
 */
@Component({
  selector: 'ngx-pending-approvals',
  templateUrl: './pending-approvals.component.html',
  styleUrls: ['./pending-approvals.component.scss'],
})
export class PendingApprovalsComponent implements OnInit, OnDestroy {
  @Input() refresh$: Observable<void>;
  @Output() processed = new EventEmitter<void>();

  source: LocalDataSource = new LocalDataSource();
  loading = true;
  error: string | null = null;
  isEmpty = false;
  filterStatus = '';
  filterType = '';

  private destroy$ = new Subject<void>();

  // 状态选项
  statusOptions = [
    { label: '全部', value: '' },
    { label: '待审批', value: 'pending' },
  ];

  // 申请类型选项
  typeOptions = [
    { label: '全部', value: '' },
    { label: '创建账户', value: 'create_account' },
    { label: '授予角色', value: 'grant_role' },
    { label: '授予权限', value: 'grant_permission' },
  ];

  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: '暂无待审批的申请',
    actions: {
      columnTitle: '操作',
      add: false,
      edit: false,
      delete: false,
      position: 'right',
    },
    columns: {
      id: {
        title: 'ID',
        type: 'number',
        width: '5%',
      },
      request_type: {
        title: '申请类型',
        type: 'string',
        width: '10%',
        valuePrepareFunction: (value: string) => {
          const typeMap = {
            create_account: '创建账户',
            grant_role: '授予角色',
            grant_permission: '授予权限',
          };
          return typeMap[value] || value;
        },
      },
      applicant_name: {
        title: '申请人',
        type: 'string',
        width: '10%',
      },
      reason: {
        title: '申请理由',
        type: 'string',
      },
      created_at: {
        title: '创建时间',
        type: 'string',
        valuePrepareFunction: (date: string) => {
          return new Date(date).toLocaleString('zh-CN');
        },
      },
    },
  };

  constructor(
    private permissionRequestService: PermissionRequestService,
    private toastrService: NbToastrService,
    private dialogService: NbDialogService,
  ) {}

  ngOnInit(): void {
    this.loadPendingApprovals();

    // 监听刷新事件
    if (this.refresh$) {
      this.refresh$
        .pipe(startWith(undefined), takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadPendingApprovals();
        });
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * 加载待审批的申请列表
   */
  loadPendingApprovals(): void {
    this.loading = true;
    this.error = null;
    this.isEmpty = false;

    const filter = {
      status: this.filterStatus || 'pending',
      request_type: this.filterType || undefined,
    };

    this.permissionRequestService.listPendingApprovals(filter)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (response: PermissionRequestResponse[]) => {
          // Check if data is empty
          if (!response || response.length === 0) {
            this.isEmpty = true;
            this.source.load([]);
          } else {
            this.isEmpty = false;
            this.source.load(response);
          }

          this.loading = false;
        },
        error: (error) => {
          this.loading = false;

          // Extract user-friendly error message
          if (error?.error?.message) {
            this.error = error.error.message;
          } else if (error?.message) {
            this.error = error.message;
          } else if (error?.status === 0) {
            this.error = '网络连接失败，请检查服务器是否正常运行';
          } else if (error?.status === 401) {
            this.error = '认证失败，请重新登录';
          } else if (error?.status === 403) {
            this.error = '您没有权限执行此操作';
          } else if (error?.status === 500) {
            this.error = '服务器错误，请稍后重试';
          } else {
            this.error = `加载失败: ${error?.statusText || '未知错误'}`;
          }

          // Also show error toast
          ErrorHandler.handleHttpError(error, this.toastrService);
        },
      });
  }

  /**
   * 过滤处理
   */
  onFilterChange(): void {
    this.loadPendingApprovals();
  }

  /**
   * 重试加载
   */
  retry(): void {
    this.error = null;
    this.isEmpty = false;
    this.loadPendingApprovals();
  }

  /**
   * 查看详情
   */
  viewDetails(request: PermissionRequestResponse): void {
    const content = `
      <div class="request-details">
        <p><strong>申请类型：</strong>${this.getTypeName(request.request_type)}</p>
        <p><strong>申请人：</strong>${request.applicant_name}</p>
        <p><strong>创建时间：</strong>${new Date(request.created_at).toLocaleString('zh-CN')}</p>
        <hr />
        <p><strong>申请理由：</strong></p>
        <p>${request.reason}</p>
        ${request.preview_sql ? `<p><strong>SQL 预览：</strong></p><pre><code>${request.preview_sql}</code></pre>` : ''}
      </div>
    `;

    this.dialogService.open(undefined, {
      title: `申请详情 (#${request.id})`,
      context: { content },
    });
  }

  /**
   * 批准申请
   */
  approveRequest(request: PermissionRequestResponse): void {
    this.dialogService.open(
      undefined,
      {
        title: '批准申请',
        context: {
          comment: '',
        },
      },
    ).subscribe((result) => {
      if (result) {
        this.permissionRequestService.approveRequest(request.id, { comment: result })
          .pipe(takeUntil(this.destroy$))
          .subscribe({
            next: () => {
              this.toastrService.success('申请已批准', '成功');
              this.processed.emit();
              this.loadPendingApprovals();
            },
            error: (error) => {
              ErrorHandler.handleHttpError(error, this.toastrService);
            },
          });
      }
    });
  }

  /**
   * 拒绝申请
   */
  rejectRequest(request: PermissionRequestResponse): void {
    this.dialogService.open(
      undefined,
      {
        title: '拒绝申请',
        context: {
          comment: '',
        },
      },
    ).subscribe((result) => {
      if (result) {
        this.permissionRequestService.rejectRequest(request.id, { comment: result })
          .pipe(takeUntil(this.destroy$))
          .subscribe({
            next: () => {
              this.toastrService.success('申请已拒绝', '成功');
              this.processed.emit();
              this.loadPendingApprovals();
            },
            error: (error) => {
              ErrorHandler.handleHttpError(error, this.toastrService);
            },
          });
      }
    });
  }

  /**
   * 获取类型显示名称
   */
  private getTypeName(type: string): string {
    const typeMap = {
      create_account: '创建账户',
      grant_role: '授予角色',
      grant_permission: '授予权限',
    };
    return typeMap[type] || type;
  }
}
