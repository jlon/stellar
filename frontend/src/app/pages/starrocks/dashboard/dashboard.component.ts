import { I18nService } from '../../../@core/i18n/i18n.service';
import { CommonModule } from '@angular/common';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, ChangeDetectionStrategy, ChangeDetectorRef, inject } from '@angular/core';
import { Router } from '@angular/router';
import { Subject, interval } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbToastrService, NbIconModule, NbButtonModule, NbSpinnerModule, NbCardModule, NbTooltipModule, NbTagModule, NbDialogService } from '@nebular/theme';
import { ClusterService, Cluster, ClusterHealth, ClusterResourceSummary } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { OrganizationService, Organization } from '../../../@core/data/organization.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { PermissionService } from '../../../@core/data/permission.service';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { ClusterFormComponent } from '../clusters/cluster-form/cluster-form.component';
import { AuthService } from '../../../@core/data/auth.service';

interface ClusterCard {
  cluster: Cluster;
  health?: ClusterHealth;
  resources?: ClusterResourceSummary;
  loading: boolean;
  isActive: boolean;
  organization?: Organization;
  showHealthDetails?: boolean;
}

@Component({
    selector: 'ngx-dashboard',
    templateUrl: './dashboard.component.html',
    styleUrls: ['./dashboard.component.scss'],
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [
    TranslatePipe,
    CommonModule,
    NbIconModule,
    NbButtonModule,
    NbSpinnerModule,
    NbCardModule,
    NbTagModule
],
})
export class DashboardComponent implements OnInit, OnDestroy {
  private clusterService = inject(ClusterService)
  private i18n = inject(I18nService);
  private clusterContext = inject(ClusterContextService);
  private organizationService = inject(OrganizationService);
  private toastrService = inject(NbToastrService);
  private router = inject(Router);
  private permissionService = inject(PermissionService);
  private confirmDialogService = inject(ConfirmDialogService);
  private dialogService = inject(NbDialogService);
  private authService = inject(AuthService);
  private cdr = inject(ChangeDetectorRef);

  clusters: ClusterCard[] = [];
  loading = true;
  activeCluster: Cluster | null = null;
  hasClusterAccess = false;
  organizationsMap = new Map<number, Organization>();
  isSuperAdmin = false;
  canListClusters = false;
  canCreateCluster = false;
  canUpdateCluster = false;
  canDeleteCluster = false;
  canActivateCluster = false;
  canViewActiveCluster = false;
  canViewBackends = false;
  canViewFrontends = false;
  canViewQueries = false;
  private permissionSignature = '';
  private destroy$ = new Subject<void>();

  ngOnInit(): void {
    // 语言切换时强制重绘（OnPush 下 TS instant() 返回值不会自动刷新）
    this.i18n.lang$.subscribe(() => this.cdr.markForCheck());

    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe(cluster => {
        this.activeCluster = cluster;
        this.updateActiveStatus();
        this.cdr.markForCheck();
      });

    this.permissionService.permissions$
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => {
        this.applyPermissionState();
        this.cdr.markForCheck();
      });

    this.applyPermissionState();

    // 定时刷新卡片资源数据（与后端 30s 采集对齐；无轮询时页面会停留在加载时的旧快照）
    interval(30000)
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.loadClusters(true));

    // Load organizations if super admin
    this.isSuperAdmin = this.authService.isSuperAdmin();
    if (this.isSuperAdmin) {
      this.loadOrganizations();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  private loadOrganizations(): void {
    this.organizationService.listOrganizations().subscribe({
      next: (orgs) => {
        orgs.forEach(org => this.organizationsMap.set(org.id, org));
        this.cdr.markForCheck();
      },
      error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
    });
  }

  loadClusters(silent = false): void {
    if (!this.canListClusters) {
      this.loading = false;
      this.cdr.markForCheck();
      return;
    }

    // 轮询刷新时不闪整页 loading（静默更新卡片数据）
    if (!silent) {
      this.loading = true;
      this.cdr.markForCheck();
    }
    this.clusterService.listClusters().subscribe({
      next: (clusters) => {
        // Update clusters, setting isActive based on backend response
        this.clusters = clusters.map((cluster) => ({
          cluster,
          loading: false,
          isActive: cluster.is_active,
          organization: cluster.organization_id ? this.organizationsMap.get(cluster.organization_id) : undefined,
          resources: this.toResourceSummary(cluster),
        }));
        
        // Refresh active cluster from backend
        this.clusterContext.refreshActiveCluster();
        
        this.loadHealthStatus();
        this.loading = false;
        this.cdr.markForCheck();
      },
      error: (error) => {
        this.handleError(error);
        this.loading = false;
        this.cdr.markForCheck();
      },
    });
  }

  updateActiveStatus(): void {
    // isActive status now comes from backend
    // Just need to refresh the display
    this.clusters.forEach(card => {
      // Status is already set from loadClusters based on is_active field
    });
  }

  toggleActiveCluster(clusterCard: ClusterCard) {
    if (!this.canActivateCluster) {
      this.toastrService.warning(this.i18n.instant('您没有激活集群的权限'), this.i18n.instant('提示'));
      return;
    }

    if (clusterCard.isActive) {
      this.toastrService.warning(this.i18n.instant('此集群已是活跃状态'), this.i18n.instant('提示'));
      return;
    }
    this.clusterContext.setActiveCluster(clusterCard.cluster);
    this.toastrService.success(`已激活集群: ${clusterCard.cluster.name}`, '成功');
    this.cdr.markForCheck();
      
      // Reload clusters to update is_active status
      setTimeout(() => this.loadClusters(), 500);
  }

  toResourceSummary(cluster: Cluster): ClusterResourceSummary | undefined {
    if (
      cluster.cpu_usage_pct == null &&
      cluster.memory_usage_pct == null &&
      cluster.disk_usage_pct == null
    ) {
      return undefined;
    }
    return {
      cluster_id: cluster.id,
      cpu_usage_pct: cluster.cpu_usage_pct,
      memory_usage_pct: cluster.memory_usage_pct,
      disk_usage_pct: cluster.disk_usage_pct,
    };
  }

  usageTone(pct?: number): '' | 'warning' | 'danger' {
    if (pct == null || !Number.isFinite(pct)) {
      return '';
    }
    if (pct >= 90) {
      return 'danger';
    }
    if (pct >= 80) {
      return 'warning';
    }
    return '';
  }

  formatPct(pct?: number): string {
    if (pct == null || !Number.isFinite(pct)) {
      return '—';
    }
    return `${Math.round(pct)}%`;
  }

  clampPct(pct?: number): number {
    if (pct == null || !Number.isFinite(pct)) {
      return 0;
    }
    return Math.min(100, Math.max(0, pct));
  }

  loadHealthStatus(): void {
    if (!this.hasClusterAccess) {
      return;
    }

    this.clusters.forEach((clusterCard) => {
      clusterCard.loading = true;
      this.cdr.markForCheck();
      this.clusterService.getHealth(clusterCard.cluster.id).subscribe({
        next: (health) => {
          clusterCard.health = health;
          clusterCard.loading = false;
          this.cdr.markForCheck();
        },
        error: () => {
          clusterCard.loading = false;
          this.cdr.markForCheck();
        },
      });
    });
  }

  statusTone(status?: string): '' | 'ok' | 'warning' | 'danger' {
    if (status === 'healthy') {
      return 'ok';
    }
    if (status === 'warning') {
      return 'warning';
    }
    if (status === 'critical') {
      return 'danger';
    }
    return '';
  }

  getHealthBadgeText(clusterCard: ClusterCard): string {
    if (!clusterCard.health) {
      return this.i18n.instant('未知');
    }
    const status = clusterCard.health.status;
    if (status === 'healthy') {
      return this.i18n.instant('运行中');
    }
    if (status === 'warning') {
      return this.i18n.instant('警告');
    }
    return this.i18n.instant('异常');
  }

  toggleHealthDetails(clusterCard: ClusterCard): void {
    clusterCard.showHealthDetails = !clusterCard.showHealthDetails;
    this.cdr.markForCheck();
  }

  getFailedChecksCount(clusterCard: ClusterCard): number {
    if (!clusterCard.health?.checks) {
      return 0;
    }
    return clusterCard.health.checks.filter(c => c.status !== 'ok').length;
  }

  // Get FE node count from health checks
  getFeCount(clusterCard: ClusterCard): string {
    if (!clusterCard.health?.checks) {
      return '—';
    }
    
    const feCheck = clusterCard.health.checks.find(c => 
      c.name.toLowerCase().includes('frontend') || 
      c.name.toLowerCase().includes('fe')
    );
    
    if (feCheck && feCheck.message && feCheck.status !== 'critical') {
      const match = feCheck.message.match(/^(?:All\s+)?(\d+)(?:\/\d+)?\s+FE/i);
      if (match) {
        return match[1];
      }
    }
    
    return '—';
  }

  // Get compute node count from health checks (BE for shared-nothing, CN for shared-data)
  getComputeNodeCount(clusterCard: ClusterCard): string {
    if (!clusterCard.health?.checks) {
      return '—';
    }
    const computeCheck = clusterCard.health.checks.find(c => 
      c.name.toLowerCase().includes('compute')
    );
    if (computeCheck?.message && computeCheck.status !== 'critical') {
      const match = computeCheck.message.match(/^(?:All\s+)?(\d+)(?:\/\d+)?\s+(?:BE|CN)/i);
      if (match) {
        return match[1];
      }
    }
    return '—';
  }

  // Check if cluster is shared-data mode
  isSharedData(clusterCard: ClusterCard): boolean {
    return clusterCard.cluster.deployment_mode === 'shared_data';
  }

  computeNodeShort(clusterCard: ClusterCard): string {
    return this.isSharedData(clusterCard) ? 'CN' : 'BE';
  }

  get alertCount(): number {
    return this.countByStatus('warning') + this.countByStatus('critical');
  }

  failedCountLabel(clusterCard: ClusterCard): string {
    const failed = this.getFailedChecksCount(clusterCard);
    return failed > 0 ? `${failed} 异常` : '';
  }

  trackByClusterId(_: number, clusterCard: ClusterCard): number {
    return clusterCard.cluster.id;
  }

  onClusterCardKeydown(event: KeyboardEvent, clusterCard: ClusterCard): void {
    if (event.target !== event.currentTarget) {
      return;
    }
    event.preventDefault();
    this.navigateToClusterOverview(clusterCard);
  }

  navigateToClusterOverview(clusterCard: ClusterCard): void {
    if (!this.permissionService.hasPermission('menu:overview')) {
      this.toastrService.warning(this.i18n.instant('您没有查看集群概览的权限'), this.i18n.instant('提示'));
      return;
    }

    const navigate = () => this.router.navigate(['/pages/starrocks/overview']);
    if (clusterCard.isActive) {
      navigate();
      return;
    }

    if (!this.canActivateCluster) {
      this.toastrService.warning(this.i18n.instant('请先切换到该集群后再查看概览'), this.i18n.instant('提示'));
      return;
    }

    // 查看非当前集群的概览必须显式切换：禁止静默激活
    this.confirmDialogService.confirm(
      '切换集群',
      `查看「${clusterCard.cluster.name}」的概览需要先将其设为当前集群，是否切换？`,
      '切换并查看',
      '取消',
      'primary'
    ).pipe(takeUntil(this.destroy$)).subscribe((confirmed) => {
      if (!confirmed) {
        return;
      }
      this.clusterService.activateCluster(clusterCard.cluster.id).subscribe({
        next: () => {
          this.clusters.forEach(card => {
            card.isActive = card.cluster.id === clusterCard.cluster.id;
          });
          this.clusterContext.refreshActiveCluster();
          this.cdr.markForCheck();
          navigate();
        },
        error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
      });
    });
  }

  navigateToBackends(clusterId?: number): void {
    if (!this.canViewBackends) {
      this.toastrService.warning(this.i18n.instant('您没有查看计算节点的权限'), this.i18n.instant('提示'));
      return;
    }
    
    // Activate cluster first if clicking from a specific cluster card
    if (clusterId) {
      const clusterCard = this.clusters.find(c => c.cluster.id === clusterId);
      if (clusterCard && !clusterCard.isActive) {
        this.clusterService.activateCluster(clusterId).subscribe({
          next: () => {
            this.router.navigate(['/pages/starrocks/backends']);
          },
          error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
        });
        return;
      }
    }
    
    // Navigate to backends page
    this.router.navigate(['/pages/starrocks/backends']);
  }

  navigateToFrontends(clusterId?: number): void {
    if (!this.canViewFrontends) {
      this.toastrService.warning(this.i18n.instant('您没有查看 Frontend 节点的权限'), this.i18n.instant('提示'));
      return;
    }
    
    // Activate cluster first if clicking from a specific cluster card
    if (clusterId) {
      const clusterCard = this.clusters.find(c => c.cluster.id === clusterId);
      if (clusterCard && !clusterCard.isActive) {
        this.clusterService.activateCluster(clusterId).subscribe({
          next: () => {
            this.router.navigate(['/pages/starrocks/frontends']);
          },
          error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
        });
        return;
      }
    }
    
    // Navigate to frontends page
    this.router.navigate(['/pages/starrocks/frontends']);
  }

  addCluster(): void {
    if (!this.canCreateCluster) {
      this.toastrService.warning(this.i18n.instant('您没有创建集群的权限'), this.i18n.instant('提示'));
      return;
    }
    this.dialogService
      .open(ClusterFormComponent, { context: { clusterId: null }, dialogClass: 'side-sheet' })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadClusters();
        }
      });
  }

  editCluster(cluster: Cluster): void {
    if (!this.canUpdateCluster) {
      this.toastrService.warning(this.i18n.instant('您没有编辑集群的权限'), this.i18n.instant('提示'));
      return;
    }
    this.dialogService
      .open(ClusterFormComponent, { context: { clusterId: cluster.id }, dialogClass: 'side-sheet' })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadClusters();
        }
      });
  }

  deleteCluster(cluster: Cluster): void {
    if (!this.canDeleteCluster) {
      this.toastrService.warning(this.i18n.instant('您没有删除集群的权限'), this.i18n.instant('提示'));
      return;
    }
    this.confirmDialogService.confirmDelete(cluster.name)
      .subscribe(confirmed => {
        if (!confirmed) {
          return;
        }

        this.clusterService.deleteCluster(cluster.id).subscribe({
          next: () => {
            this.toastrService.success(`集群 "${cluster.name}" 已删除`, '成功');
            this.loadClusters();
            this.cdr.markForCheck();
          },
          error: (error) => {
            this.handleError(error);
            this.cdr.markForCheck();
          },
        });
      });
  }

  private countByStatus(status: string): number {
    return this.clusters.filter(card => card.health?.status === status).length;
  }

  private handleError(error: any): void {
    console.error('Error:', error);
    this.toastrService.danger(
      ErrorHandler.extractErrorMessage(error),
      '错误',
    );
  }

  private applyPermissionState(): void {
    const canList = this.permissionService.hasPermission('api:clusters:list');
    const canCreate = this.permissionService.hasPermission('api:clusters:create');
    const canUpdate = this.permissionService.hasPermission('api:clusters:update');
    const canDelete = this.permissionService.hasPermission('api:clusters:delete');
    const canActivate = this.permissionService.hasPermission('api:clusters:activate');
    const canViewActive = this.permissionService.hasPermission('api:clusters:active');
    const canViewBackends = this.permissionService.hasPermission('api:clusters:backends');
    const canViewFrontends = this.permissionService.hasPermission('api:clusters:frontends');
    const canViewQuery = this.permissionService.hasPermission('api:clusters:queries');

    const signature = [
      canList,
      canCreate,
      canUpdate,
      canDelete,
      canActivate,
      canViewActive,
      canViewBackends,
      canViewFrontends,
      canViewQuery,
    ]
      .map(flag => (flag ? '1' : '0'))
      .join('');

    const signatureChanged = signature !== this.permissionSignature;
    this.permissionSignature = signature;

    this.canListClusters = canList;
    this.canCreateCluster = canCreate;
    this.canUpdateCluster = canUpdate;
    this.canDeleteCluster = canDelete;
    this.canActivateCluster = canActivate;
    this.canViewActiveCluster = canViewActive;
    this.canViewBackends = canViewBackends;
    this.canViewFrontends = canViewFrontends;
    this.canViewQueries = canViewQuery;
    this.hasClusterAccess = this.canListClusters;

    if (!this.hasClusterAccess) {
      this.loading = false;
      this.clusters = [];
      this.cdr.markForCheck();
      return;
    }

    if (signatureChanged && this.canListClusters) {
      this.loadClusters();
    }
    this.cdr.markForCheck();
  }
}
