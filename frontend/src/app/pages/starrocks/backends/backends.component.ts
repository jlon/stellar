import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { ChangeDetectorRef, Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { NodeService, Backend } from '../../../@core/data/node.service';
import { Cluster, ClusterService } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { MetricThresholds, renderMetricBadge } from '../../../@core/utils/metric-badge';
import { assignTableRows } from '../../../@core/utils/table-rows';


@Component({
    selector: 'ngx-backends',
    templateUrl: './backends.component.html',
    styleUrls: ['./backends.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbSpinnerModule,
    NbTooltipModule,
    Angular2SmartTableModule
],
})
export class BackendsComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService)
  private i18n = inject(I18nService);
  private clusterService = inject(ClusterService);
  private clusterContext = inject(ClusterContextService);
  private toastrService = inject(NbToastrService);
  private confirmDialogService = inject(ConfirmDialogService);
  private cdr = inject(ChangeDetectorRef);

  source: LocalDataSource = new LocalDataSource();
  clusterId = 0;
  activeCluster: Cluster | null = null;
  clusterName = '';
  deploymentMode = '';
  pageTitle = 'Backend 节点';
  loading = true;
  private destroy$ = new Subject<void>();
  private loadSeq = 0;
  private readonly diskThresholds: MetricThresholds = { warn: 70, danger: 85 };
  private readonly cpuThresholds: MetricThresholds = { warn: 60, danger: 85 };
  private readonly memoryThresholds: MetricThresholds = { warn: 65, danger: 85 };

  settings = {
    hideSubHeader: false,
    noDataMessage: this.i18n.instant('暂无计算节点数据'),
    actions: {
      add: false,
      edit: false,
      delete: true,
      position: 'right',
      columnTitle: this.i18n.instant('操作'),
    },
    delete: {
      deleteButtonContent: '<i class="nb-trash" title="删除"></i>',
      confirmDelete: true,
    },
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      BackendId: { title: this.i18n.instant('节点ID'), type: 'string' },
      IP: { title: this.i18n.instant('主机地址'), type: 'string' },
      HeartbeatPort: { title: this.i18n.instant('心跳端口'), type: 'string' },
      BePort: { title: this.i18n.instant('服务端口'), type: 'string' },
      HttpPort: { title: this.i18n.instant('HTTP端口'), type: 'string' },
      Alive: {
        title: this.i18n.instant('状态'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string) => {
          const online = value === 'true';
          const status = online ? 'success' : 'danger';
          const text = online ? '在线' : '离线';
          return `<span class="badge badge-${status}">${text}</span>`;
        },
      },
      Version: { title: this.i18n.instant('版本'), type: 'string' },
      TabletNum: { title: this.i18n.instant('Tablet数'), type: 'string' },
      DataUsedCapacity: { title: this.i18n.instant('已用容量'), type: 'string' },
      TotalCapacity: { title: this.i18n.instant('总容量'), type: 'string' },
      UsedPct: {
        title: this.i18n.instant('使用率'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.diskThresholds),
      },
      CpuCores: { title: this.i18n.instant('CPU核数'), type: 'string' },
      CpuUsedPct: {
        title: this.i18n.instant('CPU使用率'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.cpuThresholds),
      },
      MemLimit: { title: this.i18n.instant('内存限制'), type: 'string' },
      MemUsedPct: {
        title: this.i18n.instant('内存使用率'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.memoryThresholds),
      },
      NumRunningQueries: { title: this.i18n.instant('运行查询'), type: 'string' },
      LastHeartbeat: { title: this.i18n.instant('最后心跳'), type: 'string' },
    },
  };

  constructor() {
    // Get clusterId from ClusterContextService
    this.clusterId = this.clusterContext.getActiveClusterId() || 0;
  }

  ngOnInit(): void {
    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe(cluster => {
        this.activeCluster = cluster;
        if (!cluster) {
          return;
        }
        this.applyCluster(cluster);
        const switched = this.clusterId !== cluster.id;
        this.clusterId = cluster.id;
        if (switched) {
          this.loadBackends();
        }
      });
    // clusterId 为 0 说明活动集群尚未就绪，等 activeCluster$ 推送后再加载
    //（此前会打出无效请求；frontends 页同款修复）。
    if (this.clusterId) {
      this.loadBackends();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  applyCluster(cluster: Cluster): void {
    this.clusterName = cluster.name;
    this.deploymentMode = cluster.deployment_mode || 'shared_nothing';
    this.pageTitle = this.deploymentMode === 'shared_data'
      ? 'Compute Nodes (CN)'
      : 'Backend Nodes (BE)';
    const cache = this.deploymentMode === 'shared_data';
    this.settings = {
      ...this.settings,
      columns: {
        ...this.settings.columns,
        DataUsedCapacity: {
          ...this.settings.columns.DataUsedCapacity,
          title: this.i18n.instant(cache ? '缓存已用' : '已用容量'),
        },
        TotalCapacity: {
          ...this.settings.columns.TotalCapacity,
          title: this.i18n.instant(cache ? '缓存配额' : '总容量'),
        },
        UsedPct: {
          ...this.settings.columns.UsedPct,
          title: this.i18n.instant(cache ? '缓存使用率' : '使用率'),
        },
      },
    };
  }

  loadBackends(): void {
    const seq = ++this.loadSeq;
    this.loading = true;
    this.nodeService.listBackends()
      .pipe(takeUntil(this.destroy$), timeout(20000))
      .subscribe({
        next: (backends) => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.renderRows(backends);
        },
        error: (error) => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.handleClusterError(error),
            '错误',
          );
          this.renderRows([]);
        },
      });
  }

  private renderRows(backends: Backend[]): void {
    assignTableRows(this.source, backends).then(() => {
      this.loading = false;
      this.cdr.detectChanges();
      // Smart Table 在设置更新后异步重建数据集，需要下一微任务再次刷新视图。
      queueMicrotask(() => this.cdr.detectChanges());
    });
  }

  onDeleteConfirm(event: { data: Backend; confirm: { resolve: () => void; reject: () => void } }): void {
    const backend = event.data;
    const itemName = `${backend.IP}:${backend.HeartbeatPort}`;
    const nodeType = this.deploymentMode === 'shared_data' ? 'CN' : 'BE';
    this.confirmDialogService.confirmDelete(
      itemName,
      `删除${nodeType}节点前请确认数据已迁移、副本足够、节点已停服务`,
    ).subscribe(confirmed => {
      if (!confirmed) {
        event.confirm.reject();
        return;
      }
      this.nodeService.deleteBackend(backend.IP, backend.HeartbeatPort)
        .subscribe({
          next: () => {
            this.toastrService.success(`${nodeType} ${itemName} 已删除`, '成功');
            event.confirm.resolve();
            this.loadBackends();
          },
          error: (error) => {
            event.confirm.reject();
            this.toastrService.danger(
              ErrorHandler.extractErrorMessage(error),
              '删除失败',
            );
          },
        });
    });
  }
}
