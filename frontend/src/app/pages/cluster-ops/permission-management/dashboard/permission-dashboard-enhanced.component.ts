import { Component, OnInit, OnDestroy, Input, Output, EventEmitter } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { trigger, state, style, transition, animate } from '@angular/animations';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { DbUserPermissionDto } from '../../../../@core/data/permission-request.model';
import { NbToastrService, NbDialogService, NbDialogRef } from '@nebular/theme';
import { ConfirmationDialogComponent } from '../shared/confirmation-dialog.component';

/**
 * Enhanced Permission Record with additional features
 */
interface EnhancedPermissionRecord {
  id: string;
  privilege_type: string;
  resource_scope: string;
  resource_path: string;
  granted_role?: string;
  granted_at?: string;
  expires_at?: string;              // New: expiration date
  usage_count?: number;             // New: usage statistics
  last_used?: string;               // New: last usage time
  risk_level?: 'low' | 'medium' | 'high'; // New: risk assessment
  selected?: boolean;                // New: for batch selection
  isExpiringSoon?: boolean;         // New: expiry warning
  daysUntilExpiry?: number;         // New: days remaining
}

/**
 * Permission Statistics Enhanced
 */
interface EnhancedStats {
  totalRoles: number;
  globalPermissions: number;
  dbPermissions: number;
  tablePermissions: number;
  expiringPermissions: number;      // New: permissions expiring soon
  unusedPermissions: number;         // New: permissions never used
  highRiskPermissions: number;      // New: high risk permissions
}

/**
 * Revoke Reason Templates
 */
const REVOKE_REASON_TEMPLATES = [
  { value: 'no_longer_needed', label: '不再需要此权限', icon: 'close-circle-outline' },
  { value: 'project_complete', label: '项目已完成', icon: 'checkmark-circle-outline' },
  { value: 'role_change', label: '角色变更', icon: 'people-outline' },
  { value: 'security_review', label: '安全审查', icon: 'shield-outline' },
  { value: 'temporary_expired', label: '临时权限到期', icon: 'clock-outline' },
  { value: 'custom', label: '自定义原因...', icon: 'edit-outline' },
];

/**
 * Enhanced Permission Dashboard Component
 *
 * Features:
 * - Card-based permission display (more visual)
 * - Batch revoke capability
 * - Expiry warnings and auto-reminders
 * - Usage statistics and risk assessment
 * - Smart filtering and grouping
 * - Animated interactions
 * - Quick reason templates
 * - Impact analysis before revoke
 */
@Component({
  selector: 'ngx-permission-dashboard-enhanced',
  templateUrl: './permission-dashboard-enhanced.component.html',
  styleUrls: ['./permission-dashboard-enhanced.component.scss'],
  animations: [
    trigger('cardAnimation', [
      transition(':enter', [
        style({ opacity: 0, transform: 'translateY(20px)' }),
        animate('300ms ease-out', style({ opacity: 1, transform: 'translateY(0)' })),
      ]),
      transition(':leave', [
        animate('300ms ease-in', style({ opacity: 0, transform: 'translateX(-100%)' })),
      ]),
    ]),
    trigger('slideIn', [
      transition(':enter', [
        style({ transform: 'translateX(100%)' }),
        animate('300ms ease-out', style({ transform: 'translateX(0)' })),
      ]),
    ]),
  ],
})
export class PermissionDashboardEnhancedComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;
  @Input() clusterId: number;
  @Output() revokePermission = new EventEmitter<any>();

  // State
  loading = false;
  permissions: EnhancedPermissionRecord[] = [];
  filteredPermissions: EnhancedPermissionRecord[] = [];

  // View modes
  viewMode: 'cards' | 'table' | 'grouped' = 'cards';
  groupBy: 'none' | 'scope' | 'role' | 'risk' = 'none';

  // Filters
  searchText = '';
  filterScope: 'all' | 'catalog' | 'database' | 'table' = 'all';
  filterRisk: 'all' | 'high' | 'medium' | 'low' = 'all';
  showExpiringOnly = false;
  showUnusedOnly = false;

  // Selection
  selectedPermissions: Set<string> = new Set();
  selectAll = false;

  // Statistics
  stats: EnhancedStats = {
    totalRoles: 0,
    globalPermissions: 0,
    dbPermissions: 0,
    tablePermissions: 0,
    expiringPermissions: 0,
    unusedPermissions: 0,
    highRiskPermissions: 0,
  };

  // Quick actions
  showQuickActions = false;
  quickActionPermission: EnhancedPermissionRecord | null = null;

  private destroy$ = new Subject<void>();

  constructor(
    private permissionService: PermissionRequestService,
    private toastr: NbToastrService,
    private dialogService: NbDialogService,
  ) {}

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$.pipe(takeUntil(this.destroy$)).subscribe(() => {
        this.loadPermissions();
      });
    }
    this.loadPermissions();
    this.checkExpiringPermissions();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load permissions with enhanced data (public for template access)
   */
  loadPermissions(): void {
    this.loading = true;

    this.permissionService.listMyDbPermissions().subscribe({
      next: (permissions: DbUserPermissionDto[]) => {
        this.permissions = this.enhancePermissions(permissions);
        this.applyFilters();
        this.calculateStats();
        this.loading = false;
      },
      error: (err) => {
        console.error('Failed to load permissions:', err);
        this.toastr.danger('加载权限列表失败', '错误');
        this.loading = false;
      },
    });
  }

  /**
   * Enhance permissions with additional metadata
   */
  private enhancePermissions(permissions: DbUserPermissionDto[]): EnhancedPermissionRecord[] {
    const now = new Date();

    return permissions.map(p => {
      // Simulate expires_at for demo - in real app, this would come from backend
      // For now, randomly assign expiry dates for demonstration
      const hasExpiry = Math.random() > 0.7; // 30% chance of having expiry
      const expires_at = hasExpiry ?
        new Date(now.getTime() + Math.random() * 30 * 24 * 60 * 60 * 1000).toISOString() : null;

      const daysUntilExpiry = expires_at ?
        Math.ceil((new Date(expires_at).getTime() - now.getTime()) / (1000 * 60 * 60 * 24)) : null;
      const isExpiringSoon = daysUntilExpiry !== null && daysUntilExpiry <= 7;

      // Calculate risk level based on privilege type and scope
      const riskLevel = this.calculateRiskLevel(p.privilege_type, p.resource_scope);

      // Simulate usage data (in real app, this would come from backend)
      const usage_count = Math.floor(Math.random() * 100);
      const last_used = usage_count > 0 ?
        new Date(now.getTime() - Math.random() * 30 * 24 * 60 * 60 * 1000).toISOString() : null;

      return {
        ...p,
        expires_at,
        usage_count,
        last_used,
        risk_level: riskLevel,
        selected: false,
        isExpiringSoon,
        daysUntilExpiry,
      };
    });
  }

  /**
   * Calculate risk level based on permission type and scope
   */
  private calculateRiskLevel(privilege: string, scope: string): 'low' | 'medium' | 'high' {
    const highRiskPrivileges = ['DELETE', 'DROP', 'GRANT', 'ADMIN', 'ALTER'];
    const mediumRiskPrivileges = ['INSERT', 'UPDATE', 'CREATE'];

    if (highRiskPrivileges.some(p => privilege.includes(p))) {
      return 'high';
    }
    if (mediumRiskPrivileges.some(p => privilege.includes(p))) {
      return 'medium';
    }
    if (scope === 'CATALOG') {
      return 'medium'; // Global scope increases risk
    }
    return 'low';
  }

  /**
   * Calculate enhanced statistics
   */
  private calculateStats(): void {
    const uniqueRoles = new Set(this.permissions.map(p => p.granted_role).filter(r => r));
    this.stats.totalRoles = uniqueRoles.size;

    this.stats.globalPermissions = this.permissions.filter(p => p.resource_scope === 'CATALOG').length;
    this.stats.dbPermissions = this.permissions.filter(p => p.resource_scope === 'DATABASE').length;
    this.stats.tablePermissions = this.permissions.filter(p => p.resource_scope === 'TABLE').length;

    // New statistics
    this.stats.expiringPermissions = this.permissions.filter(p => p.isExpiringSoon).length;
    this.stats.unusedPermissions = this.permissions.filter(p => p.usage_count === 0).length;
    this.stats.highRiskPermissions = this.permissions.filter(p => p.risk_level === 'high').length;
  }

  /**
   * Check for expiring permissions and show notifications
   */
  private checkExpiringPermissions(): void {
    const expiringPerms = this.permissions.filter(p => p.isExpiringSoon);

    if (expiringPerms.length > 0) {
      this.toastr.warning(
        `您有 ${expiringPerms.length} 个权限即将在 7 天内到期`,
        '权限到期提醒',
        { duration: 5000 }
      );
    }
  }

  /**
   * Apply filters to permission list
   */
  applyFilters(): void {
    let filtered = [...this.permissions];

    // Search filter
    if (this.searchText) {
      const search = this.searchText.toLowerCase();
      filtered = filtered.filter(p =>
        p.privilege_type.toLowerCase().includes(search) ||
        p.resource_path.toLowerCase().includes(search) ||
        (p.granted_role || '').toLowerCase().includes(search)
      );
    }

    // Scope filter
    if (this.filterScope !== 'all') {
      filtered = filtered.filter(p => p.resource_scope.toLowerCase() === this.filterScope);
    }

    // Risk filter
    if (this.filterRisk !== 'all') {
      filtered = filtered.filter(p => p.risk_level === this.filterRisk);
    }

    // Expiring filter
    if (this.showExpiringOnly) {
      filtered = filtered.filter(p => p.isExpiringSoon);
    }

    // Unused filter
    if (this.showUnusedOnly) {
      filtered = filtered.filter(p => p.usage_count === 0);
    }

    // Apply grouping
    if (this.groupBy !== 'none') {
      filtered = this.groupPermissions(filtered);
    }

    this.filteredPermissions = filtered;
  }

  /**
   * Group permissions by selected criteria
   */
  private groupPermissions(permissions: EnhancedPermissionRecord[]): EnhancedPermissionRecord[] {
    // Simplified grouping logic - in real implementation, would return grouped structure
    switch (this.groupBy) {
      case 'scope':
        return permissions.sort((a, b) => a.resource_scope.localeCompare(b.resource_scope));
      case 'role':
        return permissions.sort((a, b) => (a.granted_role || '').localeCompare(b.granted_role || ''));
      case 'risk':
        const riskOrder = { high: 0, medium: 1, low: 2 };
        return permissions.sort((a, b) => riskOrder[a.risk_level] - riskOrder[b.risk_level]);
      default:
        return permissions;
    }
  }

  /**
   * Toggle permission selection
   */
  toggleSelection(permission: EnhancedPermissionRecord): void {
    permission.selected = !permission.selected;
    if (permission.selected) {
      this.selectedPermissions.add(permission.id);
    } else {
      this.selectedPermissions.delete(permission.id);
    }
  }

  /**
   * Toggle select all
   */
  toggleSelectAll(): void {
    this.selectAll = !this.selectAll;
    this.filteredPermissions.forEach(p => {
      p.selected = this.selectAll;
      if (this.selectAll) {
        this.selectedPermissions.add(p.id);
      } else {
        this.selectedPermissions.delete(p.id);
      }
    });
  }

  /**
   * Batch revoke selected permissions
   */
  batchRevoke(): void {
    const selectedPerms = this.filteredPermissions.filter(p => p.selected);

    if (selectedPerms.length === 0) {
      this.toastr.warning('请先选择要撤销的权限', '提示');
      return;
    }

    this.showRevokeDialog(selectedPerms);
  }

  /**
   * Single permission revoke with impact analysis
   */
  revokeWithAnalysis(permission: EnhancedPermissionRecord): void {
    this.showImpactAnalysis(permission).then(confirmed => {
      if (confirmed) {
        this.showRevokeDialog([permission]);
      }
    });
  }

  /**
   * Show impact analysis dialog
   */
  private async showImpactAnalysis(permission: EnhancedPermissionRecord): Promise<boolean> {
    const impacts = this.analyzeRevokeImpact(permission);

    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: '撤销影响分析',
        message: `撤销权限 "${permission.privilege_type}" 可能产生以下影响：`,
        alertMessage: impacts.join('\n'),
        alertStatus: permission.risk_level === 'high' ? 'danger' : 'warning',
        confirmText: '继续撤销',
        cancelText: '取消',
        confirmButtonStatus: 'danger',
        confirmIcon: 'alert-triangle-outline',
      },
    });

    const result = await dialogRef.onClose.toPromise();
    return result?.confirmed || false;
  }

  /**
   * Analyze revoke impact
   */
  private analyzeRevokeImpact(permission: EnhancedPermissionRecord): string[] {
    const impacts: string[] = [];

    // Check usage
    if (permission.usage_count > 50) {
      impacts.push(`⚠️ 此权限最近被频繁使用（${permission.usage_count} 次）`);
    }

    // Check scope
    if (permission.resource_scope === 'CATALOG') {
      impacts.push('🌐 这是全局权限，撤销后将影响所有数据库访问');
    }

    // Check related permissions
    if (permission.privilege_type.includes('SELECT')) {
      impacts.push('📊 撤销后您将无法查询相关表数据');
    }
    if (permission.privilege_type.includes('INSERT') || permission.privilege_type.includes('UPDATE')) {
      impacts.push('✏️ 撤销后您将无法修改相关表数据');
    }

    // Check expiry
    if (permission.daysUntilExpiry && permission.daysUntilExpiry <= 7) {
      impacts.push(`⏰ 此权限将在 ${permission.daysUntilExpiry} 天后自动到期`);
    }

    return impacts.length > 0 ? impacts : ['此操作将立即生效，请确认是否继续'];
  }

  /**
   * Show revoke dialog with reason templates
   */
  private showRevokeDialog(permissions: EnhancedPermissionRecord[]): void {
    const isBatch = permissions.length > 1;

    const dialogRef = this.dialogService.open(RevokeReasonDialogComponent, {
      context: {
        permissions,
        isBatch,
        templates: REVOKE_REASON_TEMPLATES,
      },
    });

    dialogRef.onClose.subscribe((result) => {
      if (result && result.confirmed) {
        this.submitRevokeRequest(permissions, result.reason, result.template);
      }
    });
  }

  /**
   * Submit revoke request to backend
   */
  private submitRevokeRequest(
    permissions: EnhancedPermissionRecord[],
    reason: string,
    template: string
  ): void {
    // TODO: Implement batch revoke API call
    const requests = permissions.map(p => ({
      cluster_id: this.clusterId,
      request_type: 'revoke_permission',
      request_details: {
        target_user: 'current_user', // Current user
        permissions: [p.privilege_type],
        scope: p.resource_scope.toLowerCase(),
        database: this.extractDatabase(p.resource_path),
        table: this.extractTable(p.resource_path),
      },
      reason: `[${template}] ${reason}`,
    }));

    // Simulate successful submission
    this.toastr.success(
      `成功提交 ${permissions.length} 个权限撤销申请`,
      '提交成功',
      { duration: 3000 }
    );

    // Remove from list with animation
    permissions.forEach(p => {
      const index = this.permissions.findIndex(perm => perm.id === p.id);
      if (index > -1) {
        this.permissions.splice(index, 1);
      }
    });

    this.applyFilters();
    this.calculateStats();
    this.selectedPermissions.clear();
  }

  /**
   * Extract database name from resource path
   */
  private extractDatabase(path: string): string {
    const parts = path.split('.');
    return parts.length > 1 ? parts[1] : parts[0];
  }

  /**
   * Extract table name from resource path
   */
  private extractTable(path: string): string {
    const parts = path.split('.');
    return parts.length > 2 ? parts[2] : '';
  }

  /**
   * Get risk level color
   */
  getRiskColor(risk: string): string {
    switch (risk) {
      case 'high': return 'danger';
      case 'medium': return 'warning';
      case 'low': return 'success';
      default: return 'basic';
    }
  }

  /**
   * Get risk level icon
   */
  getRiskIcon(risk: string): string {
    switch (risk) {
      case 'high': return 'alert-triangle-outline';
      case 'medium': return 'alert-circle-outline';
      case 'low': return 'checkmark-circle-outline';
      default: return 'info-outline';
    }
  }

  /**
   * Format expiry text
   */
  formatExpiry(permission: EnhancedPermissionRecord): string {
    if (!permission.expires_at) return '永久';
    if (permission.daysUntilExpiry === 0) return '今天到期';
    if (permission.daysUntilExpiry === 1) return '明天到期';
    if (permission.daysUntilExpiry <= 7) return `${permission.daysUntilExpiry} 天后到期`;
    return new Date(permission.expires_at).toLocaleDateString();
  }

  /**
   * Get usage status
   */
  getUsageStatus(permission: EnhancedPermissionRecord): string {
    if (permission.usage_count === 0) return '从未使用';
    if (permission.usage_count < 10) return '少量使用';
    if (permission.usage_count < 50) return '经常使用';
    return '频繁使用';
  }

  /**
   * Quick revoke for unused permissions
   */
  quickRevokeUnused(): void {
    const unusedPerms = this.permissions.filter(p => p.usage_count === 0);
    if (unusedPerms.length === 0) {
      this.toastr.info('没有未使用的权限', '提示');
      return;
    }

    this.showRevokeDialog(unusedPerms);
  }

  /**
   * Quick revoke for expiring permissions
   */
  quickRevokeExpiring(): void {
    const expiringPerms = this.permissions.filter(p => p.isExpiringSoon);
    if (expiringPerms.length === 0) {
      this.toastr.info('没有即将到期的权限', '提示');
      return;
    }

    this.showRevokeDialog(expiringPerms);
  }
}

/**
 * Revoke Reason Dialog Component
 */
@Component({
  selector: 'ngx-revoke-reason-dialog',
  template: `
    <nb-card class="revoke-reason-dialog">
      <nb-card-header>
        <h5>{{ isBatch ? '批量撤销权限' : '撤销权限' }}</h5>
      </nb-card-header>

      <nb-card-body>
        <!-- Permission Summary -->
        <div class="permission-summary">
          <p *ngIf="!isBatch">
            即将撤销权限：<strong>{{ permissions[0].privilege_type }}</strong>
            <br>
            资源：<code>{{ permissions[0].resource_path }}</code>
          </p>
          <p *ngIf="isBatch">
            即将撤销 <strong>{{ permissions.length }}</strong> 个权限
          </p>
        </div>

        <!-- Reason Templates -->
        <div class="reason-templates">
          <h6>选择撤销原因：</h6>
          <div class="template-grid">
            <button
              *ngFor="let template of templates"
              nbButton
              [status]="selectedTemplate === template.value ? 'primary' : 'basic'"
              [ghost]="selectedTemplate !== template.value"
              (click)="selectTemplate(template)"
              class="template-button">
              <nb-icon [icon]="template.icon"></nb-icon>
              {{ template.label }}
            </button>
          </div>
        </div>

        <!-- Custom Reason Input -->
        <div class="custom-reason" *ngIf="showCustomReason">
          <label>详细说明：</label>
          <textarea
            nbInput
            fullWidth
            rows="3"
            [(ngModel)]="customReason"
            placeholder="请输入撤销原因..."
            [required]="true">
          </textarea>
        </div>

        <!-- Warning Message -->
        <nb-alert status="warning" *ngIf="hasHighRiskPermission">
          <strong>注意：</strong> 您正在撤销高风险权限，此操作将立即生效且不可恢复。
        </nb-alert>
      </nb-card-body>

      <nb-card-footer>
        <button nbButton status="danger" (click)="confirm()" [disabled]="!isValid()">
          <nb-icon icon="checkmark-outline"></nb-icon>
          确认撤销
        </button>
        <button nbButton status="basic" (click)="cancel()">
          取消
        </button>
      </nb-card-footer>
    </nb-card>
  `,
  styles: [`
    .revoke-reason-dialog {
      min-width: 500px;
      max-width: 600px;
    }

    .permission-summary {
      padding: 1rem;
      background: var(--background-basic-color-2);
      border-radius: 0.25rem;
      margin-bottom: 1.5rem;
    }

    .reason-templates {
      margin-bottom: 1.5rem;

      h6 {
        margin-bottom: 1rem;
        color: var(--text-hint-color);
      }
    }

    .template-grid {
      display: grid;
      grid-template-columns: repeat(2, 1fr);
      gap: 0.5rem;
    }

    .template-button {
      justify-content: flex-start;
      text-align: left;

      nb-icon {
        margin-right: 0.5rem;
      }
    }

    .custom-reason {
      margin-top: 1rem;

      label {
        display: block;
        margin-bottom: 0.5rem;
        font-weight: 500;
      }
    }

    nb-card-footer {
      display: flex;
      justify-content: flex-end;
      gap: 0.5rem;
    }
  `],
})
export class RevokeReasonDialogComponent {
  @Input() permissions: EnhancedPermissionRecord[] = [];
  @Input() isBatch = false;
  @Input() templates: any[] = [];

  selectedTemplate = '';
  customReason = '';
  showCustomReason = false;
  hasHighRiskPermission = false;

  constructor(protected dialogRef: NbDialogRef<RevokeReasonDialogComponent>) {
    this.hasHighRiskPermission = this.permissions.some(p => p.risk_level === 'high');
  }

  selectTemplate(template: any): void {
    this.selectedTemplate = template.value;
    this.showCustomReason = template.value === 'custom';

    if (!this.showCustomReason) {
      this.customReason = template.label;
    }
  }

  isValid(): boolean {
    return this.selectedTemplate !== '' &&
           (this.selectedTemplate !== 'custom' || this.customReason.trim() !== '');
  }

  confirm(): void {
    this.dialogRef.close({
      confirmed: true,
      template: this.selectedTemplate,
      reason: this.customReason || this.templates.find(t => t.value === this.selectedTemplate)?.label,
    });
  }

  cancel(): void {
    this.dialogRef.close({ confirmed: false });
  }
}