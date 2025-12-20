// @ts-nocheck
import { Component, OnInit, OnDestroy, Input, Output, EventEmitter } from '@angular/core';
import { Observable, Subject } from 'rxjs';
import { takeUntil, startWith } from 'rxjs/operators';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import {
  PermissionRequestResponse,
  PaginatedResponse,
} from '../../../../@core/data/permission-request.model';
import { ErrorHandler } from '../../../../@core/utils/error-handler';

/**
 * MyRequestsComponent
 * 我的权限申请列表组件
 *
 * 功能：
 * - 显示当前用户提交的权限申请
 * - 支持状态和类型过滤
 * - 支持申请取消（仅在pending状态）
 * - 显示申请详情和预览SQL
 */
@Component({
  selector: 'ngx-my-requests',
  templateUrl: './my-requests.component.html',
  styleUrls: ['./my-requests.component.scss'],
})
export class MyRequestsComponent implements OnInit, OnDestroy {
  @Input() refresh$: Observable<void>;
  @Output() submitted = new EventEmitter<void>();

  source: LocalDataSource = new LocalDataSource();
  loading = true;
  error: string | null = null;
  isEmpty = false;
  isRefreshing = false;  // Distinguish between initial load and refresh
  filterStatus = '';
  filterType = '';
  currentPage = 1;
  pageSize = 10;
  totalItems = 0;

  private destroy$ = new Subject<void>();

  // 状态选项
  statusOptions = [
    { label: '全部', value: '' },
    { label: '待审批', value: 'pending' },
    { label: '已批准', value: 'approved' },
    { label: '已拒绝', value: 'rejected' },
    { label: '已完成', value: 'completed' },
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
    noDataMessage: '暂无申请记录',
    actions: {
      columnTitle: '操作',
      add: false,
      edit: false,
      delete: false,
      position: 'right',
    },
    pager: {
      display: true,
      perPage: this.pageSize,
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
      status: {
        title: '状态',
        type: 'string',
        width: '10%',
        valuePrepareFunction: (value: string) => {
          const statusMap = {
            pending: '待审批',
            approved: '已批准',
            rejected: '已拒绝',
            completed: '已完成',
          };
          return statusMap[value] || value;
        },
      },
      approver_name: {
        title: '审批人',
        type: 'string',
        width: '10%',
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
    this.loadRequests();

    // 监听刷新事件
    if (this.refresh$) {
      this.refresh$
        .pipe(startWith(undefined), takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadRequests();
        });
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * 加载我的申请列表
   */
  loadRequests(): void {
    const wasLoading = this.loading;
    this.loading = true;
    this.error = null;
    this.isEmpty = false;

    const filter = {
      status: this.filterStatus || undefined,
      request_type: this.filterType || undefined,
      page: this.currentPage,
      page_size: this.pageSize,
    };

    this.permissionRequestService.listMyRequests(filter)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (response: PaginatedResponse<PermissionRequestResponse>) => {
          this.totalItems = response.total;

          // Check if data is empty
          if (!response.data || response.data.length === 0) {
            this.isEmpty = true;
            this.source.load([]);
          } else {
            this.isEmpty = false;
            this.source.load(response.data);
          }

          this.loading = false;
          this.isRefreshing = false;

          // Show success toast only on initial load (not on refresh)
          if (wasLoading) {
            this.toastrService.success(`成功加载 ${response.data.length} 条申请`, '成功');
          }
        },
        error: (error) => {
          this.loading = false;
          this.isRefreshing = false;

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
    this.currentPage = 1;
    this.loadRequests();
  }

  /**
   * 重试加载
   */
  retry(): void {
    this.error = null;
    this.isEmpty = false;
    this.loadRequests();
  }

  /**
   * 查看详情
   */
  viewDetails(request: PermissionRequestResponse): void {
    const content = `
      <div class="request-details">
        <p><strong>申请类型：</strong>${this.getTypeName(request.request_type)}</p>
        <p><strong>申请人：</strong>${request.applicant_name}</p>
        <p><strong>状态：</strong>${this.getStatusName(request.status)}</p>
        <p><strong>创建时间：</strong>${new Date(request.created_at).toLocaleString('zh-CN')}</p>
        ${request.approver_name ? `<p><strong>审批人：</strong>${request.approver_name}</p>` : ''}
        ${request.approved_at ? `<p><strong>审批时间：</strong>${new Date(request.approved_at).toLocaleString('zh-CN')}</p>` : ''}
        ${request.approval_comment ? `<p><strong>审批意见：</strong>${request.approval_comment}</p>` : ''}
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
   * 取消申请（仅pending状态）
   */
  cancelRequest(request: PermissionRequestResponse): void {
    if (request.status !== 'pending') {
      this.toastrService.warning('只能取消待审批的申请', '提示');
      return;
    }

    this.dialogService.yesNo('确认取消申请吗？', '此操作无法撤销')
      .subscribe((result) => {
        if (result) {
          this.permissionRequestService.cancelRequest(request.id)
            .pipe(takeUntil(this.destroy$))
            .subscribe({
              next: () => {
                this.toastrService.success('申请已取消', '成功');
                this.loadRequests();
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

  /**
   * 获取状态显示名称
   */
  private getStatusName(status: string): string {
    const statusMap = {
      pending: '待审批',
      approved: '已批准',
      rejected: '已拒绝',
      completed: '已完成',
    };
    return statusMap[status] || status;
  }
}
