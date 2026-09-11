import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule } from '@nebular/theme';
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
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbSpinnerModule,
    Angular2SmartTableModule
],
})
export class BackendsComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService);
  private clusterService = inject(ClusterService);
  private clusterContext = inject(ClusterContextService);
  private toastrService = inject(NbToastrService);
  private confirmDialogService = inject(ConfirmDialogService);

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
    noDataMessage: '暂无计算节点数据',
    actions: {
      add: false,
      edit: false,
      delete: true,
      position: 'right',
    },
    delete: {
      deleteButtonContent: '<i class="nb-trash"></i>',
      confirmDelete: true,
    },
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      BackendId: { title: '节点ID', type: 'string' },
      IP: { title: '主机地址', type: 'string' },
      HeartbeatPort: { title: '心跳端口', type: 'string' },
      BePort: { title: '服务端口', type: 'string' },
      HttpPort: { title: 'HTTP端口', type: 'string' },
      Alive: {
        title: '状态',
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string) => {
          const online = value === 'true';
          const status = online ? 'success' : 'danger';
          const text = online ? '在线' : '离线';
          return `<span class="badge badge-${status}">${text}</span>`;
        },
      },
      Version: { title: '版本', type: 'string' },
      TabletNum: { title: 'Tablet数', type: 'string' },
      DataUsedCapacity: { title: '已用容量', type: 'string' },
      TotalCapacity: { title: '总容量', type: 'string' },
      UsedPct: {
        title: '使用率',
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.diskThresholds),
      },
      CpuCores: { title: 'CPU核数', type: 'string' },
      CpuUsedPct: {
        title: 'CPU使用率',
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.cpuThresholds),
      },
      MemLimit: { title: '内存限制', type: 'string' },
      MemUsedPct: {
        title: '内存使用率',
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.memoryThresholds),
      },
      NumRunningQueries: { title: '运行查询', type: 'string' },
      LastHeartbeat: { title: '最后心跳', type: 'string' },
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
        const switched = this.clusterId !== 0 && this.clusterId !== cluster.id;
        this.clusterId = cluster.id;
        if (switched) {
          this.loadBackends();
        }
      });
    this.loadBackends();
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
          void assignTableRows(this.source, backends);
          this.loading = false;
        },
        error: (error) => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.handleClusterError(error),
            '错误',
          );
          void assignTableRows(this.source, []);
          this.loading = false;
        },
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
