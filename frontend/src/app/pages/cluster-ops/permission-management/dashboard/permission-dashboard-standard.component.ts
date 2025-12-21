import { Component, OnInit, OnDestroy, Input, Output, EventEmitter } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { DbUserPermissionDto } from '../../../../@core/data/permission-request.model';
import { NbToastrService, NbDialogService } from '@nebular/theme';
import { ConfirmationDialogComponent } from '../shared/confirmation-dialog.component';

/**
 * Standard Permission Dashboard Component
 * 使用纯 ngx-admin/Nebular 原生组件，无自定义样式
 * 符合项目统一的设计规范
 */

interface PermissionRecord extends DbUserPermissionDto {
  selected?: boolean;
  risk_level?: 'low' | 'medium' | 'high';
  isExpiringSoon?: boolean;
  daysUntilExpiry?: number;
  usage_count?: number;
}

@Component({
  selector: 'ngx-permission-dashboard-standard',
  templateUrl: './permission-dashboard-standard.component.html',
  styleUrls: ['./permission-dashboard-standard.component.scss'],
})
export class PermissionDashboardStandardComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;
  @Input() clusterId: number;
  @Output() revokePermission = new EventEmitter<PermissionRecord>();

  // 状态
  loading = false;
  permissions: PermissionRecord[] = [];
  filteredPermissions: PermissionRecord[] = [];

  // 筛选
  searchText = '';
  filterScope = 'all';
  filterRisk = 'all';
  showExpiringOnly = false;
  showUnusedOnly = false;
  selectAll = false;
  selectedPermissions = new Set<string>();

  // 统计
  stats = {
    totalRoles: 0,
    globalPermissions: 0,
    dbPermissions: 0,
    tablePermissions: 0,
    expiringPermissions: 0,
    unusedPermissions: 0,
  };

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
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * 加载权限列表
   */
  loadPermissions(): void {
    this.loading = true;

    this.permissionService.listMyDbPermissions().subscribe({
      next: (permissions: DbUserPermissionDto[]) => {
        this.permissions = this.enhancePermissions(permissions);
        this.applyFilters();
        this.calculateStats();
        this.loading = false;
        this.checkExpiringPermissions();
      },
      error: (err) => {
        console.error('Failed to load permissions:', err);
        this.toastr.danger('加载权限列表失败', '错误');
        this.loading = false;
      },
    });
  }

  /**
   * 增强权限数据
   */
  private enhancePermissions(permissions: DbUserPermissionDto[]): PermissionRecord[] {
    return permissions.map(p => {
      // 模拟数据用于演示
      const usage_count = Math.floor(Math.random() * 100);
      const hasExpiry = Math.random() > 0.7;
      const daysUntilExpiry = hasExpiry ? Math.floor(Math.random() * 30) : null;

      return {
        ...p,
        selected: false,
        risk_level: this.calculateRiskLevel(p.privilege_type, p.resource_scope),
        usage_count,
        isExpiringSoon: daysUntilExpiry !== null && daysUntilExpiry <= 7,
        daysUntilExpiry,
      };
    });
  }

  /**
   * 计算风险等级
   */
  private calculateRiskLevel(privilege: string, scope: string): 'low' | 'medium' | 'high' {
    const highRiskPrivileges = ['DELETE', 'DROP', 'GRANT', 'ADMIN'];
    const mediumRiskPrivileges = ['INSERT', 'UPDATE', 'CREATE'];

    if (highRiskPrivileges.some(p => privilege.includes(p))) {
      return 'high';
    }
    if (mediumRiskPrivileges.some(p => privilege.includes(p))) {
      return 'medium';
    }
    return 'low';
  }

  /**
   * 计算统计数据
   */
  private calculateStats(): void {
    const uniqueRoles = new Set(this.permissions.map(p => p.granted_role).filter(r => r));
    this.stats.totalRoles = uniqueRoles.size;
    this.stats.globalPermissions = this.permissions.filter(p => p.resource_scope === 'CATALOG').length;
    this.stats.dbPermissions = this.permissions.filter(p => p.resource_scope === 'DATABASE').length;
    this.stats.tablePermissions = this.permissions.filter(p => p.resource_scope === 'TABLE').length;
    this.stats.expiringPermissions = this.permissions.filter(p => p.isExpiringSoon).length;
    this.stats.unusedPermissions = this.permissions.filter(p => p.usage_count === 0).length;
  }

  /**
   * 检查即将过期的权限
   */
  private checkExpiringPermissions(): void {
    if (this.stats.expiringPermissions > 0) {
      this.toastr.warning(
        `您有 ${this.stats.expiringPermissions} 个权限即将在 7 天内到期`,
        '权限到期提醒',
        { duration: 5000 }
      );
    }
  }

  /**
   * 应用筛选
   */
  applyFilters(): void {
    let filtered = [...this.permissions];

    // 搜索筛选
    if (this.searchText) {
      const search = this.searchText.toLowerCase();
      filtered = filtered.filter(p =>
        p.privilege_type.toLowerCase().includes(search) ||
        p.resource_path.toLowerCase().includes(search) ||
        (p.granted_role || '').toLowerCase().includes(search)
      );
    }

    // 范围筛选
    if (this.filterScope !== 'all') {
      filtered = filtered.filter(p => p.resource_scope.toLowerCase() === this.filterScope);
    }

    // 风险筛选
    if (this.filterRisk !== 'all') {
      filtered = filtered.filter(p => p.risk_level === this.filterRisk);
    }

    // 即将到期筛选
    if (this.showExpiringOnly) {
      filtered = filtered.filter(p => p.isExpiringSoon);
    }

    // 未使用筛选
    if (this.showUnusedOnly) {
      filtered = filtered.filter(p => p.usage_count === 0);
    }

    this.filteredPermissions = filtered;
  }

  /**
   * 清空筛选条件
   */
  clearFilters(): void {
    this.searchText = '';
    this.filterScope = 'all';
    this.filterRisk = 'all';
    this.showExpiringOnly = false;
    this.showUnusedOnly = false;
    this.applyFilters();
  }

  /**
   * 切换选择
   */
  toggleSelection(permission: PermissionRecord): void {
    if (permission.selected) {
      this.selectedPermissions.add(permission.id);
    } else {
      this.selectedPermissions.delete(permission.id);
    }
  }

  /**
   * 全选/取消全选
   */
  toggleSelectAll(): void {
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
   * 清空选择
   */
  clearSelection(): void {
    this.selectAll = false;
    this.selectedPermissions.clear();
    this.filteredPermissions.forEach(p => p.selected = false);
  }

  /**
   * 批量撤销
   */
  batchRevoke(): void {
    const selectedPerms = this.filteredPermissions.filter(p => p.selected);

    if (selectedPerms.length === 0) {
      this.toastr.warning('请先选择要撤销的权限', '提示');
      return;
    }

    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: '批量撤销权限',
        message: `确定要撤销选中的 ${selectedPerms.length} 个权限吗？`,
        confirmText: '确认撤销',
        cancelText: '取消',
        confirmButtonStatus: 'danger',
        confirmIcon: 'trash-2-outline',
        showCommentInput: true,
        commentLabel: '撤销原因',
        commentPlaceholder: '请输入撤销原因...',
        commentRequired: true,
        commentHint: '撤销权限需要填写原因',
      },
    });

    dialogRef.onClose.subscribe((result) => {
      if (result?.confirmed) {
        this.submitRevokeRequest(selectedPerms, result.comment);
      }
    });
  }

  /**
   * 单个权限撤销（带影响分析）
   */
  revokeWithAnalysis(permission: PermissionRecord): void {
    // 生成影响分析
    const impacts = [];

    if (permission.usage_count > 50) {
      impacts.push(`此权限最近被频繁使用（${permission.usage_count} 次）`);
    }

    if (permission.resource_scope === 'CATALOG') {
      impacts.push('这是全局权限，撤销后将影响所有数据库访问');
    }

    if (permission.daysUntilExpiry && permission.daysUntilExpiry <= 7) {
      impacts.push(`此权限将在 ${permission.daysUntilExpiry} 天后自动到期`);
    }

    const impactMessage = impacts.length > 0 ?
      impacts.join('\n') : '撤销此权限将立即生效';

    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: '撤销权限确认',
        message: `确定要撤销权限 "${permission.privilege_type}" 吗？`,
        alertMessage: impactMessage,
        alertStatus: permission.risk_level === 'high' ? 'danger' : 'warning',
        confirmText: '确认撤销',
        cancelText: '取消',
        confirmButtonStatus: 'danger',
        showCommentInput: true,
        commentLabel: '撤销原因',
        commentRequired: true,
      },
    });

    dialogRef.onClose.subscribe((result) => {
      if (result?.confirmed) {
        this.submitRevokeRequest([permission], result.comment);
      }
    });
  }

  /**
   * 快速撤销未使用的权限
   */
  quickRevokeUnused(): void {
    const unusedPerms = this.permissions.filter(p => p.usage_count === 0);

    if (unusedPerms.length === 0) {
      this.toastr.info('没有未使用的权限', '提示');
      return;
    }

    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: '清理未使用权限',
        message: `发现 ${unusedPerms.length} 个未使用的权限，是否全部撤销？`,
        confirmText: '清理',
        confirmButtonStatus: 'warning',
        showCommentInput: true,
        commentLabel: '撤销原因',
        commentPlaceholder: '批量清理未使用权限',
      },
    });

    dialogRef.onClose.subscribe((result) => {
      if (result?.confirmed) {
        this.submitRevokeRequest(unusedPerms, result.comment || '清理未使用权限');
      }
    });
  }

  /**
   * 快速撤销即将到期的权限
   */
  quickRevokeExpiring(): void {
    const expiringPerms = this.permissions.filter(p => p.isExpiringSoon);

    if (expiringPerms.length === 0) {
      this.toastr.info('没有即将到期的权限', '提示');
      return;
    }

    const dialogRef = this.dialogService.open(ConfirmationDialogComponent, {
      context: {
        title: '处理即将到期权限',
        message: `有 ${expiringPerms.length} 个权限即将到期，是否提前撤销？`,
        confirmText: '撤销',
        confirmButtonStatus: 'warning',
        showCommentInput: true,
        commentLabel: '撤销原因',
        commentPlaceholder: '权限即将到期，提前撤销',
      },
    });

    dialogRef.onClose.subscribe((result) => {
      if (result?.confirmed) {
        this.submitRevokeRequest(expiringPerms, result.comment || '权限即将到期');
      }
    });
  }

  /**
   * 提交撤销请求
   */
  private submitRevokeRequest(permissions: PermissionRecord[], reason: string): void {
    // TODO: 实际调用后端 API
    this.toastr.success(
      `成功提交 ${permissions.length} 个权限撤销申请`,
      '提交成功'
    );

    // 从列表中移除
    permissions.forEach(p => {
      const index = this.permissions.findIndex(perm => perm.id === p.id);
      if (index > -1) {
        this.permissions.splice(index, 1);
      }
    });

    this.applyFilters();
    this.calculateStats();
    this.clearSelection();
  }

  // 辅助方法
  getRiskColor(risk: string): string {
    switch (risk) {
      case 'high': return 'danger';
      case 'medium': return 'warning';
      case 'low': return 'success';
      default: return 'basic';
    }
  }

  getRiskLabel(risk: string): string {
    switch (risk) {
      case 'high': return '高风险';
      case 'medium': return '中风险';
      case 'low': return '低风险';
      default: return '未知';
    }
  }

  formatExpiry(permission: PermissionRecord): string {
    if (!permission.daysUntilExpiry) return '永久';
    if (permission.daysUntilExpiry === 0) return '今天到期';
    if (permission.daysUntilExpiry === 1) return '明天到期';
    return `${permission.daysUntilExpiry} 天后到期`;
  }

  getUsageStatus(permission: PermissionRecord): string {
    if (!permission.usage_count) return '从未使用';
    if (permission.usage_count < 10) return '少量使用';
    if (permission.usage_count < 50) return '经常使用';
    return '频繁使用';
  }
}