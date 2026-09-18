import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, AfterViewInit, ChangeDetectionStrategy, ChangeDetectorRef, inject } from '@angular/core';
import { Router } from '@angular/router';
import { Subject, interval } from 'rxjs';
import { takeUntil, skip } from 'rxjs/operators';
import { NbActionsModule, NbAlertModule, NbBadgeModule, NbButtonModule, NbCardModule, NbIconModule, NbOptionModule, NbProgressBarModule, NbSelectModule, NbSpinnerModule, NbThemeService, NbToastrService, NbTooltipModule } from '@nebular/theme';
import { CountUp } from 'countup.js';
import {
  OverviewService,
  ExtendedClusterOverview,
  HealthCard,
  PerformanceTrends,
  ResourceTrends,
  DataStatistics,
  CapacityPrediction,
  TopTableBySize,
  TopTableByAccess,
  CompactionDetailStats,
  TopPartitionByScore,
} from '../../../@core/data/overview.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { I18nService } from '../../../@core/i18n/i18n.service';
import { AuthService } from '../../../@core/data/auth.service';
import { NodeService, Session } from '../../../@core/data/node.service';
import { themeChartChrome, colorWithAlpha } from '../../../@core/utils/theme-color';
import { CommonModule } from '@angular/common';
import { NgxEchartsDirective } from 'ngx-echarts';
import { donutOption, horizontalBarOption } from './overview-charts';

@Component({
    selector: 'ngx-cluster-overview',
    templateUrl: './cluster-overview.component.html',
    styleUrls: ['./cluster-overview.component.scss'],
    changeDetection: ChangeDetectionStrategy.OnPush,
    imports: [
    TranslatePipe,
    NbCardModule,
    NbBadgeModule,
    NbIconModule,
    NbActionsModule,
    NbSelectModule,
    NbOptionModule,
    NbButtonModule,
    NbSpinnerModule,
    NbTooltipModule,
    NgxEchartsDirective,
    NbAlertModule,
    CommonModule,
    NbProgressBarModule
],
})
export class ClusterOverviewComponent implements OnInit, OnDestroy, AfterViewInit {
  private overviewService = inject(OverviewService);
  private i18n = inject(I18nService);
  private clusterContext = inject(ClusterContextService);
  private router = inject(Router);
  private toastr = inject(NbToastrService);
  private themeService = inject(NbThemeService);
  private authService = inject(AuthService);
  private nodeService = inject(NodeService);
  private cdr = inject(ChangeDetectorRef);

  overview: ExtendedClusterOverview | null = null;
  healthCards: HealthCard[] = [];
  performanceTrends: PerformanceTrends | null = null;
  resourceTrends: ResourceTrends | null = null;
  dataStatistics: DataStatistics | null = null;
  capacityPrediction: CapacityPrediction | null = null;
  compactionDetails: CompactionDetailStats | null = null;
  
  activeSessions: number = 0;
  runningQueries: number = 0;
  activeUsers1h: number = 0;
  activeUsers24h: number = 0;
  
  timeRange: string = '1h';
  loading = false;
  sessionsReady = false;
  statsReady = false;
  autoRefresh = false; // Default: disabled (will be enabled when interval is selected)
  refreshInterval: number | 'off' = 'off'; // Default: off (Grafana style)

  // Latency percentile selection
  selectedLatencyPercentile: 'P50' | 'P95' | 'P99' = 'P99';
  latencyPercentileOptions = [
    { label: 'P50 (中位数)', value: 'P50' },
    { label: 'P95', value: 'P95' },
    { label: 'P99 (长尾)', value: 'P99' }
  ];
  
  // Disk/Cache metric selection
  
  // Expose Math to template
  Math = Math;
  
  // Nebular theme colors (dynamically loaded from theme)
  chartColors: any = {};
  
  private destroy$ = new Subject<void>();
  private refreshTick$ = new Subject<void>();
  private loadSeq = 0;
  trendCharts: Array<{ title: string; options: Record<string, unknown>; hasData: boolean; facts?: string[] }> = [];
  taskChartOptions: Record<string, unknown> | null = null;
  topSizeChartOptions: Record<string, unknown> | null = null;
  topAccessChartOptions: Record<string, unknown> | null = null;
  storageChartOptions: Record<string, unknown> | null = null;

  // Time range options
  timeRangeOptions = [
    { label: '1小时', value: '1h' },
    { label: '6小时', value: '6h' },
    { label: '24小时', value: '24h' },
    { label: '3天', value: '3d' },
  ];

  // Refresh interval options (Grafana style)
  refreshIntervalOptions = [
    { label: '手动', value: 'off' },
    { label: '15秒', value: 15 },
    { label: '30秒', value: 30 },
    { label: '1分钟', value: 60 },
  ];

  ngOnInit() {
    // Load Nebular theme colors
    this.themeService.getJsTheme()
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => {
        this.chartColors = themeChartChrome();
        this.rebuildChartOptions();
        this.cdr.markForCheck();
      });

    // Initialize overview data loading
    this.loadOverview();
    this.setupAutoRefresh();

    // 语言切换后重建健康卡片：标题/描述由 i18n.instant 生成，需重新求值
    this.i18n.lang$.pipe(takeUntil(this.destroy$)).subscribe(() => this.refreshCards(false));

    // Listen to active cluster changes
    this.clusterContext.activeCluster$
      .pipe(
        skip(1), // Skip initial value
        takeUntil(this.destroy$)
      )
      .subscribe(() => {
        this.overview = null;
        this.dataStatistics = null;
        this.compactionDetails = null;
        this.sessionsReady = false;
        this.statsReady = false;
        this.loadOverview();
        this.setupAutoRefresh();
        this.cdr.markForCheck();
      });
  }

  ngAfterViewInit() {
    // Animate numbers after view is initialized
    setTimeout(() => this.animateNumbers(), 100);
  }

  ngOnDestroy() {
    this.refreshTick$.next();
    this.refreshTick$.complete();
    this.destroy$.next();
    this.destroy$.complete();
  }

  setupAutoRefresh() {
    this.refreshTick$.next();
    if (typeof this.refreshInterval === 'number' && this.refreshInterval > 0) {
      interval(this.refreshInterval * 1000)
        .pipe(
          takeUntil(this.destroy$),
          takeUntil(this.refreshTick$),
        )
        .subscribe(() => {
          if (!this.authService.isAuthenticated()) {
            this.autoRefresh = false;
            this.refreshInterval = 'off';
            this.refreshTick$.next();
            return;
          }
          if (this.autoRefresh) {
            this.loadOverview(false, false);
          }
        });
    }
  }

  loadOverview(showLoading: boolean = true, includeDeferred: boolean = true) {
    // Guard: never fire requests after logout (token already cleared)
    if (!this.authService.isAuthenticated()) {
      return;
    }
    const seq = ++this.loadSeq;
    if (showLoading && !this.overview) {
      this.loading = true;
      this.cdr.markForCheck();
    }
    if (includeDeferred) {
      this.sessionsReady = false;
      this.statsReady = false;
    }

    this.overviewService.getExtendedClusterOverview(this.timeRange)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (overview) => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.applyCore(overview);
          this.loading = false;
          this.cdr.markForCheck();
          if (includeDeferred) {
            this.loadDeferred(seq);
          }
        },
        error: (err) => {
          if (seq !== this.loadSeq) {
            return;
          }
          // Silently ignore errors after logout (in-flight request returning 401)
          if (!this.authService.isAuthenticated()) {
            return;
          }
          let errorMsg = '加载集群概览失败';
          if (err.error?.message) {
            errorMsg = err.error.message;
          } else if (err.status === 0) {
            errorMsg = '无法连接到服务器';
          } else if (err.status === 404) {
            errorMsg = '未找到数据或未激活集群';
          } else if (err.status === 401) {
            errorMsg = '没有权限执行此操作';
          }
          this.toastr.danger(errorMsg, '错误');
          this.loading = false;
          this.cdr.markForCheck();
        }
      });
  }

  private applyCore(overview: ExtendedClusterOverview) {
    this.overview = overview;
    this.performanceTrends = overview.performance_trends;
    this.resourceTrends = overview.resource_trends;
    this.capacityPrediction = overview.capacity || null;
    if (!this.statsReady) {
      this.dataStatistics = null;
    }
    this.refreshCards();
  }

  private refreshCards(rebuildChartOptions: boolean = true) {
    if (!this.overview) {
      return;
    }
    this.healthCards = this.overviewService.transformToHealthCards(this.overview);
    if (rebuildChartOptions) {
      this.rebuildChartOptions();
    }
  }

  private rebuildChartOptions(): void {
    this.taskChartOptions = this.getTaskChartOptions();
    this.topSizeChartOptions = this.getTopSizeChartOptions();
    this.topAccessChartOptions = this.getTopAccessChartOptions();
    this.storageChartOptions = this.getStorageChartOptions();
    this.trendCharts = this.getTrendCharts();
  }

  private loadDeferred(seq: number) {
    this.overviewService.getActiveDataStatistics(this.timeRange)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (raw) => {
          if (seq !== this.loadSeq || !this.overview) {
            return;
          }
          const stats = this.overviewService.mapDataStatistics(raw);
          this.dataStatistics = stats;
          this.overview.data_stats = raw;
          this.overview.mv_stats = {
            total: stats.mvTotal,
            running: stats.mvRunning,
            success: stats.mvSuccess,
            failed: stats.mvFailed,
            pending: 0,
          };
          this.overview.sessions = {
            ...this.overview.sessions,
            active_users_1h: stats.activeUsers1h,
            active_users_24h: stats.activeUsers24h,
          };
          if (this.overview.capacity) {
            this.overview.capacity.real_data_size_bytes = stats.totalDataSizeBytes;
          }
          this.activeUsers1h = stats.activeUsers1h;
          this.activeUsers24h = stats.activeUsers24h;
          this.statsReady = true;
          this.refreshCards();
          this.cdr.markForCheck();
        },
        error: () => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.statsReady = false;
          this.cdr.markForCheck();
        }
      });

    this.overviewService.getCompactionDetailStats(this.timeRange)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (details) => {
          if (seq !== this.loadSeq || !this.overview) {
            return;
          }
          this.compactionDetails = details;
          const running = details.taskStats?.runningCount
            ?? (details as any).task_stats?.running_count
            ?? 0;
          this.overview.compaction = {
            ...this.overview.compaction,
            cumulativeCompactionRunning: running,
          };
          this.refreshCards();
          this.cdr.markForCheck();
        },
        error: () => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.compactionDetails = null;
          this.cdr.markForCheck();
        }
      });

    this.nodeService.getSessions()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (sessions) => {
          if (seq !== this.loadSeq || !this.overview) {
            return;
          }
          const running = sessions.filter(session => this.isRunningQuery(session));
          this.overview.sessions = {
            ...this.overview.sessions,
            current_connections: sessions.length,
            running_queries: running.map(session => ({
              queryId: session.id,
              user: session.user,
              database: session.db || '',
              startTime: '',
              durationMs: (parseInt(session.time, 10) || 0) * 1000,
              state: session.state,
              queryPreview: (session.info || '').slice(0, 200),
            })),
          };
          this.activeSessions = sessions.length;
          this.runningQueries = running.length;
          this.sessionsReady = true;
          this.refreshCards(false);
          this.cdr.markForCheck();
        },
        error: () => {
          if (seq !== this.loadSeq) {
            return;
          }
          this.sessionsReady = false;
          this.cdr.markForCheck();
        }
      });
  }

  private isRunningQuery(session: Session): boolean {
    const info = session.info || '';
    const timeSecs = parseInt(session.time, 10) || 0;
    return (session.command === 'Query' || session.state === 'Query')
      && info.length > 0
      && timeSecs > 1
      && !info.startsWith('SHOW');
  }

  onTimeRangeChange(range: string) {
    this.timeRange = range;
    this.loadOverview();
  }

  // Grafana-style: selecting an interval automatically enables auto-refresh
  // Selecting 'off' disables auto-refresh
  onRefreshIntervalChange(interval: number | 'off') {
    this.refreshInterval = interval;
    
    if (interval === 'off') {
      // Disable auto-refresh
      this.autoRefresh = false;
      this.setupAutoRefresh(); // This will clear the interval
    } else {
      // Enable auto-refresh with selected interval
      this.autoRefresh = true;
      this.setupAutoRefresh();
    }
  }

  onManualRefresh() {
    this.loadOverview();
  }

  get overviewAlerts() {
    return this.overview?.alerts || [];
  }

  get capacityPercent(): number {
    return Math.max(0, Math.min(100, Number((this.capacityPrediction?.disk_usage_pct || 0).toFixed(1))));
  }

  get capacityTone(): string {
    // shared-data：本地盘是数据缓存配额，写满是 LRU 淘汰的稳态，不能上报警色
    if (this.isSharedData) {
      return '';
    }
    if (this.capacityPercent >= 90) {
      return 'danger';
    }
    if (this.capacityPercent >= 70) {
      return 'warning';
    }
    return '';
  }

  get taskRows(): Array<{ label: string; value: string; tone: string }> {
    if (!this.overview) {
      return [];
    }
    const loadFailed = this.overview.load_jobs?.failed || 0;
    const mvFailed = this.dataStatistics?.mvFailed ?? this.overview.mv_stats?.failed ?? 0;
    const schemaFailed = this.dataStatistics?.schemaChangeFailed ?? this.overview.schema_changes?.failed ?? 0;
    const unfinished = this.overview.unfinished_query ?? this.overview.sessions?.running_queries?.length ?? 0;
    const rows = [
      { label: '导入失败', value: String(loadFailed), tone: loadFailed > 0 ? 'danger' : '' },
      { label: 'MV失败', value: String(mvFailed), tone: mvFailed > 0 ? 'danger' : '' },
      { label: 'Schema失败', value: String(schemaFailed), tone: schemaFailed > 0 ? 'danger' : '' },
      { label: '未完成查询', value: String(unfinished), tone: unfinished > 50 ? 'warning' : '' },
      { label: '会话', value: String(this.overview.sessions?.current_connections || 0), tone: '' },
    ];
    if (this.isSharedData) {
      const running = this.overview.compaction?.cumulativeCompactionRunning || 0;
      rows.unshift({ label: '压缩运行', value: String(running), tone: running > 0 ? 'warning' : '' });
    }
    return rows;
  }

  private getTaskChartOptions(): Record<string, unknown> | null {
    if (!this.overview) {
      return null;
    }
    const items = [
      {
        name: '导入',
        value: this.overview.load_jobs?.running || 0,
        color: this.chartColors.info,
      },
      {
        name: 'MV',
        value: this.dataStatistics?.mvRunning ?? this.overview.mv_stats?.running ?? 0,
        color: this.chartColors.warning,
      },
      {
        name: 'Schema',
        value: this.dataStatistics?.schemaChangeRunning ?? this.overview.schema_changes?.running ?? 0,
        color: this.chartColors.primary,
      },
    ];
    if (this.isSharedData) {
      items.push({
        name: '压缩',
        value: this.overview.compaction?.cumulativeCompactionRunning || 0,
        color: this.chartColors.danger,
      });
    }
    const total = items.reduce((sum, item) => sum + item.value, 0);
    if (total <= 0) {
      return null;
    }
    return donutOption(this.chartColors, items, { value: String(total), name: '运行中' });
  }

  cardTitle(card: HealthCard): string {
    return card.title;
  }

  cardValue(card: HealthCard): string {
    return this.joinValueUnit(String(card.value ?? ''), card.unit);
  }

  private joinValueUnit(value: string, unit?: string): string {
    if (!unit) {
      return value;
    }
    if (unit === '%' || unit.startsWith('%')) {
      return `${value}%`;
    }
    if (unit === '个' || unit === '人') {
      return `${value}${unit}`;
    }
    return `${value} ${unit}`;
  }

  cardStatus(card: HealthCard): string {
    return card.status;
  }

  cardTooltip(card: HealthCard): string {
    return card.description || '';
  }

  handleCardClick(card: HealthCard) {
    if (card.cardId === 'compaction_score') {
      this.navigateToCompactions();
      return;
    }
    this.navigateToCard(card);
  }

  isNavigableCard(card: HealthCard): boolean {
    return card.cardId === 'compaction_score' || Boolean(card.navigateTo);
  }

  navigateToCard(card: HealthCard) {
    if (card.navigateTo) {
      this.router.navigate([card.navigateTo]);
    }
  }

  navigateToQueries() {
    this.router.navigate(['/pages/starrocks/queries/execution']);
  }

  navigateToBackends() {
    this.router.navigate(['/pages/starrocks/backends']);
  }

  navigateToMaterializedViews() {
    this.router.navigate(['/pages/starrocks/materialized-views']);
  }

  // Computed properties for template optimization (avoid repeated array operations)
  get topTablesBySize(): any[] {
    return (this.dataStatistics?.topTablesBySize || []).slice(0, 10);
  }

  get topTablesByAccess(): any[] {
    return (this.dataStatistics?.topTablesByAccess || []).slice(0, 10);
  }

  get accessEmptyText(): string {
    if (!this.dataStatistics) {
      return '暂无数据';
    }
    return this.dataStatistics.accessError || '暂无访问记录';
  }

  get isSharedData(): boolean {
    const mode = this.overview?.deployment_mode || this.clusterContext.getActiveCluster()?.deployment_mode;
    return mode === 'shared_data';
  }

  get storagePanelTitle(): string {
    return this.isSharedData ? 'Compaction' : 'BE磁盘';
  }

  get compactionPartitions(): TopPartitionByScore[] {
    return this.compactionDetails?.topPartitions || [];
  }

  get nodeDisks(): Array<{ host: string; usedPct: number }> {
    return this.compactionDetails?.nodeDisks || [];
  }

  get compactionDurationFacts(): string[] {
    const duration = this.compactionDetails?.durationStats;
    if (!duration || ((duration.minDurationMs ?? 0) <= 0 && (duration.maxDurationMs ?? 0) <= 0)) {
      return [];
    }
    return [
      `最小 ${this.formatDurationString(duration.minDurationMs)}`,
      `最大 ${this.formatDurationString(duration.maxDurationMs)}`,
      `平均 ${this.formatDurationString(duration.avgDurationMs)}`,
    ];
  }

  private getCompactionChartOptions(): Record<string, unknown> | null {
    if (this.compactionPartitions.length) {
      return this.getCompactionScoreChartOptions();
    }
    const tasks = this.compactionDetails?.taskStats;
    const running = tasks?.runningCount ?? 0;
    const finished = tasks?.finishedCount ?? 0;
    if (running + finished <= 0) {
      return null;
    }
    return donutOption(
      this.chartColors,
      [
        { name: '运行中', value: running, color: this.chartColors.warning },
        { name: '已完成', value: finished, color: this.chartColors.info },
      ],
      running > 0
        ? { value: String(running), name: '运行中' }
        : { value: String(finished), name: '已完成' },
    );
  }

  private getStorageChartOptions(): Record<string, unknown> | null {
    if (this.isSharedData) {
      return this.getCompactionChartOptions();
    }
    if (!this.nodeDisks.length) {
      return null;
    }
    return horizontalBarOption(
      this.chartColors,
      this.nodeDisks.map(node => ({ name: node.host, value: node.usedPct })),
      this.chartColors.warning,
      value => `${value.toFixed(1)}%`,
      100,
    );
  }

  private getTrendCharts(): Array<{ title: string; options: Record<string, unknown>; hasData: boolean; facts?: string[] }> {
    const tablet = this.resourceTrends?.tablet_count;
    const tabletFact = tablet?.length
      ? [`Tablet ${Math.round(tablet[tablet.length - 1].value)}`]
      : [];
    const charts = [
      { title: '吞吐', options: this.getQpsChartOptions(), hasData: !!this.performanceTrends?.qps?.length },
      { title: '延迟', options: this.getLatencyChartOptions(), hasData: !!this.performanceTrends?.latency_p99?.length },
      { title: '错误', options: this.getErrorRateChartOptions(), hasData: !!this.performanceTrends?.error_rate?.length },
      {
        title: '资源',
        options: this.getResourceChartOptions(),
        hasData: !!this.resourceTrends?.cpu_usage?.length,
        facts: tabletFact,
      },
      { title: 'JVM', options: this.getJvmHeapChartOptions(), hasData: !!this.resourceTrends?.jvm_heap_usage?.length },
      { title: 'Score', options: this.getCompactionScoreTrendOptions(), hasData: !!this.resourceTrends?.compaction_score?.length },
    ];
    if (this.isSharedData) {
      return [
        ...charts,
        { title: '超时', options: this.getTimeoutRateChartOptions(), hasData: !!this.performanceTrends?.timeout_rate?.length },
        {
          title: '事务',
          options: this.getTxnChartOptions(),
          hasData: !!(this.resourceTrends?.txn_success?.length || this.resourceTrends?.txn_failed?.length),
        },
      ];
    }
    return [
      ...charts,
      { title: '网络', options: this.getNetworkChartOptions(), hasData: !!this.resourceTrends?.network_tx?.length },
      { title: 'IO', options: this.getIoChartOptions(), hasData: !!this.resourceTrends?.io_read?.length },
    ];
  }

  private getTopSizeChartOptions(): Record<string, unknown> | null {
    if (!this.topTablesBySize.length) {
      return null;
    }
    return horizontalBarOption(
      this.chartColors,
      this.topTablesBySize.map(table => ({
        name: `${table.database}.${table.table}`,
        value: table.sizeBytes,
      })),
      this.chartColors.primary,
      value => this.formatBytes(value),
    );
  }

  private getTopAccessChartOptions(): Record<string, unknown> | null {
    if (!this.topTablesByAccess.length) {
      return null;
    }
    return horizontalBarOption(
      this.chartColors,
      this.topTablesByAccess.map(table => ({
        name: `${table.database}.${table.table}`,
        value: table.accessCount,
      })),
      this.chartColors.info,
      value => this.formatNumber(value),
    );
  }

  private getCompactionScoreChartOptions(): Record<string, unknown> {
    const partitions = this.compactionPartitions;
    if (!partitions.length) {
      return {};
    }
    return horizontalBarOption(
      this.chartColors,
      partitions.map(partition => ({
        name: `${partition.tableName}.${partition.partitionName}`,
        value: partition.maxScore,
      })),
      this.chartColors.warning,
      value => value.toFixed(1),
    );
  }

  // Helper methods
  
  /**
   * Format bytes to human-readable size string (with unit)
   * Legacy method for backward compatibility
   */
  formatBytes(bytes: number): string {
    const formatted = this.formatBytesAdaptive(bytes);
    return `${formatted.value} ${formatted.unit}`;
  }
  
  /**
   * Format bytes to human-readable size with adaptive unit
   * 自适应单位显示：大于1024T显示P，大于1024G显示T，以此类推
   */
  formatBytesAdaptive(bytes: number): { value: string; unit: string } {
    if (bytes === 0) return { value: '0', unit: 'B' };
    
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    const k = 1024;
    
    let unitIndex = 0;
    let value = bytes;
    
    while (value >= k && unitIndex < units.length - 1) {
      value /= k;
      unitIndex++;
    }
    
    // Format value: show 1 decimal place for values < 10, otherwise round
    const formattedValue = value < 10 ? value.toFixed(1) : Math.round(value).toString();
    
    return {
      value: formattedValue,
      unit: units[unitIndex]
    };
  }
  
  /**
   * Format milliseconds to human-readable duration with adaptive unit
   * 自适应时间单位：大于60s显示分钟，大于60分显示小时，以此类推
   */
  formatDuration(ms: number): { value: string; unit: string } {
    if (ms === 0) return { value: '0', unit: 'ms' };
    if (ms < 0) return { value: '0', unit: 'ms' };
    
    const units = [
      { name: 'ms', threshold: 1000 },
      { name: 's', threshold: 60 },
      { name: '分', threshold: 60 },
      { name: '时', threshold: 24 },
      { name: '天', threshold: 7 },
      { name: '周', threshold: Number.MAX_VALUE }
    ];
    
    let value = ms;
    let unitIndex = 0;
    
    // Convert ms to seconds first
    if (value >= 1000) {
      value /= 1000;
      unitIndex = 1;
    }
    
    // Then convert through other units
    while (unitIndex < units.length - 1 && value >= units[unitIndex].threshold) {
      value /= units[unitIndex].threshold;
      unitIndex++;
    }
    
    // Format value: show 1 decimal place for values < 10, otherwise round
    const formattedValue = value < 10 ? value.toFixed(1) : Math.round(value).toString();
    
    return {
      value: formattedValue,
      unit: units[unitIndex].name
    };
  }

  /**
   * Format number with K/M suffix (legacy method for backward compatibility)
   */
  formatNumber(num: number): string {
    if (num >= 1000000) {
      return (num / 1000000).toFixed(2) + 'M';
    } else if (num >= 1000) {
      return (num / 1000).toFixed(2) + 'K';
    }
    return num.toString();
  }

  /**
   * Format duration as string (legacy method for backward compatibility)
   * Use formatDuration() for adaptive unit formatting
   */
  formatDurationString(ms: number): string {
    const formatted = this.formatDuration(ms);
    return `${formatted.value}${formatted.unit}`;
  }

  getStatusIcon(status: string): string {
    switch (status) {
      case 'success': return 'checkmark-circle-2-outline';
      case 'warning': return 'alert-triangle-outline';
      case 'danger': return 'close-circle-outline';
      case 'info': return 'info-outline';
      default: return 'info-outline';
    }
  }

  getTrendIcon(trend: number): string {
    if (trend > 0) return 'trending-up-outline';
    if (trend < 0) return 'trending-down-outline';
    return 'minus-outline';
  }

  getTrendColor(trend: number): string {
    if (trend > 0) return 'success';
    if (trend < 0) return 'danger';
    return 'basic';
  }

  // Get latency value based on selected percentile
  getSelectedLatencyValue(): number {
    if (!this.performanceTrends) return 0;
    
    const trends = this.performanceTrends;
    switch (this.selectedLatencyPercentile) {
      case 'P50':
        return trends.latency_p50?.length > 0 
          ? trends.latency_p50[trends.latency_p50.length - 1].value 
          : 0;
      case 'P95':
        return trends.latency_p95?.length > 0 
          ? trends.latency_p95[trends.latency_p95.length - 1].value 
          : 0;
      case 'P99':
        return trends.latency_p99?.length > 0 
          ? trends.latency_p99[trends.latency_p99.length - 1].value 
          : 0;
      default:
        return 0;
    }
  }

  getSelectedLatencyStatus(): string {
    const latency = this.getSelectedLatencyValue();
    if (latency < 1000) return 'success';
    if (latency < 5000) return 'warning';
    return 'danger';
  }

  onLatencyPercentileChange(percentile: 'P50' | 'P95' | 'P99'): void {
    this.selectedLatencyPercentile = percentile;
  }

  // Cycle through latency percentiles on click
  cycleLatencyPercentile(): void {
    const options: Array<'P50' | 'P95' | 'P99'> = ['P50', 'P95', 'P99'];
    const currentIndex = options.indexOf(this.selectedLatencyPercentile);
    const nextIndex = (currentIndex + 1) % options.length;
    this.selectedLatencyPercentile = options[nextIndex];
  }

  getLatencyCardTitle(): string {
    return this.selectedLatencyPercentile;
  }
  
  // ECharts Configuration Methods (使用 ngx-admin 兼容的渐变样式)

  /**
   * Generate unified series configuration for smooth line chart with area fill
   * 模仿StreamLake的轻量级风格：极细线条 + 极淡面积填充
   */
  private getLineSeries(name: string, data: any[], color: string, withArea: boolean = true): any {
    const baseSeries = {
      name,
      type: 'line',
      smooth: true,
      symbol: 'circle',
      symbolSize: 0,  // Hide symbols by default
      showSymbol: false,
      sampling: 'lttb',
      lineStyle: { 
        width: 1.5,  // Very thin line like StreamLake
        color,
      },
      emphasis: {
        focus: 'series',
        lineStyle: {
          width: 2,
        },
      },
      data,
    };

    if (withArea) {
      return {
        ...baseSeries,
        areaStyle: {
          color: {
            type: 'linear',
            x: 0,
            y: 0,
            x2: 0,
            y2: 1,
            colorStops: [
              { offset: 0, color: this.hexToRgba(color, 0.12) },
              { offset: 1, color: this.hexToRgba(color, 0.01) },
            ],
          },
        },
      };
    }

    return baseSeries;
  }

  private getBaseChartOptions(_color: string): any {
    return {
      grid: {
        left: 8,
        right: 12,
        bottom: 4,
        top: 28,
        containLabel: true,
      },
      tooltip: {
        trigger: 'axis',
        axisPointer: {
          type: 'line',
          lineStyle: {
            color: this.chartColors.border,
            width: 1,
          },
        },
        backgroundColor: this.chartColors.cardBg,
        borderColor: this.chartColors.border,
        borderWidth: 1,
        textStyle: {
          color: this.chartColors.textBasic,
          fontSize: 11,
        },
        padding: [8, 12],
      },
      legend: {
        top: 0,
        left: 0,
        icon: 'circle',
        itemWidth: 8,
        itemHeight: 8,
        textStyle: {
          color: this.chartColors.textHint,
          fontSize: 11,
        },
      },
      xAxis: {
        type: 'category',
        boundaryGap: false,
        axisLabel: {
          color: this.chartColors.textHint,
          fontSize: 11,
        },
        axisLine: { show: false },
        axisTick: { show: false },
      },
      yAxis: {
        type: 'value',
        splitNumber: 3,
        axisLabel: {
          color: this.chartColors.textHint,
          fontSize: 11,
        },
        axisLine: { show: false },
        axisTick: { show: false },
        splitLine: {
          lineStyle: {
            color: this.chartColors.border,
            width: 1,
          },
        },
      },
    };
  }

  getErrorRateChartOptions(): any {
    const points = this.performanceTrends?.error_rate;
    if (!points?.length) {
      return {};
    }
    const color = this.chartColors.danger || '#ff3d71';
    return {
      ...this.getBaseChartOptions(color),
      legend: {
        ...this.getBaseChartOptions(color).legend,
        data: ['错误率'],
      },
      tooltip: {
        ...this.getBaseChartOptions(color).tooltip,
        formatter: (params: any) => {
          const point = params[0];
          return `${point.axisValue}<br/>${point.marker} 错误率: ${point.value.toFixed(2)}%`;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: points.map(d => new Date(d.timestamp).toLocaleTimeString()),
      },
      yAxis: {
        ...this.getBaseChartOptions(color).yAxis,
        axisLabel: {
          ...this.getBaseChartOptions(color).yAxis.axisLabel,
          formatter: (value: number) => `${value}%`,
        },
      },
      series: [
        this.getLineSeries('错误率', points.map(d => d.value), color),
      ],
    };
  }

  getTimeoutRateChartOptions(): any {
    const points = this.performanceTrends?.timeout_rate;
    if (!points?.length) {
      return {};
    }
    const color = this.chartColors.warning || '#ffaa00';
    return {
      ...this.getBaseChartOptions(color),
      legend: {
        ...this.getBaseChartOptions(color).legend,
        data: ['超时率'],
      },
      tooltip: {
        ...this.getBaseChartOptions(color).tooltip,
        formatter: (params: any) => {
          const point = params[0];
          return `${point.axisValue}<br/>${point.marker} 超时率: ${point.value.toFixed(2)}%`;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: points.map(d => new Date(d.timestamp).toLocaleTimeString()),
      },
      yAxis: {
        ...this.getBaseChartOptions(color).yAxis,
        axisLabel: {
          ...this.getBaseChartOptions(color).yAxis.axisLabel,
          formatter: (value: number) => `${value}%`,
        },
      },
      series: [
        this.getLineSeries('超时率', points.map(d => d.value), color),
      ],
    };
  }

  getTxnChartOptions(): any {
    const success = this.resourceTrends?.txn_success;
    const failed = this.resourceTrends?.txn_failed;
    if (!success?.length && !failed?.length) {
      return {};
    }
    const axis = success?.length ? success : failed;
    const successColor = this.chartColors.success || '#00d68f';
    const failedColor = this.chartColors.danger || '#ff3d71';
    return {
      ...this.getBaseChartOptions(successColor),
      legend: {
        ...this.getBaseChartOptions(successColor).legend,
        data: ['成功', '失败'],
      },
      tooltip: {
        ...this.getBaseChartOptions(successColor).tooltip,
        formatter: (params: any) => {
          let result = `${params[0].axisValue}<br/>`;
          params.forEach((param: any) => {
            result += `${param.marker} ${param.seriesName}: ${Math.round(param.value)}<br/>`;
          });
          return result;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(successColor).xAxis,
        data: axis.map(d => new Date(d.timestamp).toLocaleTimeString()),
      },
      series: [
        this.getLineSeries('成功', (success || []).map(d => d.value), successColor),
        this.getLineSeries('失败', (failed || []).map(d => d.value), failedColor, false),
      ],
    };
  }

  getCompactionScoreTrendOptions(): any {
    const points = this.resourceTrends?.compaction_score;
    if (!points?.length) {
      return {};
    }
    const color = this.chartColors.warning || '#ffaa00';
    const series = this.getLineSeries('Score', points.map(d => d.value), color);
    return {
      ...this.getBaseChartOptions(color),
      legend: {
        ...this.getBaseChartOptions(color).legend,
        data: ['Score'],
      },
      tooltip: {
        ...this.getBaseChartOptions(color).tooltip,
        formatter: (params: any) => {
          const point = params[0];
          return `${point.axisValue}<br/>${point.marker} Score: ${point.value.toFixed(1)}`;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: points.map(d => new Date(d.timestamp).toLocaleTimeString()),
      },
      series: [
        {
          ...series,
          markLine: {
            silent: true,
            symbol: 'none',
            lineStyle: { color: this.chartColors.danger, type: 'dashed', width: 1 },
            data: [{ yAxis: 100, label: { formatter: '100', color: this.chartColors.danger } }],
          },
        },
      ],
    };
  }

  getQpsChartOptions(): any {
    if (!this.performanceTrends || !this.performanceTrends.qps || !this.performanceTrends.rps) {
      return {};
    }

    const qpsData = this.performanceTrends.qps;
    const rpsData = this.performanceTrends.rps;
    const times = qpsData.map(d => new Date(d.timestamp).toLocaleTimeString());
    const qpsValues = qpsData.map(d => d.value);
    const rpsValues = rpsData.map(d => d.value);
    const qpsColor = this.chartColors.primary || '#3366ff';
    const rpsColor = this.chartColors.success || '#00d68f';

    return {
      ...this.getBaseChartOptions(qpsColor),
      legend: {
        ...this.getBaseChartOptions(qpsColor).legend,
        data: ['QPS', 'RPS'],
      },
      xAxis: {
        ...this.getBaseChartOptions(qpsColor).xAxis,
        data: times,
      },
      series: [
        this.getLineSeries('QPS', qpsValues, qpsColor),
        this.getLineSeries('RPS', rpsValues, rpsColor),
      ],
    };
  }

  getLatencyChartOptions(): any {
    if (!this.performanceTrends || !this.performanceTrends.latency_p50 || 
        !this.performanceTrends.latency_p95 || !this.performanceTrends.latency_p99) {
      return {};
    }

    const p50Data = this.performanceTrends.latency_p50;
    const p95Data = this.performanceTrends.latency_p95;
    const p99Data = this.performanceTrends.latency_p99;
    const times = p50Data.map(d => new Date(d.timestamp).toLocaleTimeString());
    const p50Values = p50Data.map(d => d.value);
    const p95Values = p95Data.map(d => d.value);
    const p99Values = p99Data.map(d => d.value);
    const p50Color = this.chartColors.success || '#00d68f';
    const p95Color = this.chartColors.warning || '#ffaa00';
    const p99Color = this.chartColors.danger || '#ff3d71';

    return {
      ...this.getBaseChartOptions(p99Color),
      legend: {
        ...this.getBaseChartOptions(p99Color).legend,
        data: ['P50', 'P95', 'P99'],
      },
      tooltip: {
        ...this.getBaseChartOptions(p99Color).tooltip,
        formatter: (params: any) => {
          let result = params[0].axisValue + '<br/>';
          params.forEach((item: any) => {
            result += `${item.marker} ${item.seriesName}: ${item.value.toFixed(0)} ms<br/>`;
          });
          return result;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(p99Color).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(p99Color).yAxis,
        axisLabel: {
          ...this.getBaseChartOptions(p99Color).yAxis.axisLabel,
          formatter: (value: number) => value.toFixed(0),
        },
      },
      series: [
        this.getLineSeries('P50', p50Values, p50Color, false),
        this.getLineSeries('P95', p95Values, p95Color, false),
        this.getLineSeries('P99', p99Values, p99Color, true),
      ],
    };
  }

  getCpuChartOptions(): any {
    if (!this.resourceTrends || !this.resourceTrends.cpu_usage) {
      return {};
    }

    const data = this.resourceTrends.cpu_usage;
    const times = data.map(d => new Date(d.timestamp).toLocaleTimeString());
    const values = data.map(d => d.value);
    const color = this.chartColors.success || '#00d68f';

    return {
      ...this.getBaseChartOptions(color),
      tooltip: {
        trigger: 'axis',
        backgroundColor: this.chartColors.cardBg,
        borderColor: color,
        textStyle: { color: this.chartColors.textBasic },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(color).yAxis,
        max: 100,
      },
      series: [
        this.getLineSeries('CPU Usage (%)', values, color, true),
      ],
    };
  }

  getMemoryChartOptions(): any {
    if (!this.resourceTrends || !this.resourceTrends.memory_usage) {
      return {};
    }

    const data = this.resourceTrends.memory_usage;
    const times = data.map(d => new Date(d.timestamp).toLocaleTimeString());
    const values = data.map(d => d.value);
    const color = this.chartColors.info || '#0095ff';

    return {
      ...this.getBaseChartOptions(color),
      tooltip: {
        trigger: 'axis',
        backgroundColor: this.chartColors.cardBg,
        borderColor: color,
        textStyle: { color: this.chartColors.textBasic },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(color).yAxis,
        max: 100,
      },
      series: [
        this.getLineSeries('Memory Usage (%)', values, color, true),
      ],
    };
  }

  getDiskChartOptions(): any {
    if (!this.resourceTrends || !this.resourceTrends.disk_usage) {
      return {};
    }

    const data = this.resourceTrends.disk_usage;
    const times = data.map(d => new Date(d.timestamp).toLocaleTimeString());
    const values = data.map(d => d.value);
    const color = this.chartColors.warning || '#ffaa00';

    return {
      ...this.getBaseChartOptions(color),
      tooltip: {
        trigger: 'axis',
        backgroundColor: this.chartColors.cardBg,
        borderColor: color,
        textStyle: { color: this.chartColors.textBasic },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(color).yAxis,
        max: 100,
      },
      series: [
        this.getLineSeries('Disk Usage (%)', values, color, true),
      ],
    };
  }

  getJvmHeapChartOptions(): any {
    if (!this.resourceTrends || !this.resourceTrends.jvm_heap_usage) {
      return {};
    }

    const data = this.resourceTrends.jvm_heap_usage;
    const times = data.map(d => new Date(d.timestamp).toLocaleTimeString());
    const values = data.map(d => d.value);
    const color = this.chartColors.info || '#0095ff';
    const threadColor = this.chartColors.warning || '#ffaa00';
    const threads = this.resourceTrends.jvm_thread_count || [];
    const series = this.getLineSeries('JVM', values, color, true);
    const threadSeries = threads.length
      ? {
          ...this.getLineSeries('线程', threads.map(point => point.value), threadColor, false),
          yAxisIndex: 1,
        }
      : null;

    return {
      ...this.getBaseChartOptions(color),
      legend: {
        ...this.getBaseChartOptions(color).legend,
        data: threadSeries ? ['JVM', '线程'] : ['JVM'],
      },
      tooltip: {
        ...this.getBaseChartOptions(color).tooltip,
        formatter: (params: any) => {
          let result = `${params[0].axisValue}<br/>`;
          params.forEach((param: any) => {
            const suffix = param.seriesName === 'JVM' ? '%' : '';
            result += `${param.marker} ${param.seriesName}: ${Number(param.value).toFixed(1)}${suffix}<br/>`;
          });
          return result;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(color).xAxis,
        data: times,
      },
      yAxis: [
        {
          ...this.getBaseChartOptions(color).yAxis,
          max: 100,
          axisLabel: {
            ...this.getBaseChartOptions(color).yAxis.axisLabel,
            formatter: (value: number) => `${value}%`,
          },
        },
        {
          ...this.getBaseChartOptions(color).yAxis,
          splitLine: { show: false },
        },
      ],
      series: [
        {
          ...series,
          markLine: {
            silent: true,
            symbol: 'none',
            lineStyle: { color: this.chartColors.danger, type: 'dashed', width: 1 },
            data: [{ yAxis: 80, label: { formatter: '80%', color: this.chartColors.danger } }],
          },
        },
        ...(threadSeries ? [threadSeries] : []),
      ],
    };
  }

  getResourceChartOptions(): any {
    if (!this.resourceTrends ||
        !this.resourceTrends.cpu_usage ||
        !this.resourceTrends.memory_usage ||
        !this.resourceTrends.disk_usage) {
      return {};
    }

    const times = this.resourceTrends.cpu_usage.map(d => new Date(d.timestamp).toLocaleTimeString());
    const cpuColor = this.chartColors.primary || '#3366ff';
    const memoryColor = this.chartColors.danger || '#ff3d71';
    const diskColor = this.chartColors.success || '#00d68f';

    return {
      ...this.getBaseChartOptions(cpuColor),
      legend: {
        ...this.getBaseChartOptions(cpuColor).legend,
        data: ['CPU', '内存', '磁盘'],
      },
      tooltip: {
        ...this.getBaseChartOptions(cpuColor).tooltip,
        formatter: (params: any) => {
          let result = params[0].axisValue + '<br/>';
          params.forEach((param: any) => {
            result += `${param.marker} ${param.seriesName}: ${param.value.toFixed(1)}%<br/>`;
          });
          return result;
        },
      },
      xAxis: {
        ...this.getBaseChartOptions(cpuColor).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(cpuColor).yAxis,
        max: 100,
        axisLabel: {
          ...this.getBaseChartOptions(cpuColor).yAxis.axisLabel,
          formatter: '{value}%',
        },
      },
      series: [
        this.getLineSeries('CPU', this.resourceTrends.cpu_usage.map(d => d.value), cpuColor),
        this.getLineSeries('内存', this.resourceTrends.memory_usage.map(d => d.value), memoryColor),
        this.getLineSeries('磁盘', this.resourceTrends.disk_usage.map(d => d.value), diskColor),
      ],
    };
  }

  getNetworkChartOptions(): any {
    if (!this.resourceTrends?.network_tx?.length || !this.resourceTrends?.network_rx?.length) {
      return {};
    }

    const txData = this.resourceTrends.network_tx;
    const rxData = this.resourceTrends.network_rx;
    const times = txData.map(d => new Date(d.timestamp).toLocaleTimeString());
    const txValues = txData.map(d => d.value / 1024 / 1024); // Convert to MB/s
    const rxValues = rxData.map(d => d.value / 1024 / 1024);
    
    const txColor = this.chartColors.primary || '#3366ff';
    const rxColor = this.chartColors.success || '#00d68f';

    return {
      ...this.getBaseChartOptions(txColor),
      xAxis: {
        ...this.getBaseChartOptions(txColor).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(txColor).yAxis,
      },
      legend: {
        data: ['TX (Send)', 'RX (Receive)'],
        textStyle: { color: this.chartColors.textBasic },
      },
      tooltip: {
        trigger: 'axis',
        backgroundColor: this.chartColors.cardBg,
        textStyle: { color: this.chartColors.textBasic },
      },
      series: [
        {
          name: 'TX (Send)',
          type: 'line',
          smooth: true,
          symbol: 'circle',
          symbolSize: 6,
          sampling: 'lttb',
          itemStyle: {
            color: txColor,
          },
          areaStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 0,
              y2: 1,
              colorStops: [
                { offset: 0, color: this.hexToRgba(txColor, 0.3) },
                { offset: 1, color: this.hexToRgba(txColor, 0.05) },
              ],
            },
          },
          data: txValues,
        },
        {
          name: 'RX (Receive)',
          type: 'line',
          smooth: true,
          symbol: 'circle',
          symbolSize: 6,
          sampling: 'lttb',
          itemStyle: {
            color: rxColor,
          },
          areaStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 0,
              y2: 1,
              colorStops: [
                { offset: 0, color: this.hexToRgba(rxColor, 0.3) },
                { offset: 1, color: this.hexToRgba(rxColor, 0.05) },
              ],
            },
          },
          data: rxValues,
        },
      ],
    };
  }

  getIoChartOptions(): any {
    if (!this.resourceTrends?.io_read?.length || !this.resourceTrends?.io_write?.length) {
      return {};
    }

    const readData = this.resourceTrends.io_read;
    const writeData = this.resourceTrends.io_write;
    const times = readData.map(d => new Date(d.timestamp).toLocaleTimeString());
    const readValues = readData.map(d => d.value / 1024 / 1024); // Convert to MB/s
    const writeValues = writeData.map(d => d.value / 1024 / 1024);
    
    const readColor = this.chartColors.success || '#00d68f';
    const writeColor = this.chartColors.warning || '#ffaa00';

    return {
      ...this.getBaseChartOptions(readColor),
      xAxis: {
        ...this.getBaseChartOptions(readColor).xAxis,
        data: times,
      },
      yAxis: {
        ...this.getBaseChartOptions(readColor).yAxis,
      },
      legend: {
        data: ['Read', 'Write'],
        textStyle: { color: this.chartColors.textBasic },
      },
      tooltip: {
        trigger: 'axis',
        backgroundColor: this.chartColors.cardBg,
        textStyle: { color: this.chartColors.textBasic },
      },
      series: [
        {
          name: 'Read',
          type: 'line',
          smooth: true,
          symbol: 'circle',
          symbolSize: 6,
          sampling: 'lttb',
          itemStyle: {
            color: readColor,
          },
          areaStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 0,
              y2: 1,
              colorStops: [
                { offset: 0, color: this.hexToRgba(readColor, 0.3) },
                { offset: 1, color: this.hexToRgba(readColor, 0.05) },
              ],
            },
          },
          data: readValues,
        },
        {
          name: 'Write',
          type: 'line',
          smooth: true,
          symbol: 'circle',
          symbolSize: 6,
          sampling: 'lttb',
          itemStyle: {
            color: writeColor,
          },
          areaStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 0,
              y2: 1,
              colorStops: [
                { offset: 0, color: this.hexToRgba(writeColor, 0.3) },
                { offset: 1, color: this.hexToRgba(writeColor, 0.05) },
              ],
            },
          },
          data: writeValues,
        },
      ],
    };
  }

  getTrendLabel(trend: string): string {
    const labels: { [key: string]: string } = {
      'increasing': '增长中',
      'decreasing': '下降中',
      'stable': '稳定'
    };
    return labels[trend] || trend;
  }

  /**
   * Animate numbers using CountUp.js for visual appeal
   */
  private animateNumbers() {
    // Animate health card numbers
    this.healthCards.forEach((card, index) => {
      const element = document.getElementById(`card-value-${index}`);
      if (element && card.value) {
        // Extract numeric value from card.value (may include text like "8/8")
        const numericValue = this.extractNumericValue(card.value);
        if (numericValue !== null) {
          const countUp = new CountUp(element, numericValue, {
            duration: 2,
            useEasing: true,
            separator: ',',
            decimal: '.',
            decimalPlaces: this.getDecimalPlaces(numericValue),
          });
          if (!countUp.error) {
            countUp.start();
          }
        }
      }
    });

    // Animate data statistics numbers
    if (this.dataStatistics) {
      this.animateDataStatElement('stat-databases', this.dataStatistics.databaseCount);
      this.animateDataStatElement('stat-tables', this.dataStatistics.tableCount);
      this.animateDataStatElement('stat-data-size', this.dataStatistics.totalDataSizeBytes / (1024 * 1024 * 1024 * 1024)); // TB
    }
  }

  private animateDataStatElement(elementId: string, value: number) {
    const element = document.getElementById(elementId);
    if (element) {
      const countUp = new CountUp(element, value, {
        duration: 2,
        useEasing: true,
        separator: ',',
        decimal: '.',
        decimalPlaces: this.getDecimalPlaces(value),
      });
      if (!countUp.error) {
        countUp.start();
      }
    }
  }

  private extractNumericValue(value: string | number): number | null {
    // Handle cases like "8/8", "75%", "1234", "2.5"
    if (typeof value === 'number') {
      return value;
    }
    
    const match = value.toString().match(/^(\d+\.?\d*)/);
    return match ? parseFloat(match[1]) : null;
  }

  private getDecimalPlaces(value: number): number {
    if (value >= 100) return 0;
    if (value >= 10) return 1;
    return 2;
  }

  /**
   * Convert hex color to RGBA
   * Used for chart gradient effects with Nebular theme colors
   */
  private hexToRgba(hex: string, alpha: number): string {
    return colorWithAlpha(hex || this.chartColors.primary, alpha);
  }

  /**
   * Get CSS class for compaction score status indication
   * 
   * @param score Compaction score value
   * @returns CSS class name for styling
   */
  getScoreStatusClass(score: number): string {
    if (score > 100) return 'text-danger';   // Critical
    if (score > 50) return 'text-warning';   // Warning
    return 'text-success';                   // Normal
  }

  // 导航到compaction列表页面
  navigateToCompactions() {
    this.router.navigate(['/pages/starrocks/system'], { 
      queryParams: { 
        function: 'compactions',
        from: 'overview'  // 标记来源，用于返回功能
      } 
    });
  }

  navigateToLoadJobs(event?: Event) {
    if (event) {
      event.stopPropagation();
    }
    this.router.navigate(['/pages/starrocks/system'], { 
      queryParams: { 
        function: 'loads',
        from: 'overview'  // 标记来源，用于返回功能
      } 
    });
  }

  navigateToSessions() {
    this.router.navigate(['/pages/starrocks/sessions']);
  }
}
