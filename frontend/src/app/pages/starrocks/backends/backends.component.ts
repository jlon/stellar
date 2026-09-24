import { CommonModule, DOCUMENT } from '@angular/common';
import { ChangeDetectorRef, Component, EventEmitter, OnDestroy, OnInit, Output, TemplateRef, ViewChild, inject } from '@angular/core';
import { TranslatePipe } from '@ngx-translate/core';
import { Subject } from 'rxjs';
import { take, takeUntil, timeout } from 'rxjs/operators';
import {
  NbAlertModule,
  NbButtonModule,
  NbCardModule,
  NbDialogModule,
  NbDialogRef,
  NbDialogService,
  NbIconModule,
  NbSpinnerModule,
  NbToastrService,
  NbTooltipModule,
} from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';

import { I18nService } from '../../../@core/i18n/i18n.service';
import {
  Backend,
  BackendDiagnosticRequest,
  BackendDiagnosticResponse,
  BackendMemoryTracker,
  BlockingDriver,
  NodeService,
} from '../../../@core/data/node.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { HasPermissionDirective } from '../../../@core/directives/has-permission.directive';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { MetricThresholds, renderMetricBadge } from '../../../@core/utils/metric-badge';
import { assignTableRows } from '../../../@core/utils/table-rows';

@Component({
  selector: 'ngx-backend-actions-cell',
  template: `
    <div class="backend-actions-cell">
      <button nbButton ghost size="tiny" status="info" ngxHasPermission="api:clusters:backends:diagnose"
        [nbTooltip]="'诊断' | translate" [attr.aria-label]="'诊断' | translate" (click)="openDiagnostics($event)">
        <nb-icon icon="activity-outline"></nb-icon>
      </button>
    </div>
  `,
  imports: [TranslatePipe, NbButtonModule, NbIconModule, NbTooltipModule, HasPermissionDirective],
})
export class BackendActionsCellComponent implements OnDestroy {
  backend: Backend | null = null;
  @Output() diagnose = new EventEmitter<Backend>();
  readonly destroyed$ = new Subject<void>();

  openDiagnostics(event: Event): void {
    event.stopPropagation();
    if (this.backend) {
      this.diagnose.emit(this.backend);
    }
  }

  ngOnDestroy(): void {
    this.destroyed$.next();
    this.destroyed$.complete();
  }
}

@Component({
  selector: 'ngx-backends',
  templateUrl: './backends.component.html',
  styleUrls: ['./backends.component.scss'],
  imports: [
    CommonModule,
    TranslatePipe,
    NbAlertModule,
    NbButtonModule,
    NbCardModule,
    NbDialogModule,
    NbIconModule,
    NbSpinnerModule,
    NbTooltipModule,
    Angular2SmartTableModule,
  ],
})
export class BackendsComponent implements OnInit, OnDestroy {
  private static readonly sheetExitDurationMs = 180;
  private static readonly memoryTrackerDescriptions: Readonly<Record<string, string>> = {
    query_pool: '正在执行的查询及其执行算子占用的内存。',
    load: '导入任务处理过程占用的内存。',
    metadata: '存储元数据（如 Tablet 和 Rowset 信息）占用的内存。',
    compaction: '本地 Compaction 任务合并数据文件和版本时占用的内存。',
    schema_change: '数据结构变更任务占用的内存。',
    page_cache: '节点内存中的数据页缓存，用于复用近期读取的数据页。',
    update: '数据更新处理过程占用的内存。',
    clone: '副本克隆或补数任务占用的内存。',
    consistency: '数据一致性校验任务占用的内存。',
    datacache: 'Data Cache 模块在内存中维护缓存数据时占用的内存。',
    jit_cache: '表达式即时编译生成的缓存机器码占用的内存。',
    replication: '副本同步或复制任务占用的内存。',
    jemalloc_metadata: 'jemalloc 内存分配器维护分配记录和空闲页时占用的元数据内存。',
    brpc_iobuf: 'BRPC 网络请求和响应缓冲区占用的内存。',
  };
  private static readonly blockingDriverStateLabels: Readonly<Record<string, string>> = {
    INPUT_EMPTY: '等待上游数据',
    OUTPUT_FULL: '等待下游释放缓冲',
    PRECONDITION_BLOCK: '等待前置条件',
    PENDING_FINISH: '等待后台 I/O 收尾',
    EPOCH_PENDING_FINISH: '等待当前处理周期收尾',
    LOCAL_WAITING: '本地等待可用数据',
  };
  private static readonly blockingDriverStateDescriptions: Readonly<Record<string, string>> = {
    INPUT_EMPTY: '上游算子尚未产出可继续处理的数据。',
    OUTPUT_FULL: '下游算子的输出缓冲尚未腾出空间。',
    PRECONDITION_BLOCK: '执行所需的前置条件尚未满足。',
    PENDING_FINISH: 'Sink 已完成，但仍有后台 I/O 任务等待结束。',
    EPOCH_PENDING_FINISH: '当前流式处理周期仍在等待收尾。',
    LOCAL_WAITING: '引擎暂在工作线程内等待可用数据。',
  };

  private readonly document = inject(DOCUMENT);
  private readonly nodeService = inject(NodeService);
  private readonly i18n = inject(I18nService);
  private readonly clusterContext = inject(ClusterContextService);
  private readonly toastrService = inject(NbToastrService);
  private readonly dialogService = inject(NbDialogService);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly destroy$ = new Subject<void>();
  private readonly diagnosticRequestsCancelled$ = new Subject<void>();
  private loadSeq = 0;
  private diagnosticSeq = 0;
  private detailDialogRef?: NbDialogRef<unknown>;
  private detailClusterId = 0;
  private sheetClosing = false;
  private readonly diskThresholds: MetricThresholds = { warn: 70, danger: 85 };
  private readonly cpuThresholds: MetricThresholds = { warn: 60, danger: 85 };
  private readonly memoryThresholds: MetricThresholds = { warn: 65, danger: 85 };

  @ViewChild('detailDialog') private detailDialog?: TemplateRef<unknown>;

  source: LocalDataSource = new LocalDataSource();
  clusterId = 0;
  activeCluster: Cluster | null = null;
  clusterName = '';
  deploymentMode = '';
  pageTitle = 'Backend 节点';
  loading = true;
  diagnosticLoading = false;
  diagnostic: BackendDiagnosticResponse | null = null;
  diagnosticError = '';
  selectedBackend: Backend | null = null;

  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: this.i18n.instant('暂无计算节点数据'),
    actions: false,
    pager: { display: true, perPage: 15 },
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
          return `<span class="badge badge-${online ? 'success' : 'danger'}">${online ? '在线' : '离线'}</span>`;
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
      Actions: {
        title: this.i18n.instant('操作'),
        type: 'custom',
        width: '7%',
        isFilterable: false,
        isSortable: false,
        renderComponent: BackendActionsCellComponent,
        componentInitFunction: (instance: BackendActionsCellComponent, cell: any) => {
          instance.backend = cell.getRow().getData() as Backend;
          instance.diagnose.pipe(takeUntil(instance.destroyed$)).subscribe((backend) => this.openDetails(backend));
        },
      },
    },
  };

  constructor() {
    this.clusterId = this.clusterContext.getActiveClusterId() || 0;
  }

  get isSharedData(): boolean {
    return this.deploymentMode === 'shared_data';
  }

  get nodeType(): string {
    return this.isSharedData ? 'CN' : 'BE';
  }

  get storageLabel(): string {
    return this.isSharedData ? '数据缓存' : '本地存储';
  }

  get memoryTrackers(): BackendMemoryTracker[] {
    return this.diagnostic?.memory.data?.top_trackers ?? [];
  }

  get blockingDrivers(): BlockingDriver[] {
    return this.diagnostic?.blocking_drivers?.data?.drivers ?? [];
  }

  memoryTrackerDescription(name: string): string {
    return BackendsComponent.memoryTrackerDescriptions[name]
      ?? `StarRocks 引擎内部内存分类“${name}”；当前接口未提供更细说明。`;
  }

  blockingDriverStateLabel(state: string): string {
    const normalized = state.trim();
    return BackendsComponent.blockingDriverStateLabels[normalized]
      ? `${BackendsComponent.blockingDriverStateLabels[normalized]}（${normalized}）`
      : `引擎状态（${normalized || 'UNKNOWN'}）`;
  }

  blockingDriverDescription(driver: BlockingDriver): string {
    const normalized = driver.state.trim();
    const description = BackendsComponent.blockingDriverStateDescriptions[normalized]
      ?? '当前接口只报告该执行步骤处于阻塞队列，未提供具体等待原因。';
    const fragmentStatus = driver.fragment_status
      ? ` 执行片段状态：${driver.fragment_status}。`
      : '';
    return `${description}${fragmentStatus}`;
  }

  blockingDriverCountText(queryCount: number, driverCount: number, displayedCount: number): string {
    const suffix = driverCount > displayedCount ? `；当前显示前 ${displayedCount} 个` : '';
    return `已发现 ${queryCount} 条查询中的 ${driverCount} 个等待步骤${suffix}`;
  }

  ngOnInit(): void {
    this.clusterContext.activeCluster$.pipe(takeUntil(this.destroy$)).subscribe((cluster) => {
      this.activeCluster = cluster;
      if (!cluster) {
        return;
      }
      if (this.detailDialogRef && cluster.id !== this.detailClusterId) {
        this.closeDetails();
      }
      this.applyCluster(cluster);
      const switched = this.clusterId !== cluster.id;
      this.clusterId = cluster.id;
      if (switched) {
        this.loadBackends();
      }
    });
    if (this.clusterId) {
      this.loadBackends();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
    this.diagnosticRequestsCancelled$.next();
    this.diagnosticRequestsCancelled$.complete();
    const dialogRef = this.detailDialogRef;
    this.detailDialogRef = undefined;
    dialogRef?.close();
  }

  applyCluster(cluster: Cluster): void {
    this.clusterName = cluster.name;
    this.deploymentMode = cluster.deployment_mode || 'shared_nothing';
    this.pageTitle = this.isSharedData ? 'Compute Nodes (CN)' : 'Backend Nodes (BE)';
    this.settings = {
      ...this.settings,
      columns: {
        ...this.settings.columns,
        TabletNum: { ...this.settings.columns.TabletNum, title: this.i18n.instant(this.isSharedData ? '缓存分片' : 'Tablet数') },
        DataUsedCapacity: { ...this.settings.columns.DataUsedCapacity, title: this.i18n.instant(this.isSharedData ? '缓存已用' : '已用容量') },
        TotalCapacity: { ...this.settings.columns.TotalCapacity, title: this.i18n.instant(this.isSharedData ? '缓存配额' : '总容量') },
        UsedPct: { ...this.settings.columns.UsedPct, title: this.i18n.instant(this.isSharedData ? '缓存使用率' : '使用率') },
      },
    };
  }

  loadBackends(): void {
    const seq = ++this.loadSeq;
    this.loading = true;
    this.nodeService.listBackends().pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: (backends) => {
        if (seq === this.loadSeq) {
          this.renderRows(backends);
        }
      },
      error: (error) => {
        if (seq !== this.loadSeq) {
          return;
        }
        this.toastrService.danger(ErrorHandler.handleClusterError(error), '错误');
        this.renderRows([]);
      },
    });
  }

  openDetails(backend: Backend): void {
    if (!this.detailDialog || !this.clusterId || this.sheetClosing) {
      return;
    }
    this.diagnosticRequestsCancelled$.next();
    this.diagnosticSeq++;
    this.detailClusterId = this.clusterId;
    this.selectedBackend = backend;
    this.diagnostic = null;
    this.diagnosticError = '';
    this.diagnosticLoading = false;
    const dialogRef = this.dialogService.open(this.detailDialog, {
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: false,
      dialogClass: 'side-sheet',
      hasBackdrop: true,
      hasScroll: true,
    });
    this.detailDialogRef = dialogRef;
    dialogRef.onBackdropClick.pipe(take(1)).subscribe(() => this.closeDetails(dialogRef));
    dialogRef.onClose.pipe(take(1)).subscribe(() => this.onDetailsClosed(dialogRef));
    this.document.defaultView?.requestAnimationFrame(() => {
      this.document.querySelector<HTMLElement>('.cdk-overlay-pane.side-sheet .backend-detail-sheet')?.focus({ preventScroll: true });
    });
    this.loadDiagnostics();
  }

  loadDiagnostics(include?: 'blocking_drivers' | 'compaction'): void {
    const backend = this.selectedBackend;
    if (!backend || !this.detailClusterId || (include === 'compaction' && this.isSharedData)) {
      return;
    }
    const seq = ++this.diagnosticSeq;
    this.diagnosticLoading = true;
    this.diagnosticError = '';
    const request: BackendDiagnosticRequest = {
      cluster_id: this.detailClusterId,
      backend_id: backend.BackendId,
      host: backend.IP,
      heartbeat_port: backend.HeartbeatPort,
      http_port: backend.HttpPort,
      ...(include ? { include } : {}),
    };
    this.nodeService.getBackendDiagnostics(request)
      .pipe(takeUntil(this.destroy$), takeUntil(this.diagnosticRequestsCancelled$), timeout(20000))
      .subscribe({
        next: (diagnostic) => {
          if (seq !== this.diagnosticSeq) {
            return;
          }
          this.diagnostic = diagnostic;
          this.diagnosticLoading = false;
          this.cdr.markForCheck();
        },
        error: (error) => {
          if (seq !== this.diagnosticSeq) {
            return;
          }
          this.diagnosticLoading = false;
          this.diagnosticError = ErrorHandler.handleClusterError(error);
          this.cdr.markForCheck();
        },
      });
  }

  closeDetails(ref = this.detailDialogRef): void {
    if (!ref || this.sheetClosing) {
      return;
    }
    this.diagnosticRequestsCancelled$.next();
    this.diagnosticSeq++;
    const pane = this.document.querySelector<HTMLElement>('.cdk-overlay-pane.side-sheet');
    if (!pane || this.prefersReducedMotion()) {
      ref.close();
      return;
    }
    this.sheetClosing = true;
    pane.classList.add('side-sheet--closing');
    this.document.querySelector<HTMLElement>('.cdk-overlay-backdrop.side-sheet-backdrop')?.classList.add('side-sheet-backdrop--closing');
    this.document.defaultView?.setTimeout(() => ref.close(), BackendsComponent.sheetExitDurationMs);
  }

  formatBytes(bytes: number | null | undefined): string {
    if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) {
      return '-';
    }
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = bytes;
    let index = 0;
    while (value >= 1024 && index < units.length - 1) {
      value /= 1024;
      index++;
    }
    return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
  }

  formatRate(rate: number | null | undefined): string {
    return rate === null || rate === undefined || !Number.isFinite(rate) ? '-' : `${rate.toFixed(1)}%`;
  }

  showValue(value?: string | null): string {
    return value?.trim() || '-';
  }

  private renderRows(backends: Backend[]): void {
    assignTableRows(this.source, backends).then(() => {
      this.loading = false;
      this.cdr.detectChanges();
      queueMicrotask(() => this.cdr.detectChanges());
    });
  }

  private onDetailsClosed(ref: NbDialogRef<unknown>): void {
    this.diagnosticRequestsCancelled$.next();
    this.diagnosticSeq++;
    if (this.detailDialogRef === ref) {
      this.detailDialogRef = undefined;
      this.selectedBackend = null;
      this.diagnostic = null;
      this.diagnosticError = '';
      this.diagnosticLoading = false;
      this.sheetClosing = false;
      this.cdr.markForCheck();
    }
  }

  private prefersReducedMotion(): boolean {
    return this.document.defaultView?.matchMedia('(prefers-reduced-motion: reduce)').matches ?? false;
  }
}
