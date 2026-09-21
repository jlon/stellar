import { TranslatePipe } from '@ngx-translate/core';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { Component, OnInit, OnDestroy, TemplateRef, ViewChild, inject, ChangeDetectorRef } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { NbToastrService, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbSelectModule, NbOptionModule, NbFormFieldModule, NbInputModule, NbDatepickerModule, NbBadgeModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { NodeService, QueryHistoryItem } from '../../../../@core/data/node.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Cluster } from '../../../../@core/data/cluster.service';
import { ErrorHandler } from '../../../../@core/utils/error-handler';
import { MetricThresholds, renderMetricBadge } from '../../../../@core/utils/metric-badge';
import { renderLongText } from '../../../../@core/utils/text-truncate';
import { assignTableRows } from '../../../../@core/utils/table-rows';
import { TablePaginationComponent } from '../../../../@theme/components/table-pagination/table-pagination.component';

import { FormsModule } from '@angular/forms';

@Component({
    selector: 'ngx-audit-logs',
    templateUrl: './audit-logs.component.html',
    styleUrls: ['./audit-logs.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbSelectModule,
    NbOptionModule,
    NbFormFieldModule,
    NbInputModule,
    NbDatepickerModule,
    FormsModule,
    NbBadgeModule,
    NbSpinnerModule,
    NbTooltipModule,
    TablePaginationComponent,
    Angular2SmartTableModule
],
})
export class AuditLogsComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService);
  private i18n = inject(I18nService);
  private cdr = inject(ChangeDetectorRef);
  private route = inject(ActivatedRoute);
  private router = inject(Router);
  private toastrService = inject(NbToastrService);
  private clusterContext = inject(ClusterContextService);
  private dialogService = inject(NbDialogService);

  // Data sources
  historySource: LocalDataSource = new LocalDataSource();
  
  // State
  clusterId: number;
  activeCluster: Cluster | null = null;
  loading = true;
  private destroy$ = new Subject<void>();
  private readonly durationThresholds: MetricThresholds = { warn: 3000, danger: 10000 };

  // Profile dialog
  currentProfile: any = null;
  @ViewChild('profileDialog') profileDialogTemplate: TemplateRef<any>;

  // History search filters
  searchKeyword: string = '';
  searchStartTime: Date | null = null;
  searchEndTime: Date | null = null;

  // Pagination state for history
  historyPageSize: number = 10;
  historyCurrentPage: number = 1;
  historyTotalCount: number = 0;

  // History queries settings with Profile button
  historySettings = {
    mode: 'external',
    hideSubHeader: false, // Enable search
    noDataMessage: this.i18n.instant('暂无审计日志记录'),
    actions: {
      add: false,
      edit: true,
      delete: false,
      position: 'right',
      columnTitle: this.i18n.instant('操作'),
    },
    edit: {
      editButtonContent: '<i class="nb-search" title="查看"></i>',
    },
    pager: {
      display: false, // Disable ng2-smart-table's built-in pagination (we'll use custom pagination)
    },
    columns: {
      query_id: { title: 'Query ID', type: 'string' },
      user: { title: this.i18n.instant('用户'), type: 'string', width: '8%' },
      default_db: { title: this.i18n.instant('数据库'), type: 'string', width: '8%' },
      query_type: { title: this.i18n.instant('类型'), type: 'string', width: '8%' },
      query_state: { title: this.i18n.instant('状态'), type: 'string', width: '8%' },
      start_time: { title: this.i18n.instant('开始时间'), type: 'string', width: '12%' },
      total_ms: {
        title: this.i18n.instant('耗时(ms)'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '8%',
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.durationThresholds),
      },
      sql_statement: { 
        title: 'SQL', 
        type: 'html',
        sanitizer: { bypassHtml: true },
        valuePrepareFunction: (value: any) => renderLongText(value, 100),
      },
    },
  };

  constructor() {
    // Try to get clusterId from route first (for direct navigation)
    const routeClusterId = parseInt(this.route.snapshot.paramMap.get('clusterId') || '0', 10);
    this.clusterId = routeClusterId;
  }

  ngOnInit(): void {
    // Subscribe to active cluster changes
    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe(cluster => {
        this.activeCluster = cluster;
        if (cluster) {
          // Always use the active cluster (override route parameter)
          const newClusterId = cluster.id;
          if (this.clusterId !== newClusterId) {
            this.clusterId = newClusterId;
            // Reset pagination when cluster changes
            this.historyCurrentPage = 1;
            this.loadHistoryQueries();
          }
        }
        // Backend will handle "no active cluster" case
      });

    // 筛选条件 URL 参数化（复制链接即复现视图）
    const qp = this.route.snapshot.queryParamMap;
    if (qp.get('q')) {
      this.searchKeyword = qp.get('q')!;
    }
    if (qp.get('from')) {
      this.searchStartTime = this.parseDateTime(qp.get('from')!);
    }
    if (qp.get('to')) {
      this.searchEndTime = this.parseDateTime(qp.get('to')!);
    }

    // Load data - backend will get active cluster automatically
    this.loadHistoryQueries();
  }

  /** 筛选变化同步到 URL（replaceUrl，不污染历史） */
  private syncFilterToUrl(): void {
    this.router.navigate([], {
      relativeTo: this.route,
      queryParams: {
        ...(this.searchKeyword?.trim() ? { q: this.searchKeyword.trim() } : {}),
        ...(this.searchStartTime ? { from: this.dateTimeValue(this.searchStartTime) } : {}),
        ...(this.searchEndTime ? { to: this.dateTimeValue(this.searchEndTime) } : {}),
      },
      replaceUrl: true,
    });
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  // Load query history with pagination and filters
  loadHistoryQueries(): void {
    this.loading = true;
    
    // Prepare filters
    const filters = {
      keyword: this.searchKeyword?.trim() || undefined,
      startTime: this.dateTimeValue(this.searchStartTime) || undefined,
      endTime: this.dateTimeValue(this.searchEndTime) || undefined,
    };
    
    this.nodeService
      .listQueryHistory(
        this.historyPageSize, 
        (this.historyCurrentPage - 1) * this.historyPageSize,
        filters
      )
      .pipe(takeUntil(this.destroy$), timeout(20000))
      .subscribe({
        next: (data) => {
          this.historyTotalCount = data.total;
          assignTableRows(this.historySource, data.data).then(() => {
            this.loading = false;
            this.cdr.markForCheck();
          });
        },
        error: (error) => {
          this.toastrService.danger(
            ErrorHandler.handleClusterError(error),
            '加载失败'
          );
          assignTableRows(this.historySource, []).then(() => {
            this.loading = false;
            this.cdr.markForCheck();
          });
        },
      });
  }

  // Calculate total pages
  get historyTotalPages(): number {
    return Math.ceil(this.historyTotalCount / this.historyPageSize);
  }

  // Handle page change
  onHistoryPageChange(page: number): void {
    if (page < 1 || page > this.historyTotalPages) {
      return;
    }
    this.historyCurrentPage = page;
    this.loadHistoryQueries();
  }

  // Handle page size change
  onHistoryPageSizeChange(size: number): void {
    this.historyPageSize = size;
    this.historyCurrentPage = 1; // Reset to first page
    this.loadHistoryQueries();
  }

  // Handle edit action (View Profile)
  onEditProfile(event: any): void {
    const query: QueryHistoryItem = event.data;
    this.viewProfile(query.query_id);
  }

  // View query profile
  viewProfile(queryId: string): void {
    this.nodeService.getQueryProfile(queryId).subscribe({
      next: (profile) => {
        this.currentProfile = profile;
        // Open profile dialog
        this.dialogService.open(this.profileDialogTemplate, {
          context: { profile },
        });
      },
      error: (error) => {
        this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '加载失败');
      },
    });
  }

  // Search history methods
  searchHistory(): void {
    this.syncFilterToUrl();
    this.loadHistoryQueries();
  }

  // Check if there are active filters
  hasActiveFilters(): boolean {
    return !!(this.searchKeyword?.trim() || this.searchStartTime || this.searchEndTime);
  }

  // Clear all filters
  clearFilters(): void {
    this.searchKeyword = '';
    this.searchStartTime = null;
    this.searchEndTime = null;
    this.searchHistory();
  }

  // Note: Filtering is now handled by the backend API
  // This method is kept for reference but no longer used
  applyHistoryFilters(queries: QueryHistoryItem[]): QueryHistoryItem[] {
    // Backend handles filtering now, so this method is not used
    return queries;
  }

  /** Keep the API and URL contract used by the previous native datetime-local inputs. */
  dateTimeValue(value: Date | null): string {
    if (!value || Number.isNaN(value.getTime())) {
      return '';
    }
    const pad = (part: number) => String(part).padStart(2, '0');
    return `${value.getFullYear()}-${pad(value.getMonth() + 1)}-${pad(value.getDate())}T${pad(value.getHours())}:${pad(value.getMinutes())}`;
  }

  private parseDateTime(value: string): Date | null {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed;
  }
}
