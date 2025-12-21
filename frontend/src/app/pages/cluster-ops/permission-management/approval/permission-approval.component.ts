import {
  Component,
  OnInit,
  OnDestroy,
  Input,
  Output,
  EventEmitter,
} from '@angular/core';
import { FormBuilder, FormGroup, Validators } from '@angular/forms';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { PermissionRequestResponse } from '../../../../@core/data/permission-request.model';
import { PermissionApprovalDetailDialogComponent } from './permission-approval-detail-dialog.component';
import { ConfirmationDialogComponent } from '../shared/confirmation-dialog.component';

/**
 * PermissionApprovalComponent
 * Tab 3: 权限审批 (Permission Approval)
 *
 * Purpose:
 * - Review pending permission requests
 * - Show request details with SQL preview
 * - Approve or reject requests with optional comments
 * - Track approval workflow
 *
 * Features:
 * - Pending requests list with pagination
 * - Detail modal dialog with full request information
 * - Approval/rejection with comment
 * - Status tracking and timestamps
 * - Automatic list refresh after action
 */
@Component({
  selector: 'ngx-permission-approval',
  templateUrl: './permission-approval.component.html',
  styleUrls: ['./permission-approval.component.scss'],
})
export class PermissionApprovalComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;
  @Output() processed = new EventEmitter<void>();

  // State
  pendingRequests: PermissionRequestResponse[] = [];
  filteredRequests: PermissionRequestResponse[] = [];
  requestsLoading = false;
  typeFilter = 'all';

  // Request type options
  typeOptions = [
    { label: '全部', value: 'all' },
    { label: '授予角色', value: 'grant_role' },
    { label: '授予权限', value: 'grant_permission' },
    { label: '撤销权限', value: 'revoke_permission' },
  ];

  // Modal/Dialog state
  selectedRequest: PermissionRequestResponse | null = null;
  approvalForm: FormGroup;
  approvalInProgress = false;

  private destroy$ = new Subject<void>();

  constructor(
    private fb: FormBuilder,
    private permissionService: PermissionRequestService,
    private dialogService: NbDialogService,
    private toastr: NbToastrService,
  ) {
    this.approvalForm = this.fb.group({
      comment: [''],
    });
  }

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$
        .pipe(takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadPendingRequests();
        });
    }
    this.loadPendingRequests();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load pending approval requests (only 'pending' status)
   */
  private loadPendingRequests(): void {
    this.requestsLoading = true;

    this.permissionService.listPendingApprovals().subscribe({
      next: (requests) => {
        this.pendingRequests = requests;
        this.applyFilters();
        this.requestsLoading = false;
      },
      error: (err) => {
        console.error('Failed to load pending requests:', err);
        this.toastr.danger('加载待审批申请失败', '错误');
        this.requestsLoading = false;
      },
    });
  }

  /**
   * Apply type filter to pending requests
   */
  onTypeFilterChange(): void {
    this.applyFilters();
  }

  /**
   * Apply filters to requests list
   */
  private applyFilters(): void {
    if (this.typeFilter === 'all') {
      this.filteredRequests = [...this.pendingRequests];
    } else {
      this.filteredRequests = this.pendingRequests.filter(
        (req) => req.request_type === this.typeFilter,
      );
    }
  }

  /**
   * Open detail modal for a request using ngx-admin native dialog service
   */
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
      if (result) {
        if (result.action === 'approve') {
          this.performApproval(request);
        } else if (result.action === 'reject') {
          this.performRejection(request);
        }
      }
    });
  }

  /**
   * Perform approval action with confirmation dialog
   */
  private performApproval(request: PermissionRequestResponse): void {
    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: `批准申请 #${request.id}`,
        message: `确定要批准 ${request.applicant_name} 的权限申请吗？`,
        confirmText: '批准',
        cancelText: '取消',
        confirmButtonStatus: 'success',
        confirmIcon: 'checkmark-outline',
        showCommentInput: true,
        commentLabel: '审批备注',
        commentPlaceholder: '请输入审批备注（可选）...',
        commentRequired: false,
      },
      hasBackdrop: true,
      closeOnBackdropClick: false,
    });

    dialogRef.onClose.subscribe((result) => {
      if (result && result.confirmed) {
        this.approvalInProgress = true;
        this.permissionService.approveRequest(request.id, { comment: result.comment || '' }).subscribe({
          next: () => {
            this.toastr.success(`已批准申请 #${request.id}`, '批准成功');
            this.approvalInProgress = false;
            this.processed.emit();
            this.loadPendingRequests();
          },
          error: (err) => {
            console.error('Failed to approve request:', err);
            this.toastr.danger('批准申请失败: ' + (err.error?.message || err.message), '错误');
            this.approvalInProgress = false;
          },
        });
      }
    });
  }

  /**
   * Perform rejection action with confirmation dialog
   */
  private performRejection(request: PermissionRequestResponse): void {
    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: `拒绝申请 #${request.id}`,
        message: `请说明拒绝 ${request.applicant_name} 权限申请的原因。`,
        confirmText: '拒绝',
        cancelText: '取消',
        confirmButtonStatus: 'danger',
        confirmIcon: 'close-outline',
        showCommentInput: true,
        commentLabel: '拒绝原因',
        commentPlaceholder: '请输入拒绝原因...',
        commentRequired: true,
        commentHint: '拒绝申请时必须填写原因',
      },
      hasBackdrop: true,
      closeOnBackdropClick: false,
    });

    dialogRef.onClose.subscribe((result) => {
      if (result && result.confirmed && result.comment) {
        this.approvalInProgress = true;
        this.permissionService.rejectRequest(request.id, { comment: result.comment }).subscribe({
          next: () => {
            this.toastr.success(`已拒绝申请 #${request.id}`, '拒绝成功');
            this.approvalInProgress = false;
            this.processed.emit();
            this.loadPendingRequests();
          },
          error: (err) => {
            console.error('Failed to reject request:', err);
            this.toastr.danger('拒绝申请失败: ' + (err.error?.message || err.message), '错误');
            this.approvalInProgress = false;
          },
        });
      }
    });
  }

  /**
   * Close detail modal - deprecated method kept for compatibility
   */
  onCloseDetail(): void {
    // No longer needed with dialog service
    this.selectedRequest = null;
    this.approvalForm.reset();
  }

  /**
   * Get request type label in Chinese
   */
  getRequestTypeLabel(requestType: string): string {
    const typeMap: { [key: string]: string } = {
      grant_role: '授予角色',
      grant_permission: '授予权限',
      revoke_permission: '撤销权限',
    };
    return typeMap[requestType] || requestType;
  }

  /**
   * Get target description for the request
   */
  getTargetDescription(request: PermissionRequestResponse): string {
    const details = request.request_details;
    if (request.request_type === 'grant_role') {
      return `${details?.target_user} ← ${details?.target_role}`;
    } else {
      const scope = details?.scope || details?.database;
      return `${details?.target_user} @ ${scope}`;
    }
  }
}
