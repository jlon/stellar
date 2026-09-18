import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, Input, Output, EventEmitter, inject } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { NbBadgeModule, NbButtonModule, NbCardModule, NbDialogService, NbIconModule, NbOptionModule, NbSelectModule, NbSpinnerModule, NbToastrService, NbTooltipModule } from '@nebular/theme';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { PermissionRequestResponse } from '../../../../@core/data/permission-request.model';
import { PermissionApprovalDetailDialogComponent } from './permission-approval-detail-dialog.component';
import { ConfirmationDialogComponent } from '../shared/confirmation-dialog.component';


@Component({
    selector: 'ngx-permission-approval',
    templateUrl: './permission-approval.component.html',
    styleUrls: ['./permission-approval.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbSelectModule,
    NbOptionModule,
    NbButtonModule,
    NbIconModule,
    NbBadgeModule,
    NbTooltipModule,
    NbSpinnerModule,
    Angular2SmartTableModule
],
})
export class PermissionApprovalComponent implements OnInit, OnDestroy {
  private permissionService = inject(PermissionRequestService)
  private i18n = inject(I18nService);
  private dialogService = inject(NbDialogService);
  private toastr = inject(NbToastrService);

  @Input() refresh$: Subject<void>;
  @Output() processed = new EventEmitter<void>();

  // State
  pendingRequests: PermissionRequestResponse[] = [];
  filteredRequests: PermissionRequestResponse[] = [];
  requestsLoading = false;
  typeFilter = 'all';

  // ng2-smart-table - 使用标准edit按钮查看详情（与dashboard、audit-logs保持一致）
  approvalSource: LocalDataSource = new LocalDataSource();
  tableSettings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: '暂无待审批申请',
    actions: {
      columnTitle: this.i18n.instant('操作'),
      add: false,
      edit: true,
      delete: false,
      position: 'right',
    },
    edit: {
      editButtonContent: '<i class="nb-search" title="查看"></i>',  // 使用搜索图标表示查看详情
    },
    pager: {
      display: true,
      perPage: 10,
    },
    columns: {
      id: {
        title: 'ID',
        type: 'number',
        width: '50px',
      },
      request_type: {
        title: this.i18n.instant('类型'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '90px',
        valuePrepareFunction: (value: string) => {
          const labels: { [key: string]: string } = {
            grant_role: '授予角色',
            grant_permission: '授予权限',
            revoke_permission: '撤销权限',
          };
          return `<span class="badge badge-primary">${labels[value] || value}</span>`;
        },
      },
      applicant_name: {
        title: this.i18n.instant('申请人'),
        type: 'string',
        width: '80px',
      },
      target: {
        title: this.i18n.instant('目标'),
        type: 'string',
      },
      reason: {
        title: this.i18n.instant('申请原因'),
        type: 'string',
      },
      created_at: {
        title: this.i18n.instant('申请时间'),
        type: 'string',
        width: '180px',
        valuePrepareFunction: (value: string) => {
          if (!value) return '-';
          return value.replace('T', ' ').substring(0, 19);
        },
      },
    },
  };

  typeOptions = [
    { label: '全部', value: 'all' },
    { label: '授予角色', value: 'grant_role' },
    { label: '授予权限', value: 'grant_permission' },
    { label: '撤销权限', value: 'revoke_permission' },
  ];

  selectedRequest: PermissionRequestResponse | null = null;
  approvalInProgress = false;

  private destroy$ = new Subject<void>();

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$.pipe(takeUntil(this.destroy$)).subscribe(() => {
        this.loadPendingRequests();
      });
    }
    this.loadPendingRequests();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadPendingRequests(): void {
    this.requestsLoading = true;
    this.permissionService.listPendingApprovals().subscribe({
      next: (requests) => {
        this.pendingRequests = requests;
        this.applyFilters();
        this.requestsLoading = false;
      },
      error: (err) => {
        console.error('Failed to load pending requests:', err);
        this.toastr.danger(this.i18n.instant('加载待审批申请失败'), this.i18n.instant('错误'));
        this.requestsLoading = false;
      },
    });
  }

  onTypeFilterChange(): void {
    this.applyFilters();
  }

  private applyFilters(): void {
    if (this.typeFilter === 'all') {
      this.filteredRequests = [...this.pendingRequests];
    } else {
      this.filteredRequests = this.pendingRequests.filter(
        (req) => req.request_type === this.typeFilter,
      );
    }

    const tableData = this.filteredRequests.map(req => ({
      id: req.id,
      request_type: req.request_type,
      applicant_name: req.applicant_name,
      target: this.getTargetDescription(req),
      reason: req.reason?.substring(0, 50) + (req.reason?.length > 50 ? '...' : ''),
      created_at: req.created_at,
    }));
    this.approvalSource.load(tableData);
  }

  // 使用edit事件处理查看详情
  onEditRow(event: any): void {
    const request = this.filteredRequests.find(r => r.id === event.data.id);
    if (request) {
      this.onViewDetail(request);
    }
  }

  onViewDetail(request: PermissionRequestResponse): void {
    const dialogRef = this.dialogService.open(PermissionApprovalDetailDialogComponent, {
      context: {
        request: request,
        showActions: true,
      },
      hasBackdrop: true,
      closeOnBackdropClick: false,
      closeOnEsc: true,
    });

    dialogRef.onClose.subscribe((result) => {
      if (result?.action === 'approve') {
        this.openDecisionDialog(request, true);
      } else if (result?.action === 'reject') {
        this.openDecisionDialog(request, false);
      }
    });
  }

  private openDecisionDialog(request: PermissionRequestResponse, approve: boolean): void {
    const action = approve ? '批准' : '拒绝';
    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: `${action}申请 #${request.id}`,
        message: approve
          ? `确定要批准 ${request.applicant_name} 的权限申请吗？`
          : `请说明拒绝 ${request.applicant_name} 权限申请的原因。`,
        confirmText: action,
        cancelText: '取消',
        confirmButtonStatus: approve ? 'success' : 'danger',
        confirmIcon: approve ? 'checkmark-outline' : 'close-outline',
        showCommentInput: true,
        commentLabel: approve ? '审批备注' : '拒绝原因',
        commentPlaceholder: approve ? '可选，说明审批结论' : '请说明拒绝原因',
        commentRequired: !approve,
        commentHint: approve ? '' : '拒绝申请时必须填写原因',
      },
      hasBackdrop: true,
      closeOnBackdropClick: false,
    });

    dialogRef.onClose.subscribe((result) => {
      if (!result?.confirmed) return;

      this.approvalInProgress = true;
      const request$ = approve
        ? this.permissionService.approveRequest(request.id, { comment: result.comment || '' })
        : this.permissionService.rejectRequest(request.id, { comment: result.comment });

      request$.subscribe({
        next: () => {
          this.toastr.success(`已${action}申请 #${request.id}`, `${action}成功`);
          this.approvalInProgress = false;
          this.processed.emit();
          this.loadPendingRequests();
        },
        error: (err) => {
          console.error(`Failed to ${approve ? 'approve' : 'reject'} request:`, err);
          this.toastr.danger(`${action}申请失败: ${err.error?.message || err.message}`, '错误');
          this.approvalInProgress = false;
        },
      });
    });
  }

  getTargetDescription(request: PermissionRequestResponse): string {
    const details = request.request_details;
    if (!details) return '-';

    if (request.request_type === 'grant_role') {
      return `${details.target_user || '-'} ← ${details.target_role || '-'}`;
    } else {
      const user = details.target_user || '-';
      const resourceType = details.resource_type?.toLowerCase();
      let scope = '';
      if (resourceType === 'catalog') {
        scope = details.catalog || '*';
      } else if (resourceType === 'database') {
        const catalog = details.catalog ? `${details.catalog}.` : '';
        scope = `${catalog}${details.database || '*'}.*`;
      } else if (resourceType === 'table') {
        const catalog = details.catalog ? `${details.catalog}.` : '';
        const db = details.database || '*';
        const table = details.table || '*';
        scope = `${catalog}${db}.${table}`;
      } else {
        scope = details.scope || details.database || details.catalog || '-';
      }
      const perms = details.permissions?.join(', ') || '';
      return perms ? `${user} @ ${scope} (${perms})` : `${user} @ ${scope}`;
    }
  }
}
