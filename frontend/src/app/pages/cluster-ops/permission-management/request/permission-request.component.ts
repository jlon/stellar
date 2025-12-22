import {
  Component,
  OnInit,
  OnDestroy,
  Input,
  Output,
  EventEmitter,
} from '@angular/core';
import { FormBuilder, FormGroup, Validators } from '@angular/forms';
import { Subject, forkJoin } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { PermissionRequestResponse, SubmitRequestDto, DbAccountDto, DbRoleDto } from '../../../../@core/data/permission-request.model';
import { NodeService } from '../../../../@core/data/node.service';
import { NbToastrService } from '@nebular/theme';

/**
 * PermissionRequestComponent
 * Tab 2: 权限申请 (Permission Request)
 *
 * Purpose:
 * - Submit new permission requests (grant_role, grant_permission, revoke_permission)
 * - Display list of user's requests with status
 *
 * Form Types:
 * 1. grant_role: User → Role assignment
 * 2. grant_permission: User + Resource → Permission assignment
 * 3. revoke_permission: User + Resource → Permission revocation
 *
 * Features:
 * - Dynamic form based on request type
 * - Real-time data loading from backend
 * - Request history with filtering
 * - Status tracking (pending, approved, executing, completed, rejected, failed)
 * - Cascade selection: Catalog → Database → Table
 */
@Component({
  selector: 'ngx-permission-request',
  templateUrl: './permission-request.component.html',
  styleUrls: ['./permission-request.component.scss'],
})
export class PermissionRequestComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;
  @Output() submitted = new EventEmitter<void>();

  // Form state
  requestForm: FormGroup;
  showForm = true;
  formSubmitting = false;

  // Inline create modes for user/role
  isCreatingUser = false;
  isCreatingRole = false;

  // Request types
  requestTypes = [
    { label: '授予角色', value: 'grant_role' },
    { label: '授予权限', value: 'grant_permission' },
    { label: '撤销权限', value: 'revoke_permission' },
  ];

  // Resource types
  resourceTypes = [
    { label: 'Catalog', value: 'catalog' },
    { label: 'Database', value: 'database' },
    { label: 'Table', value: 'table' },
  ];

  // Real data from API
  catalogs: string[] = [];
  databases: string[] = [];
  tables: string[] = [];
  dbAccounts: string[] = []; // Database accounts list
  dbRoles: string[] = []; // Database roles list

  // Loading states
  loadingAccounts = false;
  loadingRoles = false;
  loadingCatalogs = false;
  loadingDatabases = false;
  loadingTables = false;

  // StarRocks permission options (from official docs)
  // Catalog-level permissions
  catalogPermissions = [
    { label: 'USAGE', value: 'USAGE' },
    { label: 'CREATE DATABASE', value: 'CREATE DATABASE' },
    { label: 'DROP', value: 'DROP' },
    { label: 'ALL', value: 'ALL' },
  ];

  // Database-level permissions
  databasePermissions = [
    { label: 'ALTER', value: 'ALTER' },
    { label: 'DROP', value: 'DROP' },
    { label: 'CREATE TABLE', value: 'CREATE TABLE' },
    { label: 'CREATE VIEW', value: 'CREATE VIEW' },
    { label: 'CREATE FUNCTION', value: 'CREATE FUNCTION' },
    { label: 'CREATE MATERIALIZED VIEW', value: 'CREATE MATERIALIZED VIEW' },
    { label: 'ALL', value: 'ALL' },
  ];

  // Table-level permissions
  tablePermissions = [
    { label: 'SELECT', value: 'SELECT' },
    { label: 'INSERT', value: 'INSERT' },
    { label: 'UPDATE', value: 'UPDATE' },
    { label: 'DELETE', value: 'DELETE' },
    { label: 'ALTER', value: 'ALTER' },
    { label: 'DROP', value: 'DROP' },
    { label: 'EXPORT', value: 'EXPORT' },
    { label: 'ALL', value: 'ALL' },
  ];

  // Get available permissions based on current resource type
  get availablePermissions() {
    const resourceType = this.currentResourceType;
    if (resourceType === 'catalog') {
      return this.catalogPermissions;
    } else if (resourceType === 'database') {
      return this.databasePermissions;
    } else if (resourceType === 'table') {
      return this.tablePermissions;
    }
    return this.tablePermissions; // Default to table permissions
  }

  // Request list
  myRequests: PermissionRequestResponse[] = [];
  filteredRequests: PermissionRequestResponse[] = [];
  requestsLoading = false;
  statusFilter = 'all';
  requestSource: LocalDataSource = new LocalDataSource();

  // ng2-smart-table settings
  tableSettings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: '暂无申请记录',
    actions: {
      add: false,
      edit: false,
      delete: false,
    },
    pager: {
      display: true,
      perPage: 10,
    },
    columns: {
      id: {
        title: 'ID',
        type: 'number',
        width: '60px',
      },
      request_type: {
        title: '类型',
        type: 'html',
        width: '100px',
        valuePrepareFunction: (value: string) => {
          const labels: {[key: string]: string} = {
            'grant_role': '授予角色',
            'grant_permission': '授予权限',
            'revoke_permission': '撤销权限',
          };
          return `<span class="badge badge-primary">${labels[value] || value}</span>`;
        },
      },
      target: {
        title: '目标',
        type: 'string',
      },
      permissions: {
        title: '权限',
        type: 'string',
      },
      reason: {
        title: '原因',
        type: 'string',
      },
      status: {
        title: '状态',
        type: 'html',
        width: '90px',
        valuePrepareFunction: (value: string) => {
          const statusMap: {[key: string]: {label: string, badge: string}} = {
            'pending': { label: '待审批', badge: 'warning' },
            'approved': { label: '已批准', badge: 'info' },
            'executing': { label: '执行中', badge: 'primary' },
            'completed': { label: '已完成', badge: 'success' },
            'rejected': { label: '已拒绝', badge: 'danger' },
            'failed': { label: '失败', badge: 'danger' },
          };
          const s = statusMap[value] || { label: value, badge: 'default' };
          return `<span class="badge badge-${s.badge}">${s.label}</span>`;
        },
      },
      created_at: {
        title: '创建时间',
        type: 'string',
        width: '150px',
        valuePrepareFunction: (value: string) => {
          if (!value) return '-';
          // 格式化 ISO 日期字符串为 YYYY-MM-DD HH:mm:ss
          return value.replace('T', ' ').substring(0, 19);
        },
      },
    },
  };

  // Status options
  statusOptions = [
    { label: '全部', value: 'all' },
    { label: '待审批', value: 'pending' },
    { label: '已批准', value: 'approved' },
    { label: '执行中', value: 'executing' },
    { label: '已完成', value: 'completed' },
    { label: '已拒绝', value: 'rejected' },
    { label: '失败', value: 'failed' },
  ];

  private destroy$ = new Subject<void>();

  constructor(
    private fb: FormBuilder,
    private permissionService: PermissionRequestService,
    private nodeService: NodeService,
    private toastr: NbToastrService,
  ) {
    this.initForm();
  }

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$
        .pipe(takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadMyRequests();
        });
    }

    // Load initial data
    this.loadInitialData();
    this.loadMyRequests();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Initialize form with default values and watchers
   */
  private initForm(): void {
    this.requestForm = this.fb.group({
      request_type: ['grant_permission', Validators.required],
      target_user: ['', Validators.required],
      target_role: [''], // For grant_role or role-based operations
      resource_type: [''], // For grant_permission, revoke_permission
      catalog: [''], // For catalog/database/table level permissions
      database: [''],
      table: [''], // Added: Table selector
      permissions: [[]], // Array of permission values (e.g., ['SELECT', 'INSERT'])
      reason: ['', Validators.required],
      // Inline create fields
      new_user_name: [''],
      new_user_password: [''],
      new_role_name: [''],
    });

    // Setup cascade watchers
    this.setupCascadeWatchers();
  }

  /**
   * Setup cascade watchers for resource selection
   */
  private setupCascadeWatchers(): void {
    // When catalog changes, reload databases
    this.requestForm.get('catalog')?.valueChanges
      .pipe(takeUntil(this.destroy$))
      .subscribe((catalog) => {
        if (catalog) {
          this.loadDatabases(catalog);
        } else {
          this.databases = [];
        }
        // Clear downstream selections
        this.requestForm.patchValue({ database: '', table: '' }, { emitEvent: false });
        this.tables = [];
      });

    // When database changes, reload tables
    this.requestForm.get('database')?.valueChanges
      .pipe(takeUntil(this.destroy$))
      .subscribe((database) => {
        if (database) {
          const catalog = this.requestForm.get('catalog')?.value;
          this.loadTables(catalog, database);
        } else {
          this.tables = [];
        }
        // Clear downstream selections
        this.requestForm.patchValue({ table: '' }, { emitEvent: false });
      });
  }


  /**
   * Load initial data (catalogs, accounts, roles)
   */
  private loadInitialData(): void {
    this.loadingCatalogs = true;
    this.loadingAccounts = true;
    this.loadingRoles = true;

    forkJoin({
      catalogs: this.nodeService.getCatalogs(),
      accounts: this.permissionService.listDbAccounts(),
      roles: this.permissionService.listDbRoles(),
    }).subscribe({
      next: (result) => {
        this.catalogs = result.catalogs;
        this.dbAccounts = result.accounts.map(acc => acc.account_name || '');
        this.dbRoles = result.roles.map(role => role.role_name || '');
        this.loadingCatalogs = false;
        this.loadingAccounts = false;
        this.loadingRoles = false;
      },
      error: (err) => {
        console.error('Failed to load initial data:', err);
        this.toastr.danger('加载基础数据失败', '错误');
        this.loadingCatalogs = false;
        this.loadingAccounts = false;
        this.loadingRoles = false;
      },
    });
  }

  /**
   * Load databases for selected catalog
   */
  private loadDatabases(catalog: string): void {
    this.loadingDatabases = true;
    this.nodeService.getDatabases(catalog).subscribe({
      next: (dbs) => {
        this.databases = dbs;
        this.loadingDatabases = false;
      },
      error: (err) => {
        console.error('Failed to load databases:', err);
        this.toastr.danger('加载数据库列表失败', '错误');
        this.loadingDatabases = false;
      },
    });
  }

  /**
   * Load tables for selected database
   */
  private loadTables(catalog: string, database: string): void {
    this.loadingTables = true;
    this.nodeService.getTables(catalog, database).subscribe({
      next: (tables) => {
        this.tables = tables.map(t => t.name);
        this.loadingTables = false;
      },
      error: (err) => {
        console.error('Failed to load tables:', err);
        this.toastr.danger('加载表列表失败', '错误');
        this.loadingTables = false;
      },
    });
  }

  /**
   * Get the current request type
   */
  get currentRequestType(): string {
    return this.requestForm.get('request_type')?.value || 'grant_permission';
  }

  /**
   * Check if this is grant_role type
   */
  get isGrantRole(): boolean {
    return this.currentRequestType === 'grant_role';
  }

  /**
   * Check if this is grant/revoke_permission type
   */
  get isPermissionType(): boolean {
    return this.currentRequestType === 'grant_permission' || this.currentRequestType === 'revoke_permission';
  }

  /**
   * Get current resource type
   */
  get currentResourceType(): string {
    return this.requestForm.get('resource_type')?.value || 'database';
  }

  /**
   * Handle request type change
   */
  onRequestTypeChange(): void {
    // Clear form based on type
    if (this.isGrantRole) {
      this.requestForm.patchValue({
        target_role: '',
        resource_type: '',
        catalog: '',
        database: '',
        table: '',
        permissions: [],
      });
    }
  }

  onTargetUserChange(value: string): void {
    if (value === '__CREATE_NEW_USER__') {
      this.isCreatingUser = true;
      this.requestForm.patchValue({
        target_user: '',
        new_user_name: '',
        new_user_password: '',
      });
    } else {
      this.isCreatingUser = false;
      this.requestForm.patchValue({
        target_user: value,
        new_user_name: '',
        new_user_password: '',
      });
    }
  }

  onTargetRoleChange(value: string): void {
    if (value === '__CREATE_NEW_ROLE__') {
      this.isCreatingRole = true;
      this.requestForm.patchValue({
        target_role: '',
        new_role_name: '',
      });
    } else {
      this.isCreatingRole = false;
      this.requestForm.patchValue({
        target_role: value,
        new_role_name: '',
      });
    }
  }

  /**
   * Toggle permission selection
   */
  togglePermission(permission: string): void {
    const permissions = this.requestForm.get('permissions')?.value || [];
    const index = permissions.indexOf(permission);

    if (index > -1) {
      permissions.splice(index, 1);
    } else {
      permissions.push(permission);
    }

    this.requestForm.patchValue({ permissions });
  }

  /**
   * Check if permission is selected
   */
  isPermissionSelected(permission: string): boolean {
    const permissions = this.requestForm.get('permissions')?.value || [];
    return permissions.includes(permission);
  }

  /**
   * Submit permission request
   */
  onSubmitRequest(): void {
    if (!this.requestForm.valid) {
      this.toastr.warning('请填写必要字段', '验证失败');
      return;
    }

    this.formSubmitting = true;
    const formValue = this.requestForm.value;

    // Build final reason with inline new user/role information
    let reason: string = formValue.reason || '';
    const extraInfo: string[] = [];

    if (formValue.new_user_name) {
      extraInfo.push(`新建数据库账户: ${formValue.new_user_name}`);
    }
    if (formValue.new_user_password) {
      extraInfo.push(`账户初始密码: ${formValue.new_user_password}`);
    }
    if (formValue.new_role_name) {
      extraInfo.push(`新建数据库角色: ${formValue.new_role_name}`);
    }

    if (extraInfo.length > 0) {
      const extraBlock = `[系统自动补充信息]\n${extraInfo.join('\n')}`;
      reason = reason ? `${reason}\n\n${extraBlock}` : extraBlock;
    }

    // Build request DTO
    const dto: SubmitRequestDto = {
      cluster_id: 1, // TODO: Get from active cluster context
      request_type: formValue.request_type,
      request_details: this.buildRequestDetails(formValue),
      reason,
    };

    this.permissionService.submitRequest(dto).subscribe({
      next: (requestId) => {
        this.toastr.success(`权限申请提交成功 (ID: ${requestId})`, '提交成功');
        this.requestForm.reset({ request_type: 'grant_permission' });
        this.formSubmitting = false;
        this.submitted.emit();
        this.loadMyRequests();
      },
      error: (err) => {
        console.error('Failed to submit request:', err);
        this.toastr.danger('提交申请失败: ' + (err.error?.message || err.message), '错误');
        this.formSubmitting = false;
      },
    });
  }

  /**
   * Build request details based on form type
   */
  private buildRequestDetails(formValue: any): any {
    const details: any = {
      action: formValue.request_type,
      target_user: formValue.target_user,
    };

    if (formValue.request_type === 'grant_role') {
      details.target_role = formValue.target_role;
    } else {
      // grant_permission or revoke_permission
      details.resource_type = formValue.resource_type;

      if (formValue.catalog) {
        details.catalog = formValue.catalog;
      }
      if (formValue.database) {
        details.database = formValue.database;
      }
      if (formValue.table) {
        details.table = formValue.table;
      }
      details.permissions = formValue.permissions;
      if (formValue.target_role) {
        details.target_role = formValue.target_role;
      }
    }

    // Attach inline new user/role information for backend auto-provisioning
    if (formValue.new_user_name) {
      details.new_user_name = formValue.new_user_name;
    }
    if (formValue.new_user_password) {
      details.new_user_password = formValue.new_user_password;
    }
    if (formValue.new_role_name) {
      details.new_role_name = formValue.new_role_name;
    }

    return details;
  }

  /**
   * Load user's permission requests
   */
  loadMyRequests(): void {
    this.requestsLoading = true;

    this.permissionService.listMyRequests({ page: 1, page_size: 100 }).subscribe({
      next: (response) => {
        this.myRequests = response.data;
        this.applyFilters();
        this.requestsLoading = false;
      },
      error: (err) => {
        console.error('Failed to load requests:', err);
        this.toastr.danger('加载申请列表失败', '错误');
        this.requestsLoading = false;
      },
    });
  }

  /**
   * Apply status filter to requests
   */
  onStatusFilterChange(): void {
    this.applyFilters();
  }

  /**
   * Apply filters to requests list
   */
  private applyFilters(): void {
    if (this.statusFilter === 'all') {
      this.filteredRequests = [...this.myRequests];
    } else {
      this.filteredRequests = this.myRequests.filter(
        (req) => req.status === this.statusFilter,
      );
    }
    
    // Transform data for ng2-smart-table
    const tableData = this.filteredRequests.map(req => ({
      id: req.id,
      request_type: req.request_type,
      target: this.getRequestTarget(req),
      permissions: this.getRequestPermissions(req),
      reason: req.reason?.substring(0, 50) + (req.reason?.length > 50 ? '...' : ''),
      status: req.status,
      created_at: req.created_at,
    }));
    this.requestSource.load(tableData);
  }

  /**
   * Get badge color for status
   */
  getStatusBadgeType(status: string): string {
    switch (status) {
      case 'pending':
        return 'warning';
      case 'approved':
        return 'info';
      case 'executing':
        return 'primary';
      case 'completed':
        return 'success';
      case 'rejected':
        return 'danger';
      case 'failed':
        return 'danger';
      default:
        return 'default';
    }
  }

  /**
   * Get status label
   */
  getStatusLabel(status: string): string {
    const statusMap: { [key: string]: string } = {
      pending: '待审批',
      approved: '已批准',
      executing: '执行中',
      completed: '已完成',
      rejected: '已拒绝',
      failed: '失败',
    };
    return statusMap[status] || status;
  }

  /**
   * Format request target for display in table
   */
  getRequestTarget(req: PermissionRequestResponse): string {
    if (req.request_type === 'grant_role') {
      return `${req.request_details.target_user} → ${req.request_details.target_role}`;
    } else {
      const details = req.request_details;
      let target = details.target_user || '';
      if (details.resource_type) {
        target += ` (${details.resource_type})`;
      }
      if (details.database) {
        target += ` ${details.database}`;
      }
      if (details.table) {
        target += `.${details.table}`;
      }
      return target;
    }
  }

  /**
   * Format request permissions for display
   */
  getRequestPermissions(req: PermissionRequestResponse): string {
    if (req.request_details.permissions && req.request_details.permissions.length > 0) {
      return req.request_details.permissions.join(', ');
    }
    return '-';
  }

  /**
   * Prefill form for revoke request (called from dashboard)
   */
  prefillRevokeRequest(permission: any): void {
    // Set request type to revoke_permission first
    this.requestForm.patchValue({
      request_type: 'revoke_permission',
      permissions: [permission.privilege_type],
      reason: `申请撤销权限: ${permission.privilege_type} on ${permission.resource_path}`,
    }, { emitEvent: false });

    // Set resource type without triggering cascade
    const resourceType = this.mapResourceType(permission.resource_type);
    this.requestForm.patchValue({
      resource_type: resourceType,
    }, { emitEvent: false });

    // If granted through a role, set target_role
    if (permission.granted_role) {
      this.requestForm.patchValue({
        target_role: permission.granted_role,
      }, { emitEvent: false });
    }

    // Note: Don't auto-fill catalog/database/table as it triggers API calls
    // User should select them manually

    this.toastr.info('已预填撤销信息，请选择用户并补充申请原因', '预填完成');
  }

  /**
   * Map backend resource type to form resource type
   */
  private mapResourceType(backendType: string): string {
    const mapping: {[key: string]: string} = {
      'GLOBAL': 'catalog',
      'SYSTEM': 'catalog',
      'CATALOG': 'catalog',
      'DATABASE': 'database',
      'TABLE': 'table',
      'ROLE': 'catalog', // Role grants are at catalog level
    };
    return mapping[backendType?.toUpperCase()] || 'database';
  }

  /**
   * Parse resource path and fill form fields
   */
  private parseAndFillResourcePath(resourcePath: string, resourceType: string): void {
    if (!resourcePath) return;

    // Resource path formats:
    // - CATALOG: catalog_name or *.*
    // - DATABASE: database_name.* or catalog.database.*
    // - TABLE: database.table or catalog.database.table

    const parts = resourcePath.split('.');

    if (resourceType === 'CATALOG' || resourceType === 'GLOBAL' || resourceType === 'SYSTEM') {
      if (parts[0] && parts[0] !== '*') {
        this.requestForm.patchValue({ catalog: parts[0] });
      }
    } else if (resourceType === 'DATABASE') {
      if (parts.length >= 1 && parts[0] !== '*') {
        // Could be database.* or catalog.database.*
        if (parts.length === 2) {
          this.requestForm.patchValue({ database: parts[0] });
        } else if (parts.length === 3) {
          this.requestForm.patchValue({
            catalog: parts[0],
            database: parts[1],
          });
        }
      }
    } else if (resourceType === 'TABLE') {
      if (parts.length === 2) {
        this.requestForm.patchValue({
          database: parts[0],
          table: parts[1],
        });
      } else if (parts.length === 3) {
        this.requestForm.patchValue({
          catalog: parts[0],
          database: parts[1],
          table: parts[2],
        });
      }
    }
  }
}
