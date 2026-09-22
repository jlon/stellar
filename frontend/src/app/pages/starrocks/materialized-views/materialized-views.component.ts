import { I18nService } from '../../../@core/i18n/i18n.service';
import { DOCUMENT } from '@angular/common';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, TemplateRef, ViewChild, ChangeDetectorRef, inject } from '@angular/core';
import { Subject } from 'rxjs';
import { skip, take, takeUntil, timeout } from 'rxjs/operators';
import { NbToastrService, NbDialogRef, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbDatepickerModule, NbSelectModule, NbOptionModule, NbBadgeModule, NbSpinnerModule, NbAccordionModule, NbTabsetModule, NbAlertModule, NbCheckboxModule, NbFormFieldModule, NbTooltipModule } from '@nebular/theme';
import { MarkdownModule } from 'ngx-markdown';
import { LocalDataSource, Angular2SmartTableModule, RowSelectionEvent } from 'angular2-smart-table';
import {
  MaterializedViewService,
  MaterializedView,
} from '../../../@core/data/materialized-view.service';
import { ClusterService, Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { AuthService } from '../../../@core/data/auth.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { assignTableRows } from '../../../@core/utils/table-rows';
import { ActiveToggleRenderComponent } from './active-toggle-render.component';
import { BadgeRenderComponent, BadgeInfo } from './badge-render.component';
import { FormsModule } from '@angular/forms';


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
    NbSelectModule,
    NbOptionModule,
    NbBadgeModule,
    NbSpinnerModule,
    Angular2SmartTableModule,
    NbAccordionModule,
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
  private detailRequest$ = new Subject<void>();
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

  // Create form
  createSQL = '';
  creating = false;

  // Refresh form
  refreshMode = 'ASYNC';
  refreshForce = false;
  refreshPartitionStart = '';
  refreshPartitionEnd = '';
  refreshing = false;

  // Edit form
  editAction = 'rename'; // rename | refresh_strategy | properties | advanced
  editNewName = '';
  editRefreshStrategy = 'MANUAL';
  editRefreshInterval = '1';
  editRefreshUnit = 'HOUR'; // HOUR | DAY | WEEK | MONTH
  editPropertyKey = '';
  editPropertyValue = '';
  editAdvancedClause = '';
  editing = false;

  refreshModeOptions = [
    { value: 'ASYNC', label: this.i18n.instant('异步模式') },
    { value: 'SYNC', label: this.i18n.instant('同步模式') },
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
        componentInitFunction: (instance: BadgeRenderComponent) => {
          instance.getBadge = (_value: any, row: MaterializedView) => ({
            status: row?.refresh_type === 'ROLLUP' ? 'primary' : 'info',
            label: row?.refresh_type === 'ROLLUP' ? '同步' : '异步',
          });
        },
      },
      refresh_type: {
        title: this.i18n.instant('刷新策略'),
        type: 'custom',
        width: '9%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent) => {
          instance.getBadge = (value: string): BadgeInfo | null => {
            const map: Record<string, BadgeInfo> = {
              ASYNC: { status: 'success', label: this.i18n.instant('自动') },
              MANUAL: { status: 'info', label: this.i18n.instant('手动') },
              ROLLUP: { status: 'primary', label: this.i18n.instant('同步') },
              INCREMENTAL: { status: 'warning', label: this.i18n.instant('增量') },
            };
            return map[value] ?? null;
          };
        },
      },
      is_active: {
        title: this.i18n.instant('状态'),
        type: 'custom',
        width: '12%',
        renderComponent: ActiveToggleRenderComponent,
        componentInitFunction: (instance: any) => {
          instance.toggleActive.subscribe((rowData: any) => {
            this.toggleActiveState(rowData);
          });
        },
      },
      last_refresh_state: {
        title: this.i18n.instant('刷新状态'),
        type: 'custom',
        width: '9%',
        renderComponent: BadgeRenderComponent,
        componentInitFunction: (instance: BadgeRenderComponent) => {
          instance.getBadge = (value: string, row: MaterializedView): BadgeInfo | null => {
            if (row?.refresh_type === 'ROLLUP') return null;
            const map: Record<string, BadgeInfo> = {
              SUCCESS: { status: 'success', label: this.i18n.instant('成功') },
              RUNNING: { status: 'info', label: this.i18n.instant('运行中') },
              FAILED: { status: 'danger', label: this.i18n.instant('失败') },
              PENDING: { status: 'warning', label: this.i18n.instant('等待中') },
            };
            return map[value] ?? null;
          };
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
        componentInitFunction: (instance: BadgeRenderComponent) => {
          instance.getBadge = (_value: any, row: MaterializedView): BadgeInfo | null =>
            row?.last_refresh_error_message
              ? { status: 'danger', label: this.i18n.instant('错误'), tooltip: row.last_refresh_error_message }
              : null;
        },
      },
    },
  };

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
    this.detailRequest$.next();
    this.detailRequest$.complete();
    this.detailDialogRef?.close();
    this.destroy$.next();
    this.destroy$.complete();
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
  openCreateDialog() {
    this.createSQL = '';
    this.creating = false;
    this.createDialogRef = this.dialogService.open(this.createDialogTemplate, {
      context: {},
    });
  }

  closeCreateDialog() {
    if (this.createDialogRef) {
      this.createDialogRef.close();
    }
  }

  createMV() {
    if (!this.createSQL.trim()) {
      this.toastrService.warning(this.i18n.instant('请输入CREATE MATERIALIZED VIEW SQL语句'), this.i18n.instant('输入错误'));
      return;
    }

    this.creating = true;
    this.mvService
      .createMaterializedView( { sql: this.createSQL })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('物化视图创建成功'), this.i18n.instant('成功'));
          this.closeCreateDialog();
          this.loadMaterializedViews();
        },
        error: (error) => {
          if (!this.authService.isAuthenticated()) {
            return;
          }
          this.toastrService.danger(
            ErrorHandler.extractErrorMessage(error),
            '创建物化视图失败',
          );
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
    this.mvDDL = '';
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
      this.detailDialogRef = undefined;
      this.sheetClosing = false;
      this.restoreDetailFocus();
    });

    this.detailRequest$.next();
    this.mvService
      .getMaterializedViewDDL( mv.name)
      .pipe(takeUntil(this.destroy$), takeUntil(this.detailRequest$), timeout(20000))
      .subscribe({
        next: (result) => {
          if (this.detailDialogRef === dialogRef && this.selectedMV?.name === mv.name) {
            this.mvDDL = result.ddl;
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
    this.closeDetailThen((mv) => this.openEditDialog(mv));
  }

  refreshSelectedMV(): void {
    this.closeDetailThen((mv) => this.openRefreshDialog(mv));
  }

  toggleSelectedMV(): void {
    this.closeDetailThen((mv) => this.toggleActiveState(mv));
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

  openRefreshDialog(mv: MaterializedView) {
    this.selectedMV = mv;
    this.refreshMode = 'ASYNC';
    this.refreshForce = false;
    this.refreshPartitionStart = '';
    this.refreshPartitionEnd = '';
    this.refreshing = false;

    this.refreshDialogRef = this.dialogService.open(this.refreshDialogTemplate, {
      context: {},
    });
  }

  closeRefreshDialog() {
    if (this.refreshDialogRef) {
      this.refreshDialogRef.close();
    }
  }

  refreshMV() {
    if (!this.selectedMV) return;

    this.refreshing = true;
    this.mvService
      .refreshMaterializedView( this.selectedMV.name, {
        mode: this.refreshMode,
        force: this.refreshForce,
        partition_start: this.refreshPartitionStart || undefined,
        partition_end: this.refreshPartitionEnd || undefined,
      })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('刷新任务已启动'), this.i18n.instant('成功'));
          this.closeRefreshDialog();
          setTimeout(() => this.loadMaterializedViews(), 1000);
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
            .cancelRefreshMaterializedView( mv.name, false)
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
      .deleteMaterializedView( mv.name, true)
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
  toggleActiveState(mv: MaterializedView) {
    const newState = mv.is_active ? 'INACTIVE' : 'ACTIVE';
    const action = mv.is_active ? '停用' : '激活';
    
    this.confirmDialogService
      .confirm(
        `${action}物化视图`,
        `确定要${action}物化视图 "${mv.name}" 吗？`,
        action,
        '取消',
      )
      .subscribe((confirmed) => {
        if (confirmed) {
          this.mvService
            .alterMaterializedView( mv.name, { alter_clause: newState })
            .pipe(takeUntil(this.destroy$))
            .subscribe({
              next: () => {
                this.toastrService.success(`物化视图已${action}`, '成功');
                this.loadMaterializedViews();
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
    this.selectedMV = mv;
    this.editAction = 'rename';
    this.editNewName = mv.name;
    this.editRefreshStrategy = 'MANUAL';
    this.editRefreshInterval = '1';
    this.editRefreshUnit = 'HOUR';
    this.editPropertyKey = '';
    this.editPropertyValue = '';
    this.editAdvancedClause = '';
    this.editing = false;

    this.editDialogRef = this.dialogService.open(this.editDialogTemplate, {
      context: {},
    });
  }

  closeEditDialog() {
    if (this.editDialogRef) {
      this.editDialogRef.close();
    }
  }

  // Execute edit action
  editMV() {
    if (!this.selectedMV) return;

    let alterClause = '';
    
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
        alterClause = `RENAME ${this.editNewName}`;
        break;
        
      case 'refresh_strategy':
        if (this.editRefreshStrategy === 'MANUAL') {
          alterClause = 'REFRESH MANUAL';
        } else {
          const interval = parseInt(this.editRefreshInterval);
          if (!interval || interval <= 0) {
            this.toastrService.warning(this.i18n.instant('请输入有效的刷新间隔'), this.i18n.instant('输入错误'));
            return;
          }
          alterClause = `REFRESH ASYNC EVERY(INTERVAL ${interval} ${this.editRefreshUnit})`;
        }
        break;
        
      case 'properties':
        if (!this.editPropertyKey.trim() || !this.editPropertyValue.trim()) {
          this.toastrService.warning(this.i18n.instant('请输入属性名称和值'), this.i18n.instant('输入错误'));
          return;
        }
        alterClause = `SET ("${this.editPropertyKey}" = "${this.editPropertyValue}")`;
        break;
        
      case 'advanced':
        if (!this.editAdvancedClause.trim()) {
          this.toastrService.warning(this.i18n.instant('请输入ALTER子句'), this.i18n.instant('输入错误'));
          return;
        }
        alterClause = this.editAdvancedClause;
        break;
    }

    this.editing = true;
    this.mvService
      .alterMaterializedView( this.selectedMV.name, { alter_clause: alterClause })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastrService.success(this.i18n.instant('物化视图修改成功'), this.i18n.instant('成功'));
          this.closeEditDialog();
          this.loadMaterializedViews();
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

  private closeDetailThen(action: (mv: MaterializedView) => void): void {
    const mv = this.selectedMV;
    const dialogRef = this.detailDialogRef;
    if (!mv || !dialogRef) {
      return;
    }
    dialogRef.onClose.pipe(take(1)).subscribe(() => action(mv));
    this.closeDetailDialog(dialogRef);
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
