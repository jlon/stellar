import { CommonModule } from '@angular/common';
import { ChangeDetectionStrategy, ChangeDetectorRef, Component, HostListener, OnDestroy, OnInit, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import {
  NbAlertModule,
  NbButtonModule,
  NbCardModule,
  NbFormFieldModule,
  NbIconModule,
  NbInputModule,
  NbOptionModule,
  NbProgressBarModule,
  NbSelectModule,
  NbSpinnerModule,
  NbTooltipModule,
} from '@nebular/theme';
import { Subject, interval } from 'rxjs';
import { finalize, take, takeUntil, timeout } from 'rxjs/operators';
import { ActivatedRoute, Router } from '@angular/router';

import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { LoadJob, LoadService } from '../../../@core/data/load.service';
import { NodeService } from '../../../@core/data/node.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

@Component({
  selector: 'ngx-load-management',
  templateUrl: './load-management.component.html',
  styleUrls: ['./load-management.component.scss'],
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    CommonModule,
    FormsModule,
    NbAlertModule,
    NbButtonModule,
    NbCardModule,
    NbFormFieldModule,
    NbIconModule,
    NbInputModule,
    NbOptionModule,
    NbProgressBarModule,
    NbSelectModule,
    NbSpinnerModule,
    NbTooltipModule,
  ],
})
export class LoadManagementComponent implements OnInit, OnDestroy {
  private readonly clusterContext = inject(ClusterContextService);
  private readonly loadService = inject(LoadService);
  private readonly nodeService = inject(NodeService);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly destroy$ = new Subject<void>();

  activeCluster: Cluster | null = null;
  databases: string[] = [];
  jobs: LoadJob[] = [];
  selectedJob: LoadJob | null = null;
  loading = false;
  refreshing = false;
  errorMessage = '';
  lastUpdated: Date | null = null;

  filters: {
    db: string;
    type: string;
    state: string;
    search: string;
    range: '24h' | '7d' | '30d' | 'all';
  } = {
    db: '',
    type: '',
    state: '',
    search: '',
    range: '24h',
  };

  summary = {
    running: 0,
    queued: 0,
    failed: 0,
    finished: 0,
  };

  ngOnInit(): void {
    const db = this.route.snapshot.queryParamMap.get('db');
    if (db) {
      this.filters.db = db;
    }

    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe(cluster => {
        this.activeCluster = cluster;
        if (!cluster) {
          this.jobs = [];
          this.selectedJob = null;
          this.cdr.markForCheck();
          return;
        }
        this.loadDatabases();
        this.loadJobs();
      });

    interval(30_000)
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => {
        if (this.activeCluster && !this.loading && !this.refreshing) {
          this.loadJobs(true);
        }
      });
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadDatabases(): void {
    this.nodeService.getDatabases().pipe(take(1), timeout(20_000)).subscribe({
      next: databases => {
        this.databases = databases.filter(name => !['information_schema', '_statistics_'].includes(name));
        this.cdr.markForCheck();
      },
      error: () => {
        this.databases = [];
        this.cdr.markForCheck();
      },
    });
  }

  loadJobs(silent = false): void {
    if (!this.activeCluster) {
      return;
    }
    if (silent) {
      this.refreshing = true;
    } else {
      this.loading = true;
    }
    this.errorMessage = '';
    this.cdr.markForCheck();

    this.loadService.list({
      db: this.filters.db || undefined,
      type: this.filters.type || undefined,
      state: this.filters.state || undefined,
      search: this.filters.search || undefined,
      range: this.filters.range,
      limit: 200,
    }).pipe(
      take(1),
      timeout(20_000),
      finalize(() => {
        this.loading = false;
        this.refreshing = false;
        this.cdr.markForCheck();
      }),
    ).subscribe({
      next: response => {
        this.jobs = response.items;
        this.summary = response.summary;
        this.lastUpdated = new Date();
        if (this.selectedJob) {
          this.selectedJob = this.jobs.find(job => job.job_id === this.selectedJob?.job_id) || null;
        }
        this.cdr.markForCheck();
      },
      error: error => {
        this.errorMessage = ErrorHandler.handleClusterError(error);
        this.jobs = [];
        this.summary = { running: 0, queued: 0, failed: 0, finished: 0 };
        this.cdr.markForCheck();
      },
    });
  }

  applyFilters(): void {
    this.syncFiltersToUrl();
    this.loadJobs();
  }

  clearFilters(): void {
    this.filters = { db: '', type: '', state: '', search: '', range: '24h' };
    this.syncFiltersToUrl();
    this.loadJobs();
  }

  onSearchKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      this.applyFilters();
    }
  }

  openDetails(job: LoadJob): void {
    this.selectedJob = job;
  }

  onRowKeydown(event: KeyboardEvent, job: LoadJob): void {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      this.openDetails(job);
    }
  }

  @HostListener('document:keydown.escape')
  closeDetails(): void {
    this.selectedJob = null;
  }

  goToQueryExecution(): void {
    void this.router.navigate(['/pages/starrocks/queries/execution']);
  }

  trackJob(index: number, job: LoadJob): string {
    return job.job_id || job.label || String(index);
  }

  statusLabel(state: string): string {
    const labels: Record<string, string> = {
      pending: '待处理',
      queueing: '排队中',
      before_load: '准备中',
      loading: '导入中',
      preparing: '准备提交',
      prepared: '待提交',
      committed: '已提交',
      commited: '已提交',
      finished: '已完成',
      cancelled: '已取消',
      canceled: '已取消',
      failed: '失败',
    };
    return labels[state.toLowerCase()] || state || '未知';
  }

  statusClass(state: string): string {
    const value = state.toLowerCase();
    if (value.includes('cancel') || value.includes('fail') || value.includes('error')) return 'failed';
    if (['finished', 'committed', 'commited', 'success', 'succeed'].includes(value)) return 'finished';
    if (['pending', 'queueing', 'before_load', 'queued'].includes(value)) return 'queued';
    return 'running';
  }

  typeLabel(type: string): string {
    const labels: Record<string, string> = {
      stream_load: 'Stream Load',
      routine_load: 'Routine Load',
      broker_load: 'Broker Load',
      spark_load: 'Spark Load',
      insert: 'INSERT',
    };
    return labels[type.toLowerCase()] || type || '未知';
  }

  progressValue(job: LoadJob): number {
    const progress = job.progress || '';
    const load = /load\s*:\s*(\d+(?:\.\d+)?)%/i.exec(progress)?.[1];
    const etl = /etl\s*:\s*(\d+(?:\.\d+)?)%/i.exec(progress)?.[1];
    const value = Number.parseFloat(
      job.state.toLowerCase() === 'etl' ? (etl ?? load ?? progress) : (load ?? etl ?? progress),
    );
    return Number.isFinite(value) ? Math.max(0, Math.min(100, value)) : 0;
  }

  formatNumber(value?: number): string {
    return value === undefined || value === null ? '-' : new Intl.NumberFormat('zh-CN').format(value);
  }

  formatBytes(value?: number): string {
    if (value === undefined || value === null) return '-';
    if (value < 1024) return `${this.formatNumber(value)} B`;
    const units = ['KB', 'MB', 'GB', 'TB'];
    let amount = value;
    let unit = 'B';
    for (const nextUnit of units) {
      amount /= 1024;
      unit = nextUnit;
      if (amount < 1024) break;
    }
    return `${amount.toFixed(amount >= 10 ? 0 : 1)} ${unit}`;
  }

  formatDuration(job: LoadJob): string {
    const duration = job.stage_timeline.reduce((total, stage) => total + stage.duration_ms, 0);
    if (!duration) return '-';
    if (duration < 1000) return `${duration} ms`;
    const seconds = Math.round(duration / 1000);
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  }

  hasQualityWarning(job: LoadJob): boolean {
    if (job.filtered_rows === undefined || job.scan_rows === undefined) return false;
    return job.scan_rows > 0 && job.filtered_rows / job.scan_rows > 0.01;
  }

  qualityRate(job: LoadJob): string {
    if (job.filtered_rows === undefined || job.scan_rows === undefined) return '-';
    if (job.scan_rows <= 0) return '0%';
    const rate = (job.filtered_rows / job.scan_rows) * 100;
    return `${rate.toFixed(rate >= 10 ? 0 : 1)}%`;
  }

  prettyJson(value?: string): string {
    if (!value) return '';
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }

  private syncFiltersToUrl(): void {
    void this.router.navigate([], {
      relativeTo: this.route,
      queryParams: {
        ...(this.filters.db ? { db: this.filters.db } : {}),
        ...(this.filters.type ? { type: this.filters.type } : {}),
        ...(this.filters.state ? { state: this.filters.state } : {}),
        ...(this.filters.search ? { search: this.filters.search } : {}),
        ...(this.filters.range !== '24h' ? { range: this.filters.range } : {}),
      },
      replaceUrl: true,
    });
  }
}
