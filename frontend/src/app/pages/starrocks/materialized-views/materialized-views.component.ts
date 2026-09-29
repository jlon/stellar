import { I18nService } from '../../../@core/i18n/i18n.service';
import { DatePipe, DOCUMENT } from '@angular/common';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, TemplateRef, ViewChild, ChangeDetectorRef, HostListener, inject } from '@angular/core';
import { forkJoin, of, Subject } from 'rxjs';
import { catchError, map, skip, take, takeUntil, timeout } from 'rxjs/operators';
import { NbToastrService, NbDialogRef, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbDatepickerModule, NbSelectModule, NbOptionModule, NbBadgeModule, NbSpinnerModule, NbTabsetModule, NbAlertModule, NbCheckboxModule, NbFormFieldModule, NbTooltipModule } from '@nebular/theme';
import { MarkdownModule } from 'ngx-markdown';
import { LocalDataSource, Angular2SmartTableModule, RowSelectionEvent } from 'angular2-smart-table';
import {
  MaterializedViewService,
  MaterializedView,
  MaterializedViewDependencies,
  MaterializedViewRef,
  CreateMaterializedViewRequest,
  RefreshMode,
  RefreshSchedule,
} from '../../../@core/data/materialized-view.service';
import { ClusterService, Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { AuthService } from '../../../@core/data/auth.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { assignTableRows } from '../../../@core/utils/table-rows';
import { ActiveToggleRenderComponent } from './active-toggle-render.component';
import { BadgeRenderComponent, BadgeInfo } from './badge-render.component';
import { MvOpportunitiesSheetComponent } from './mv-opportunities-sheet.component';
import { FormsModule } from '@angular/forms';
import * as dagre from 'dagre';
import {
  NodeService,
  SchemaColumn,
  SchemaObjectDetail,
  SchemaObjectKind,
  SchemaObjectSummary,
} from '../../../@core/data/node.service';


@Component({
    selector: 'ngx-materialized-views',
    templateUrl: './materialized-views.component.html',
    styleUrls: ['./materialized-views.component.scss'],
    providers: [MaterializedViewService],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbInputModule,
    NbDatepickerModule,
    FormsModule,
    DatePipe,
    NbSelectModule,
    NbOptionModule,
    NbBadgeModule,
    NbSpinnerModule,
    Angular2SmartTableModule,
    NbTabsetModule,
    NbAlertModule,
    NbCheckboxModule,
    NbFormFieldModule,
    NbTooltipModule,
    MarkdownModule,
],
})
export class MaterializedViewsComponent implements OnInit, OnDestroy {
  private static readonly sheetExitDurationMs = 180;

  private document = inject(DOCUMENT);
  private mvService = inject(MaterializedViewService)
  private i18n = inject(I18nService);
  private clusterService = inject(ClusterService);
  private clusterContextService = inject(ClusterContextService);
  private toastrService = inject(NbToastrService);
  private confirmDialogService = inject(ConfirmDialogService);
  private dialogService = inject(NbDialogService);
  private cdRef = inject(ChangeDetectorRef);
  private authService = inject(AuthService);
  private nodeService = inject(NodeService);

  @ViewChild('createDialog', { static: false }) createDialogTemplate: TemplateRef<any>;
  @ViewChild('detailDialog', { static: false }) detailDialogTemplate: TemplateRef<any>;
  @ViewChild('refreshDialog', { static: false }) refreshDialogTemplate: TemplateRef<any>;
  @ViewChild('editDialog', { static: false }) editDialogTemplate: TemplateRef<any>;

  source: LocalDataSource = new LocalDataSource();
  allMaterializedViews: MaterializedView[] = [];
  filteredMaterializedViews: MaterializedView[] = [];
  clusterId: number;
  activeCluster: Cluster | null = null;
  loading = true;
  private destroy$ = new Subject<void>();
  private createDialogClosed$ = new Subject<void>();
  private createTablesRequest = 0;
  private createColumnsRequest = 0;
  private detailRequest$ = new Subject<void>();
  private dependencyObjectRequest$ = new Subject<void>();
  private dependencyGraphCompact?: boolean;
  private detailTrigger?: HTMLElement;
  private sheetClosing = false;

  // Filter states
  searchText = '';
  selectedDatabase = 'all';
  selectedRefreshType = 'all';
  selectedActiveState = 'all';
  selectedRefreshState = 'all';
  showAdvancedFilters = false;

  // Advanced filters
  refreshTimeStart: Date | null = null;
  refreshTimeEnd: Date | null = null;
  rowCountMin: number | null = null;
  rowCountMax: number | null = null;
  selectedPartitionType = 'all';

  // Options for filters
  databases: string[] = [];
  refreshTypeOptions = [
    { value: 'all', label: this.i18n.instant('全部类型') },
    { value: 'ASYNC', label: this.i18n.instant('自动刷新') },
    { value: 'MANUAL', label: this.i18n.instant('手动刷新') },
    { value: 'ROLLUP', label: this.i18n.instant('同步') },
    { value: 'INCREMENTAL', label: this.i18n.instant('增量') },
  ];
  activeStateOptions = [
    { value: 'all', label: this.i18n.instant('全部') },
    { value: 'active', label: 'Active' },
    { value: 'inactive', label: 'Inactive' },
  ];
  refreshStateOptions = [
    { value: 'all', label: this.i18n.instant('全部') },
    { value: 'SUCCESS', label: this.i18n.instant('成功') },
    { value: 'RUNNING', label: this.i18n.instant('运行中') },
    { value: 'FAILED', label: this.i18n.instant('失败') },
    { value: 'PENDING', label: this.i18n.instant('等待中') },
  ];
  partitionTypeOptions = [
    { value: 'all', label: this.i18n.instant('全部分区类型') },
    { value: 'RANGE', label: 'RANGE' },
    { value: 'LIST', label: 'LIST' },
    { value: 'UNPARTITIONED', label: 'UNPARTITIONED' },
  ];

  // Dialog states
  createDialogRef: any;
  detailDialogRef?: NbDialogRef<unknown>;
  refreshDialogRef: any;
  editDialogRef: any;
  selectedMV: MaterializedView | null = null;
  mvDDL = '';
  dependencies: MaterializedViewDependencies | null = null;
  dependenciesLoading = false;
  dependencyObject: SchemaObjectDetail | null = null;
  dependencyObjectLoading = false;
  dependencyObjectReferenceWarning = '';
  private dependencyObjectRefs = new Map<string, string>();
  dependencyGraphNodes: Array<{
    id: string;
    label: string;
    subtitle: string;
    type: string;
    evidence?: string;
    catalog?: string;
    database?: string;
    objectName?: string;
    objectKind?: SchemaObjectKind;
    objectRef?: string;
    x: number;
    y: number;
    width: number;
    height: number;
  }> = [];
  dependencyGraphEdges: Array<{
    from: { x: number; y: number };
    to: { x: number; y: number };
    path: string;
    label: string;
    labelX: number;
    labelY: number;
  }> = [];
  dependencyGraphWidth = 600;
  dependencyGraphHeight = 250;

  // Create form
  createDatabase = '';
  createName = '';
  createSourceDatabase = '';
  createSourceTable = '';
  createColumns: string[] = [];
  createSchedule: 'manual' | 'scheduled' = 'manual';
  createScheduleInterval = '1';
  createScheduleUnit: 'hour' | 'day' = 'hour';
  createMode: 'guided' | 'sql' = 'guided';
  createQuerySql = '';
  createPartitionBy = '';
  createDistribution: 'default' | 'random' | 'hash' = 'default';
  createHashColumns = '';
  createBuckets = '';
  createSortColumns = '';
  createReplicationNum = '';
  createBuildImmediate = false;
  createStep: 'configure' | 'review' = 'configure';
  createPreview = '';
  createPreviewRequest: CreateMaterializedViewRequest | null = null;
  previewing = false;
  creating = false;
  createError = '';
  createDatabases: string[] = [];
  createSourceTables: SchemaObjectSummary[] = [];
  createAvailableColumns: SchemaColumn[] = [];
  createDatabasesLoading = false;
  createSourceTablesLoading = false;
  createColumnsLoading = false;

  // Refresh form
  refreshMode: RefreshMode = 'async';
  refreshForce = false;
  refreshPartitionStart = '';
  refreshPartitionEnd = '';
  refreshing = false;

  // Edit form
  editAction = 'rename'; // rename | refresh_strategy
  editNewName = '';
  editRefreshStrategy: 'manual' | 'scheduled' = 'manual';
  editRefreshInterval = '1';
  editRefreshUnit: 'hour' | 'day' = 'hour';
  editing = false;

  refreshModeOptions = [
    { value: 'async' as const, label: this.i18n.instant('异步模式') },
    { value: 'sync' as const, label: this.i18n.instant('同步模式') },
  ];

  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: this.i18n.instant('暂无物化视图数据'),
    actions: {
      columnTitle: this.i18n.instant('操作'),
      add: false,
      edit: false,
      delete: true,
      position: 'right',
    },
    selectMode: 'single',
    delete: {
      deleteButtonContent: '<i class="nb-trash" title="删除"></i>',
      confirmDelete: false,
    },
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      name: {
        title: this.i18n.instant('名称'),
        type: 'string',
        width: '12%',
      },
      database_name: {
        title: this.i18n.instant('数据库'),
        type: 'string',
        width: '10%',
      },
      mv_type: {
        title: this.i18n.instant('类型'),
        type: 'custom',
        width: '7%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent, cell: any) => {
          instance.getBadge = (_value: any, row: MaterializedView) => ({
            status: row?.kind === 'rollup' ? 'primary' : 'info',
            label: row?.kind === 'rollup' ? '同步' : '异步',
          });
          this.bindTableCell(instance, cell);
        },
      },
      refresh_type: {
        title: this.i18n.instant('刷新策略'),
        type: 'custom',
        width: '9%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent, cell: any) => {
          instance.getBadge = (value: string): BadgeInfo | null => {
            const map: Record<string, BadgeInfo> = {
              ASYNC: { status: 'success', label: this.i18n.instant('自动') },
              MANUAL: { status: 'info', label: this.i18n.instant('手动') },
              ROLLUP: { status: 'primary', label: this.i18n.instant('同步') },
              INCREMENTAL: { status: 'warning', label: this.i18n.instant('增量') },
            };
            return map[value] ?? null;
          };
          this.bindTableCell(instance, cell);
        },
      },
      is_active: {
        title: this.i18n.instant('状态'),
        type: 'custom',
        width: '12%',
        renderComponent: ActiveToggleRenderComponent,
        componentInitFunction: (instance: ActiveToggleRenderComponent, cell: any) => {
          instance.toggleActive.subscribe((rowData: any) => {
            this.toggleActiveState(rowData);
          });
          this.bindTableCell(instance, cell);
        },
      },
      last_refresh_state: {
        title: this.i18n.instant('刷新状态'),
        type: 'custom',
        width: '9%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent, cell: any) => {
          instance.getBadge = (value: string, row: MaterializedView): BadgeInfo | null => {
            if (row?.kind === 'rollup') return null;
            const map: Record<string, BadgeInfo> = {
              SUCCESS: { status: 'success', label: this.i18n.instant('成功') },
              RUNNING: { status: 'info', label: this.i18n.instant('运行中') },
              FAILED: { status: 'danger', label: this.i18n.instant('失败') },
              PENDING: { status: 'warning', label: this.i18n.instant('等待中') },
            };
            return map[value] ?? null;
          };
          this.bindTableCell(instance, cell);
        },
      },
      last_refresh_finished_time: {
        title: this.i18n.instant('最后刷新时间'),
        type: 'string',
        width: '15%',
        valuePrepareFunction: (value: string) => value || '-',
      },
      rows: {
        title: this.i18n.instant('行数'),
        type: 'string',
        width: '8%',
        valuePrepareFunction: (value: number) => {
          if (value === null || value === undefined) return '-';
          return this.formatNumber(value);
        },
      },
      partition_type: {
        title: this.i18n.instant('分区类型'),
        type: 'string',
        width: '8%',
        valuePrepareFunction: (value: string) => value || '-',
      },
      error_info: {
        title: this.i18n.instant('错误信息'),
        type: 'custom',
        width: '8%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent, cell: any) => {
          instance.getBadge = (_value: any, row: MaterializedView): BadgeInfo | null =>
            row?.last_refresh_error_message
              ? { status: 'danger', label: this.i18n.instant('错误'), tooltip: row.last_refresh_error_message }
              : null;
          this.bindTableCell(instance, cell);
        },
      },
    },
  };

  private bindTableCell(
    renderer: { setCell(value: unknown, rowData: unknown): void },
    cell: { getRawValue(): unknown; getRow(): { getData(): unknown } },
  ): void {
    renderer.setCell(cell.getRawValue(), cell.getRow().getData());
  }

  ngOnInit() {
    // Only fire requests when a cluster is active (backend rejects clusterId=0 anyway)
    const activeId = this.clusterContextService.getActiveClusterId();
    if (activeId) {
      this.clusterId = activeId;
      this.loadClusterInfo();
      this.loadMaterializedViews();
    }

    // Follow cluster switches; skip(1) avoids a duplicate load for the initial value
    this.clusterContextService.activeCluster$
      .pipe(skip(1), takeUntil(this.destroy$))
      .subscribe((cluster) => {
        this.activeCluster = cluster;
        if (cluster) {
          const newClusterId = cluster.id;
          if (this.clusterId !== newClusterId) {
            this.clusterId = newClusterId;
            this.loadClusterInfo();
            this.loadMaterializedViews();
          }
        }
      });
  }

  ngOnDestroy() {
    this.createDialogClosed$.next();
    this.createDialogClosed$.complete();
    this.createDialogRef?.close();
    this.createDialogRef = undefined;
    this.dependencyObjectRequest$.next();
    this.dependencyObjectRequest$.complete();
    this.detailRequest$.next();
    this.detailRequest$.complete();
    this.detailDialogRef?.close();
    this.destroy$.next();
    this.destroy$.complete();
  }

  @HostListener('window:resize')
  onViewportResize(): void {
    const compact = this.isCompactDependencyGraph();
    if (compact !== this.dependencyGraphCompact && this.selectedMV && this.dependencies) {
      this.buildDependencyGraph(this.selectedMV, this.dependencies);
    }
  }

  loadClusterInfo() {
    this.clusterService
      .getCluster(this.clusterId)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (cluster) => {
          this.activeCluster = cluster;
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '加载集群信息失败',
          );
        },
      });
  }

  loadMaterializedViews() {
    this.loading = true;
    this.mvService
      .getMaterializedViews()
      .pipe(takeUntil(this.destroy$), timeout(20000))
      .subscribe({
        next: (data) => {
          this.allMaterializedViews = data;
          this.extractDatabases();
          void this.applyFilters()
            .finally(() => this.finishLoading())
            .catch(() => undefined);
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.handleClusterError(error),
            '加载物化视图失败',
          );
          this.finishLoading();
        },
      });
  }

  private finishLoading(): void {
    this.loading = false;
    this.cdRef.detectChanges();
  }

  extractDatabases() {
    const dbSet = new Set<string>();
    this.allMaterializedViews.forEach((mv) => {
      if (mv && mv.database_name) {
        dbSet.add(mv.database_name);
      }
    });
    this.databases = Array.from(dbSet).sort();
  }

  applyFilters() {
    let filtered = [...this.allMaterializedViews];

    // Search filter
    if (this.searchText.trim()) {
      const searchLower = this.searchText.toLowerCase();
      filtered = filtered.filter(
        (mv) =>
          (mv.name && mv.name.toLowerCase().includes(searchLower)) ||
          (mv.database_name && mv.database_name.toLowerCase().includes(searchLower)),
      );
    }

    // Database filter
    if (this.selectedDatabase !== 'all') {
      filtered = filtered.filter((mv) => mv && mv.database_name === this.selectedDatabase);
    }

    // Refresh type filter
    if (this.selectedRefreshType !== 'all') {
      filtered = filtered.filter((mv) => mv && mv.refresh_type === this.selectedRefreshType);
    }

    // Active state filter
    if (this.selectedActiveState !== 'all') {
      const isActive = this.selectedActiveState === 'active';
      filtered = filtered.filter((mv) => mv && mv.is_active === isActive);
    }

    // Refresh state filter
    if (this.selectedRefreshState !== 'all') {
      filtered = filtered.filter(
        (mv) => mv && mv.last_refresh_state === this.selectedRefreshState,
      );
    }

    // Advanced filters
    if (this.showAdvancedFilters) {
      // Refresh time filter: back-end timestamps use a space separator
      // ('2026-09-11 09:25:50') while datetime-local emits 'T'; compare
      // normalized minute-granularity strings so the filter actually matches.
      const tStart = this.normalizeTime(this.refreshTimeStart);
      const tEnd = this.normalizeTime(this.refreshTimeEnd);
      if (tStart) {
        filtered = filtered.filter((mv) => {
          const t = mv.last_refresh_finished_time
            ? this.normalizeTime(mv.last_refresh_finished_time)
            : '';
          return !!t && t >= tStart;
        });
      }
      if (tEnd) {
        filtered = filtered.filter((mv) => {
          const t = mv.last_refresh_finished_time
            ? this.normalizeTime(mv.last_refresh_finished_time)
            : '';
          return !!t && t <= tEnd;
        });
      }

      // Row count filter (0 is a valid row count)
      if (this.rowCountMin !== null) {
        filtered = filtered.filter(
          (mv) => mv && mv.rows !== null && mv.rows !== undefined && mv.rows >= this.rowCountMin,
        );
      }
      if (this.rowCountMax !== null) {
        filtered = filtered.filter(
          (mv) => mv && mv.rows !== null && mv.rows !== undefined && mv.rows <= this.rowCountMax,
        );
      }

      // Partition type filter
      if (this.selectedPartitionType !== 'all') {
        filtered = filtered.filter(
          (mv) => mv && mv.partition_type === this.selectedPartitionType,
        );
      }
    }

    this.filteredMaterializedViews = filtered;
    return assignTableRows(this.source, filtered).then(() => {
      // 表格内部行更新可能晚于本次变更检测，显式触发一次（变量页同款）
      this.cdRef.detectChanges();
    });
  }

  /**
   * Normalize back-end timestamps and picker values to a local, minute-precision
   * representation before lexical comparison.
   */
  private normalizeTime(value: string | Date | null): string {
    if (value instanceof Date) {
      if (Number.isNaN(value.getTime())) {
        return '';
      }
      const pad = (part: number) => String(part).padStart(2, '0');
      return `${value.getFullYear()}-${pad(value.getMonth() + 1)}-${pad(value.getDate())}T${pad(value.getHours())}:${pad(value.getMinutes())}`;
    }
    if (!value) {
      return '';
    }
    const match = value.replace(' ', 'T').match(/^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})/);
    return match ? match[1] : '';
  }

  onSearch() {
    this.applyFilters();
  }

  clearAllFilters() {
    this.searchText = '';
    this.selectedDatabase = 'all';
    this.selectedRefreshType = 'all';
    this.selectedActiveState = 'all';
    this.selectedRefreshState = 'all';
    this.refreshTimeStart = null;
    this.refreshTimeEnd = null;
    this.rowCountMin = null;
    this.rowCountMax = null;
    this.selectedPartitionType = 'all';
    this.applyFilters();
  }

  toggleAdvancedFilters() {
    this.showAdvancedFilters = !this.showAdvancedFilters;
  }

  getActiveFiltersCount(): number {
    let count = 0;
    if (this.searchText.trim()) count++;
    if (this.selectedDatabase !== 'all') count++;
    if (this.selectedRefreshType !== 'all') count++;
    if (this.selectedActiveState !== 'all') count++;
    if (this.selectedRefreshState !== 'all') count++;
    if (this.refreshTimeStart) count++;
    if (this.refreshTimeEnd) count++;
    if (this.rowCountMin !== null) count++;
    if (this.rowCountMax !== null) count++;
    if (this.selectedPartitionType !== 'all') count++;
    return count;
  }

  onRowSelect(event: RowSelectionEvent): void {
    const mv = event.data as MaterializedView | null;
    if (mv) {
      this.viewDetail(mv, event.row?.index);
    }
  }

  onDelete(event: any) {
    const mv = event.data as MaterializedView;

    this.confirmDialogService.confirmDelete(mv.name)
      .subscribe(confirmed => {
        if (!confirmed) {
          return;
        }

        this.deleteMV(mv, event);
      });
  }

  // Check if refresh action should be shown
  openOptimizationOpportunities(): void {
    this.dialogService.open(MvOpportunitiesSheetComponent, {
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: true,
      dialogClass: 'side-sheet',
    });
  }

  openCreateDialog(): void {
    this.createDialogClosed$.next();
    this.createDatabase = this.selectedDatabase === 'all' ? '' : this.selectedDatabase;
    this.createName = '';
    this.createSourceDatabase = '';
    this.createSourceTable = '';
    this.createColumns = [];
    this.createSchedule = 'manual';
    this.createScheduleInterval = '1';
    this.createScheduleUnit = 'hour';
    this.createMode = 'guided';
    this.createQuerySql = '';
    this.createPartitionBy = '';
    this.createDistribution = 'default';
    this.createHashColumns = '';
    this.createBuckets = '';
    this.createSortColumns = '';
    this.createReplicationNum = '';
    this.createBuildImmediate = false;
    this.createStep = 'configure';
    this.createPreview = '';
    this.createPreviewRequest = null;
    this.previewing = false;
    this.creating = false;
    this.createError = '';
    this.createSourceTables = [];
    this.createAvailableColumns = [];
    this.createSourceTablesLoading = false;
    this.createColumnsLoading = false;
    this.loadCreateDatabases();
    this.createDialogRef = this.dialogService.open(this.createDialogTemplate, {
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: false,
      dialogClass: 'side-sheet',
      hasBackdrop: true,
      hasScroll: true,
    });
  }

  closeCreateDialog(): void {
    this.createDialogClosed$.next();
    if (this.createDialogRef) {
      this.createDialogRef.close();
      this.createDialogRef = undefined;
    }
  }

  onCreateSourceDatabaseChange(database: string): void {
    const requestId = ++this.createTablesRequest;
    ++this.createColumnsRequest;
    this.createSourceDatabase = database;
    this.createSourceTable = '';
    this.createColumns = [];
    this.createSourceTables = [];
    this.createAvailableColumns = [];
    this.createSourceTablesLoading = false;
    this.createColumnsLoading = false;
    this.createError = '';

    if (!this.createDatabase) {
      this.createDatabase = database;
    }
    if (!database || !this.clusterId) {
      return;
    }

    const catalog = this.activeCluster?.catalog || 'default_catalog';
    this.createSourceTablesLoading = true;
    this.nodeService
      .getSchemaObjects(this.clusterId, catalog, database)
      .pipe(
        takeUntil(this.createDialogClosed$),
        takeUntil(this.destroy$),
        timeout(20_000),
        catchError((error) => {
          if (requestId === this.createTablesRequest) {
            this.createError = ErrorHandler.extractErrorMessage(error);
          }
          return of([] as SchemaObjectSummary[]);
        }),
      )
      .subscribe((objects) => {
        if (requestId === this.createTablesRequest) {
          this.createSourceTables = objects.filter((object) => object.object_kind === 'table');
          this.createSourceTablesLoading = false;
          this.cdRef.detectChanges();
        }
      });
  }

  onCreateSourceTableChange(table: string): void {
    const requestId = ++this.createColumnsRequest;
    this.createSourceTable = table;
    this.createColumns = [];
    this.createAvailableColumns = [];
    this.createColumnsLoading = false;
    this.createError = '';

    const source = this.createSourceTables.find((object) => object.name === table);
    if (!source || !this.clusterId) {
      return;
    }

    this.createColumnsLoading = true;
    this.nodeService
      .getSchemaObject(this.clusterId, source.object_ref)
      .pipe(
        takeUntil(this.createDialogClosed$),
        takeUntil(this.destroy$),
        timeout(20_000),
        catchError((error) => {
          if (requestId === this.createColumnsRequest) {
            this.createError = ErrorHandler.extractErrorMessage(error);
          }
          return of(null);
        }),
      )
      .subscribe((detail) => {
        if (requestId === this.createColumnsRequest) {
          this.createAvailableColumns = detail?.columns ?? [];
          this.createColumnsLoading = false;
          this.cdRef.detectChanges();
        }
      });
  }

  loadCreateDatabases(): void {
    this.createDatabasesLoading = true;
    this.createError = '';
    this.nodeService
      .getDatabases(this.activeCluster?.catalog)
      .pipe(
        takeUntil(this.createDialogClosed$),
        takeUntil(this.destroy$),
        timeout(20_000),
        catchError((error) => {
          this.createError = ErrorHandler.extractErrorMessage(error);
          return of([] as string[]);
        }),
      )
      .subscribe((databases) => {
        this.createDatabases = databases;
        this.createDatabasesLoading = false;
        this.cdRef.detectChanges();
      });
  }

  async formatCreateQuery(): Promise<void> {
    const query = this.createQuerySql;
    if (!query.trim()) return;
    try {
      const { format } = await import('sql-formatter');
      if (!this.createDialogRef || this.createStep !== 'configure' || this.createQuerySql !== query) return;
      this.createQuerySql = format(query, { language: 'mysql', tabWidth: 2, keywordCase: 'upper' });
      this.cdRef.detectChanges();
    } catch {
      if (this.createDialogRef) this.createError = this.i18n.instant('SQL 格式化失败，请检查查询语法。');
    }
  }

  private createRequest(): CreateMaterializedViewRequest | null {
    if (!this.clusterId) {
      this.createError = this.i18n.instant('当前没有可用集群，请切换集群后重试。');
      return null;
    }
    if (!this.createDatabase || !this.createName.trim()
      || (this.createMode === 'guided' && (!this.createSourceDatabase || !this.createSourceTable || !this.createColumns.length))
      || (this.createMode === 'sql' && !this.createQuerySql.trim())) {
      this.createError = this.i18n.instant('请填写目标库与名称，并选择源表和列或输入 SELECT 查询。');
      return null;
    }
    if (!/^[a-zA-Z][a-zA-Z0-9_]{0,63}$/.test(this.createName.trim())) {
      this.createError = this.i18n.instant('物化视图名称必须以字母开头，仅含字母、数字和下划线，最多 64 个字符。');
      return null;
    }
    const interval = Number(this.createScheduleInterval);
    if (this.createSchedule === 'scheduled' && (!Number.isInteger(interval) || interval < 1 || interval > 8760)) {
      this.createError = this.i18n.instant('刷新间隔必须是 1 到 8760 之间的整数。');
      return null;
    }
    const buckets = String(this.createBuckets ?? '').trim() ? Number(this.createBuckets) : undefined;
    if (buckets !== undefined && (!Number.isInteger(buckets) || buckets < 1 || buckets > 1024)) {
      this.createError = this.i18n.instant('分桶数必须是 1 到 1024 之间的整数。');
      return null;
    }
    const hashColumns = this.createHashColumns.split(',').map(column => column.trim()).filter(Boolean);
    const sortColumns = this.createSortColumns.split(',').map(column => column.trim()).filter(Boolean);
    const replicationNum = String(this.createReplicationNum ?? '').trim() ? Number(this.createReplicationNum) : undefined;
    if (replicationNum !== undefined && (!Number.isInteger(replicationNum) || replicationNum < 1 || replicationNum > 10)) {
      this.createError = this.i18n.instant('副本数必须是 1 到 10 之间的整数。');
      return null;
    }
    if (this.createDistribution === 'hash' && !hashColumns.length) {
      this.createError = this.i18n.instant('HASH 分布需填写至少一个结果列。');
      return null;
    }
    return {
      database: this.createDatabase.trim(),
      name: this.createName.trim(),
      cluster_id: this.clusterId,
      ...(this.createMode === 'sql'
        ? { query_sql: this.createQuerySql.trim() }
        : { source_database: this.createSourceDatabase, source_table: this.createSourceTable, columns: [...this.createColumns] }),
      ...(this.createPartitionBy.trim() ? { partition_by: this.createPartitionBy.trim() } : {}),
      ...(this.createDistribution === 'default' ? {} : {
        distribution: this.createDistribution === 'hash'
          ? { kind: 'hash' as const, columns: hashColumns, buckets }
          : { kind: 'random' as const, buckets },
      }),
      build_immediate: this.createBuildImmediate,
      ...(sortColumns.length ? { sort_columns: sortColumns } : {}),
      ...(replicationNum !== undefined ? { replication_num: replicationNum } : {}),
      schedule: this.createSchedule === 'manual'
        ? { kind: 'manual' }
        : { kind: 'scheduled', interval, unit: this.createScheduleUnit },
    };
  }

  previewMV(): void {
    if (this.previewing || this.creating) return;
    this.createError = '';
    const request = this.createRequest();
    if (!request) return;
    this.previewing = true;
    this.mvService.previewMaterializedView(request).pipe(takeUntil(this.createDialogClosed$), takeUntil(this.destroy$), timeout(30_000)).subscribe({
      next: ({ ddl }) => {
        this.createPreviewRequest = request;
        this.createPreview = ddl;
        this.createStep = 'review';
        this.previewing = false;
        this.cdRef.detectChanges();
      },
      error: (error) => {
        this.createError = ErrorHandler.extractErrorMessage(error);
        this.previewing = false;
        this.cdRef.detectChanges();
      },
    });
  }

  editCreateDraft(): void {
    this.createStep = 'configure';
    this.createError = '';
    this.createPreview = '';
    this.createPreviewRequest = null;
  }

  createMV(): void {
    if (this.creating) return;
    if (!this.createPreviewRequest || !this.createPreview) {
      this.createError = this.i18n.instant('请先预览并确认实际执行的 DDL。');
      return;
    }
    const request = { ...this.createPreviewRequest, confirmed_ddl: this.createPreview };
    this.createError = '';
    this.creating = true;
    this.mvService
      .createMaterializedView(request)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          const message = request.build_immediate
            ? '物化视图已创建，已触发首次构建；请查看刷新状态。'
            : request.schedule.kind === 'scheduled'
              ? '物化视图已创建；将按计划刷新，也可手动发起首次刷新。'
              : '物化视图已创建；首次刷新需单独发起。';
          this.toastrService.success(this.i18n.instant(message), this.i18n.instant('创建成功'));
          this.closeCreateDialog();
          this.loadMaterializedViews();
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.createError = ErrorHandler.extractErrorMessage(error);
          this.creating = false;
        },
      });
  }

  viewDetail(mv: MaterializedView, rowIndex?: number): void {
    const template = this.detailDialogTemplate;
    if (!template || this.sheetClosing) {
      return;
    }

    this.captureDetailTrigger(rowIndex);
    this.selectedMV = mv;
    this.resetDetailResources();
    const dialogRef = this.dialogService.open(template, {
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: false,
      dialogClass: 'side-sheet',
      hasBackdrop: true,
      hasScroll: true,
    });
    this.detailDialogRef = dialogRef;
    this.document.defaultView?.requestAnimationFrame(() => {
      this.document
        .querySelector<HTMLElement>('.cdk-overlay-pane.side-sheet .materialized-view-detail-sheet')
        ?.focus({ preventScroll: true });
    });
    dialogRef.onBackdropClick.pipe(take(1)).subscribe(() => this.closeDetailDialog(dialogRef));
    dialogRef.onClose.pipe(take(1)).subscribe(() => {
      this.detailRequest$.next();
      this.selectedMV = null;
      this.mvDDL = '';
      this.dependencies = null;
      this.dependenciesLoading = false;
      this.dependencyObject = null;
      this.dependencyObjectLoading = false;
      this.dependencyObjectRequest$.next();
      this.dependencyObjectReferenceWarning = '';
      this.dependencyObjectRefs.clear();
      this.dependencyGraphNodes = [];
      this.dependencyGraphEdges = [];
      this.detailDialogRef = undefined;
      this.sheetClosing = false;
      this.restoreDetailFocus();
    });

    this.loadDetailResources(mv, dialogRef);
  }

  private resetDetailResources(): void {
    this.mvDDL = '';
    this.dependencies = null;
    this.dependenciesLoading = true;
    this.dependencyObject = null;
    this.dependencyObjectLoading = false;
    this.dependencyObjectRequest$.next();
    this.dependencyObjectReferenceWarning = '';
    this.dependencyObjectRefs.clear();
    this.dependencyGraphNodes = [];
    this.dependencyGraphEdges = [];
  }

  private loadDetailResources(mv: MaterializedView, dialogRef: NbDialogRef<unknown>): void {
    this.detailRequest$.next();
    this.mvService
      .getMaterializedViewDDL(this.objectRef(mv))
      .pipe(takeUntil(this.destroy$), takeUntil(this.detailRequest$), timeout(20000))
      .subscribe({
        next: (result) => {
          if (this.detailDialogRef === dialogRef && this.isSelected(mv)) {
            this.mvDDL = result.ddl;
            this.cdRef.detectChanges();
          }
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '加载DDL失败',
          );
        },
      });

    this.mvService
      .getDependencies(this.objectRef(mv))
      .pipe(takeUntil(this.destroy$), takeUntil(this.detailRequest$), timeout(20000))
      .subscribe({
        next: (dependencies) => {
          if (this.detailDialogRef === dialogRef && this.isSelected(mv)) {
            this.dependencies = dependencies;
            this.dependenciesLoading = false;
            this.buildDependencyGraph(mv, dependencies);
            this.resolveDependencyObjectReferences(mv, dependencies, dialogRef);
            this.cdRef.detectChanges();
          }
        },
        error: (error) => {
          if (this.detailDialogRef === dialogRef && this.isSelected(mv)) {
            this.dependencies = {
              object: this.objectRef(mv),
              dependencies: [],
              complete: false,
              warnings: [ErrorHandler.extractErrorMessage(error)],
              read_at: new Date().toISOString(),
            };
            this.dependenciesLoading = false;
            this.cdRef.detectChanges();
          }
        },
      });
  }

  private reloadSelectedMVDetail(): void {
    this.loadMaterializedViews();

    const mv = this.selectedMV;
    const dialogRef = this.detailDialogRef;
    if (!mv || !dialogRef) {
      return;
    }

    this.detailRequest$.next();
    this.mvService
      .getMaterializedView(this.objectRef(mv))
      .pipe(takeUntil(this.destroy$), takeUntil(this.detailRequest$), timeout(20000))
      .subscribe({
        next: (updated) => {
          if (this.detailDialogRef !== dialogRef || !this.isSelected(mv)) {
            return;
          }
          this.selectedMV = updated;
          this.resetDetailResources();
          this.loadDetailResources(updated, dialogRef);
        },
        error: () => {
          if (this.detailDialogRef === dialogRef && this.isSelected(mv)) {
            this.toastrService.warning('操作已完成，但详情更新失败；请稍后刷新查看最新状态', '提示');
          }
        },
      });
  }

  closeDetailDialog(ref?: NbDialogRef<unknown>): void {
    const dialogRef = ref || this.detailDialogRef;
    if (!dialogRef || this.sheetClosing) {
      return;
    }

    const sheet = this.document.querySelector<HTMLElement>('.cdk-overlay-pane.side-sheet');
    if (!sheet || this.prefersReducedMotion()) {
      dialogRef.close();
      return;
    }

    this.sheetClosing = true;
    sheet.classList.add('side-sheet--closing');
    this.document
      .querySelector<HTMLElement>('.cdk-overlay-backdrop.side-sheet-backdrop')
      ?.classList.add('side-sheet-backdrop--closing');
    this.document.defaultView?.setTimeout(
      () => dialogRef.close(),
      MaterializedViewsComponent.sheetExitDurationMs,
    );
  }

  editSelectedMV(): void {
    if (this.selectedMV) {
      this.openEditDialog(this.selectedMV);
    }
  }

  refreshSelectedMV(): void {
    if (this.selectedMV) {
      this.openRefreshDialog(this.selectedMV);
    }
  }

  toggleSelectedMV(): void {
    if (this.selectedMV) {
      this.toggleActiveState(this.selectedMV, true);
    }
  }

  copyDDL(): void {
    if (!this.mvDDL) {
      return;
    }
    const done = () => this.toastrService.success(this.i18n.instant('DDL 已复制'), this.i18n.instant('成功'));
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(this.mvDDL).then(done).catch(() => done());
    } else {
      done();
    }
  }

  openDependencyNode(node: typeof this.dependencyGraphNodes[number], event?: Event): void {
    event?.preventDefault();
    event?.stopPropagation();
    if (!node.objectRef) {
      return;
    }

    this.dependencyObjectRequest$.next();
    this.dependencyObject = null;
    this.dependencyObjectLoading = true;
    this.cdRef.detectChanges();
    this.loadDependencyObject(node, node.objectRef, true);
  }

  dependencyObjectPropertyEntries(): Array<[string, string]> {
    return Object.entries(this.dependencyObject?.physical_properties?.properties || {});
  }

  openRefreshDialog(mv: MaterializedView) {
    if (this.refreshDialogRef) {
      return;
    }
    this.selectedMV = mv;
    this.refreshMode = this.activeCluster?.cluster_type === 'doris' ? 'auto' : 'async';
    this.refreshForce = false;
    this.refreshPartitionStart = '';
    this.refreshPartitionEnd = '';
    this.refreshing = false;

    this.refreshDialogRef = this.dialogService.open(this.refreshDialogTemplate, {
      context: {},
      hasBackdrop: true,
      backdropClass: 'mv-action-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: true,
      autoFocus: true,
      dialogClass: 'mv-action-dialog',
    });
    this.refreshDialogRef.onClose.pipe(take(1)).subscribe(() => {
      this.refreshDialogRef = undefined;
    });
  }

  closeRefreshDialog() {
    if (this.refreshDialogRef) {
      this.refreshDialogRef.close();
    }
  }

  refreshMV() {
    if (!this.selectedMV || this.refreshing) return;

    this.refreshing = true;
    this.mvService
      .refreshMaterializedView(this.objectRef(this.selectedMV), {
        mode: this.refreshMode,
        force: this.refreshForce,
        partition: this.refreshPartitionStart && this.refreshPartitionEnd
          ? {
              start: { type: 'string', value: this.refreshPartitionStart },
              end: { type: 'string', value: this.refreshPartitionEnd },
            }
          : undefined,
      })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('刷新任务已启动'), this.i18n.instant('成功'));
          this.closeRefreshDialog();
          setTimeout(() => this.reloadSelectedMVDetail(), 1000);
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '刷新失败',
          );
          this.refreshing = false;
        },
      });
  }

  cancelRefresh(mv: MaterializedView) {
    this.confirmDialogService
      .confirm(
        '取消刷新',
        `确定要取消物化视图 "${mv.name}" 的刷新任务吗？`,
        '取消刷新',
        '不取消',
      )
      .subscribe((confirmed) => {
        if (confirmed) {
          this.mvService
            .cancelRefreshMaterializedView(this.objectRef(mv), false)
            .pipe(takeUntil(this.destroy$))
            .subscribe({
              next: () => {
                this.toastrService.success(this.i18n.instant('刷新任务已取消'), this.i18n.instant('成功'));
                this.loadMaterializedViews();
              },
              error: (error) => {
                if (!this.authService.isAuthenticated()) {
                  return;
                }
                this.toastrService.danger(
                  ErrorHandler.extractErrorMessage(error),
                  '取消刷新失败',
                );
              },
            });
        }
      });
  }

  deleteMV(mv: MaterializedView, tableEvent?: any) {
    this.mvService
      .deleteMaterializedView(this.objectRef(mv))
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('物化视图删除成功'), this.i18n.instant('成功'));
          tableEvent?.confirm.resolve();
          this.loadMaterializedViews();
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '删除失败',
          );
          tableEvent?.confirm.reject();
        },
      });
  }

  // Toggle Active/Inactive state
  toggleActiveState(mv: MaterializedView, nested = false) {
    const action = mv.is_active ? '停用' : '激活';
    
    this.confirmDialogService
      .confirm(
        `${action}物化视图`,
        `确定要${action}物化视图 "${mv.name}" 吗？`,
        action,
        '取消',
        'primary',
        { nested },
      )
      .subscribe((confirmed) => {
        if (confirmed) {
          this.mvService
            .setMaterializedViewState(this.objectRef(mv), mv.is_active ? 'inactive' : 'active')
            .pipe(takeUntil(this.destroy$))
            .subscribe({
              next: () => {
                this.toastrService.success(`物化视图已${action}`, '成功');
                this.reloadSelectedMVDetail();
              },
              error: (error) => {
                if (!this.authService.isAuthenticated()) {
                  return;
                }
                this.toastrService.danger(
                  ErrorHandler.extractErrorMessage(error),
                  `${action}失败`,
                );
              },
            });
        }
      });
  }

  // Open edit dialog
  openEditDialog(mv: MaterializedView) {
    if (this.editDialogRef) {
      return;
    }
    this.selectedMV = mv;
    this.editAction = 'rename';
    this.editNewName = mv.name;
    this.editRefreshStrategy = 'manual';
    this.editRefreshInterval = '1';
    this.editRefreshUnit = 'hour';
    this.editing = false;

    this.editDialogRef = this.dialogService.open(this.editDialogTemplate, {
      context: {},
      hasBackdrop: true,
      backdropClass: 'mv-action-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: true,
      autoFocus: true,
      dialogClass: 'mv-action-dialog',
    });
    this.editDialogRef.onClose.pipe(take(1)).subscribe(() => {
      this.editDialogRef = undefined;
    });
  }

  closeEditDialog() {
    if (this.editDialogRef) {
      this.editDialogRef.close();
    }
  }

  // Execute edit action
  editMV() {
    if (!this.selectedMV || this.editing) return;

    switch (this.editAction) {
      case 'rename':
        if (!this.editNewName.trim()) {
          this.toastrService.warning(this.i18n.instant('请输入新名称'), this.i18n.instant('输入错误'));
          return;
        }
        if (this.editNewName === this.selectedMV.name) {
          this.toastrService.warning(this.i18n.instant('新名称与当前名称相同'), this.i18n.instant('输入错误'));
          return;
        }
        break;
        
      case 'refresh_strategy':
        if (this.editRefreshStrategy === 'scheduled') {
          const interval = Number(this.editRefreshInterval);
          if (!Number.isInteger(interval) || interval < 1 || interval > 8760) {
            this.toastrService.warning(this.i18n.instant('请输入 1 到 8760 之间的整数刷新间隔'), this.i18n.instant('输入错误'));
            return;
          }
        }
        break;
    }

    this.editing = true;
    const reference = this.objectRef(this.selectedMV);
    const renamedTo = this.editAction === 'rename' ? this.editNewName.trim() : undefined;
    const request = this.editAction === 'rename'
      ? this.mvService.renameMaterializedView(reference, renamedTo!)
      : this.mvService.updateRefreshSchedule(reference, this.refreshSchedule());
    request
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('物化视图修改成功'), this.i18n.instant('成功'));
          if (renamedTo && this.selectedMV) {
            this.selectedMV = { ...this.selectedMV, name: renamedTo };
          }
          this.closeEditDialog();
          this.reloadSelectedMVDetail();
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '修改失败',
          );
          this.editing = false;
        },
      });
  }

  formatNumber(num: number): string {
    if (num >= 1000000) {
      return (num / 1000000).toFixed(1) + 'M';
    } else if (num >= 1000) {
      return (num / 1000).toFixed(1) + 'K';
    }
    return num.toString();
  }

  get isDoris(): boolean {
    return this.activeCluster?.cluster_type === 'doris';
  }

  get availableRefreshModeOptions(): Array<{ value: RefreshMode; label: string }> {
    return this.isDoris
      ? [
          { value: 'auto', label: this.i18n.instant('自动模式') },
          { value: 'complete', label: this.i18n.instant('全量模式') },
        ]
      : this.refreshModeOptions;
  }

  private objectRef(mv: MaterializedView): MaterializedViewRef {
    return { database: mv.database_name, name: mv.name, kind: mv.kind };
  }

  private isSelected(mv: MaterializedView): boolean {
    return this.selectedMV?.name === mv.name
      && this.selectedMV.database_name === mv.database_name
      && this.selectedMV.kind === mv.kind;
  }

  private refreshSchedule(): RefreshSchedule {
    if (this.editRefreshStrategy === 'manual') {
      return { kind: 'manual' };
    }
    return {
      kind: 'scheduled',
      interval: Number(this.editRefreshInterval),
      unit: this.editRefreshUnit,
    };
  }

  dependencyObjectLabel(catalog: string | undefined, database: string | undefined, name: string): string {
    return [catalog, database, name].filter((part): part is string => !!part).join('.');
  }

  private resolveDependencyObjectReferences(
    materializedView: MaterializedView,
    dependencies: MaterializedViewDependencies,
    dialogRef: NbDialogRef<unknown>,
  ): void {
    if (!this.clusterId || dependencies.dependencies.length === 0) {
      return;
    }

    const locations = new Map<string, { catalog: string; database: string }>();
    dependencies.dependencies.forEach((dependency) => {
      const catalog = dependency.object.catalog || this.activeCluster?.catalog;
      const database = dependency.object.database || materializedView.database_name;
      if (catalog && database) {
        locations.set(`${catalog}\u0000${database}`, { catalog, database });
      }
    });
    const requests = Array.from(locations.values()).map((location) =>
      this.nodeService.getSchemaObjects(this.clusterId, location.catalog, location.database).pipe(
        map((objects) => ({ ...location, objects })),
        catchError(() => of({ ...location, objects: [] as SchemaObjectSummary[] })),
      ),
    );
    if (requests.length === 0) {
      return;
    }

    forkJoin(requests)
      .pipe(takeUntil(this.destroy$), takeUntil(this.detailRequest$))
      .subscribe((results) => {
        if (this.detailDialogRef !== dialogRef || !this.isSelected(materializedView)) {
          return;
        }

        const objectsByLocation = new Map(
          results.map((result) => [`${result.catalog}\u0000${result.database}`, result.objects]),
        );
        let unresolved = 0;
        dependencies.dependencies.forEach((dependency) => {
          const catalog = dependency.object.catalog || this.activeCluster?.catalog;
          const database = dependency.object.database || materializedView.database_name;
          const expectedKind = this.schemaKindForDependency(dependency.object.kind);
          const objects = catalog && database
            ? objectsByLocation.get(`${catalog}\u0000${database}`) || []
            : [];
          const object = objects.find((candidate) =>
            candidate.name === dependency.object.name
            && (!expectedKind || candidate.object_kind === expectedKind),
          );
          if (!object || !catalog || !database) {
            unresolved += 1;
            return;
          }
          this.dependencyObjectRefs.set(
            this.dependencyObjectKey(catalog, database, dependency.object.name, expectedKind),
            object.object_ref,
          );
        });
        this.dependencyObjectReferenceWarning = unresolved > 0
          ? '部分对象详情不可用；关系图仍仅基于引擎返回的依赖元数据。'
          : '';
        this.buildDependencyGraph(materializedView, dependencies);
        this.cdRef.detectChanges();
      });
  }

  private loadDependencyObject(
    node: typeof this.dependencyGraphNodes[number],
    objectRef: string,
    retryExpiredReference: boolean,
  ): void {
    this.nodeService
      .getSchemaObject(this.clusterId, objectRef)
      .pipe(
        takeUntil(this.destroy$),
        takeUntil(this.detailRequest$),
        takeUntil(this.dependencyObjectRequest$),
        timeout(20000),
      )
      .subscribe({
        next: (object) => {
          this.dependencyObjectLoading = false;
          this.dependencyObject = object;
          this.cdRef.detectChanges();
        },
        error: (error) => {
          if (retryExpiredReference && error?.status === 404) {
            this.renewDependencyObjectReference(node);
            return;
          }
          this.dependencyObjectLoading = false;
          this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '获取对象详情失败');
          this.cdRef.detectChanges();
        },
      });
  }

  private renewDependencyObjectReference(node: typeof this.dependencyGraphNodes[number]): void {
    if (!node.catalog || !node.database || !node.objectName) {
      this.dependencyObjectLoading = false;
      return;
    }
    this.nodeService
      .getSchemaObjects(this.clusterId, node.catalog, node.database)
      .pipe(
        takeUntil(this.destroy$),
        takeUntil(this.detailRequest$),
        takeUntil(this.dependencyObjectRequest$),
        timeout(20000),
      )
      .subscribe({
        next: (objects) => {
          const object = objects.find((candidate) =>
            candidate.name === node.objectName
            && (!node.objectKind || candidate.object_kind === node.objectKind),
          );
          if (!object) {
            this.dependencyObjectLoading = false;
            this.toastrService.warning('对象已不存在或当前用户无权读取', '提示');
            this.cdRef.detectChanges();
            return;
          }
          node.objectRef = object.object_ref;
          this.dependencyObjectRefs.set(
            this.dependencyObjectKey(node.catalog!, node.database!, node.objectName!, node.objectKind),
            object.object_ref,
          );
          this.loadDependencyObject(node, object.object_ref, false);
        },
        error: (error) => {
          this.dependencyObjectLoading = false;
          this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '刷新对象引用失败');
          this.cdRef.detectChanges();
        },
      });
  }

  private buildDependencyGraph(
    materializedView: MaterializedView,
    dependencies: MaterializedViewDependencies,
  ): void {
    const compact = this.isCompactDependencyGraph();
    const nodeWidth = compact ? 132 : 220;
    const nodeHeight = compact ? 68 : 72;
    const labelLength = compact ? 16 : 28;
    this.dependencyGraphCompact = compact;
    const graph = new dagre.graphlib.Graph();
    graph.setGraph({
      rankdir: 'LR',
      marginx: compact ? 16 : 28,
      marginy: compact ? 16 : 28,
      ranksep: compact ? 36 : 92,
      nodesep: compact ? 20 : 28,
    });
    graph.setDefaultEdgeLabel(() => ({}));

    const currentId = 'current';
    const items = [
      {
        id: currentId,
        label: this.graphLabel(`${materializedView.database_name}.${materializedView.name}`, labelLength),
        subtitle: materializedView.kind === 'rollup' ? 'ROLLUP' : 'MATERIALIZED VIEW',
        type: 'current',
      },
      ...dependencies.dependencies.map((dependency, index) => {
        const catalog = dependency.object.catalog || this.activeCluster?.catalog;
        const database = dependency.object.database || materializedView.database_name;
        const objectKind = this.schemaKindForDependency(dependency.object.kind);
        return {
          id: `dependency-${index}`,
          label: this.graphLabel(
            this.dependencyObjectLabel(catalog, database, dependency.object.name),
            labelLength,
          ),
          subtitle: dependency.object.kind.replace('_', ' ').toUpperCase(),
          type: dependency.object.kind,
          evidence: dependency.evidence,
          catalog,
          database,
          objectName: dependency.object.name,
          objectKind,
          objectRef: catalog && database
            ? this.dependencyObjectRefs.get(
                this.dependencyObjectKey(catalog, database, dependency.object.name, objectKind),
              )
            : undefined,
        };
      }),
    ];

    items.forEach((item) => graph.setNode(item.id, { width: nodeWidth, height: nodeHeight }));
    items.slice(1).forEach((item) => graph.setEdge(item.id, currentId));
    dagre.layout(graph);

    this.dependencyGraphNodes = items.map((item) => {
      const layout = graph.node(item.id);
      return { ...item, x: Math.round(layout.x), y: Math.round(layout.y), width: nodeWidth, height: nodeHeight };
    });
    const byId = new Map(this.dependencyGraphNodes.map((node) => [node.id, node]));
    const upstreamItems = items.slice(1);
    this.dependencyGraphEdges = upstreamItems.flatMap((item, index) => {
      const source = byId.get(item.id);
      const target = byId.get(currentId);
      if (!source || !target) {
        return [];
      }

      const from = { x: source.x + source.width / 2, y: source.y };
      const to = {
        x: target.x - target.width / 2,
        y: Math.round(target.y - target.height / 2 + (target.height / (upstreamItems.length + 1)) * (index + 1)),
      };
      const controlOffset = Math.min(72, Math.max(12, Math.round((to.x - from.x) / 2)));
      const routeLift = upstreamItems.length === 1 ? (compact ? -8 : -10) : 0;

      return [{
        from,
        to,
        path: `M ${from.x} ${from.y} C ${from.x + controlOffset} ${from.y + routeLift}, ${to.x - controlOffset} ${to.y + routeLift}, ${to.x} ${to.y}`,
        label: 'mv_reads',
        labelX: Math.round((from.x + to.x) / 2),
        labelY: Math.max(12, Math.round(Math.min(from.y, to.y) - source.height / 2 + routeLift - 6)),
      }];
    });
    const margin = compact ? 16 : 28;
    this.dependencyGraphWidth = Math.max(
      compact ? 320 : 600,
      ...this.dependencyGraphNodes.map((node) => node.x + node.width / 2 + margin),
    );
    this.dependencyGraphHeight = Math.max(
      128,
      ...this.dependencyGraphNodes.map((node) => node.y + node.height / 2 + margin),
    );
  }

  private isCompactDependencyGraph(): boolean {
    return (this.document.defaultView?.innerWidth || 1024) <= 640;
  }

  private schemaKindForDependency(kind: string): SchemaObjectKind | undefined {
    switch (kind) {
      case 'table':
        return 'table';
      case 'view':
        return 'view';
      case 'materialized_view':
        return 'materialized_view';
      default:
        return undefined;
    }
  }

  private dependencyObjectKey(
    catalog: string,
    database: string,
    name: string,
    kind: SchemaObjectKind | undefined,
  ): string {
    return `${catalog}\u0000${database}\u0000${name}\u0000${kind || ''}`;
  }

  private graphLabel(label: string, maxLength = 28): string {
    return label.length > maxLength ? `${label.slice(0, maxLength - 1)}...` : label;
  }

  private captureDetailTrigger(rowIndex?: number): void {
    const rows = this.document.querySelectorAll<HTMLElement>('angular2-smart-table tbody tr');
    const visibleRowIndex = rowIndex === undefined ? undefined : rowIndex % this.settings.pager.perPage;
    const row = visibleRowIndex === undefined ? undefined : rows.item(visibleRowIndex);
    if (row) {
      row.tabIndex = -1;
      row.focus();
    }
    const activeElement = this.document.activeElement;
    this.detailTrigger = activeElement instanceof HTMLElement ? activeElement : undefined;
  }

  private restoreDetailFocus(): void {
    const trigger = this.detailTrigger;
    this.detailTrigger = undefined;
    if (trigger?.isConnected) {
      this.document.defaultView?.setTimeout(() => trigger.focus());
    }
  }

  private prefersReducedMotion(): boolean {
    return this.document.defaultView?.matchMedia('(prefers-reduced-motion: reduce)').matches || false;
  }
}
