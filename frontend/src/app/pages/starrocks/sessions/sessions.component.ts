import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, inject, ChangeDetectorRef } from '@angular/core';
import { CommonModule } from '@angular/common';

import { NbToastrService, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbSelectModule, NbOptionModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { NodeService, Session } from '../../../@core/data/node.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { withTableRow } from '../../../@core/utils/smart-table';
import { MetricThresholds, renderMetricBadge } from '../../../@core/utils/metric-badge';
import { renderLongText } from '../../../@core/utils/text-truncate';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { assignTableRows } from '../../../@core/utils/table-rows';

import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router } from '@angular/router';

@Component({
    selector: 'ngx-sessions',
    templateUrl: './sessions.component.html',
    styleUrls: ['./sessions.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    FormsModule,
    NbButtonModule,
    NbIconModule,
    NbSelectModule,
    NbOptionModule,
    NbSpinnerModule,
    NbTooltipModule,
    Angular2SmartTableModule,
    CommonModule
],
})
export class SessionsComponent implements OnInit, OnDestroy {
  private toastrService = inject(NbToastrService)
  private i18n = inject(I18nService);
  private dialogService = inject(NbDialogService);
  private confirmDialogService = inject(ConfirmDialogService);
  private clusterContext = inject(ClusterContextService);
  private nodeService = inject(NodeService);
  private cdr = inject(ChangeDetectorRef);
  private route = inject(ActivatedRoute);
  private router = inject(Router);

  clusterId: number;
  activeCluster: Cluster | null = null;
  sessions: Session[] = [];
  source: LocalDataSource = new LocalDataSource();
  loading = true;
  private destroy$ = new Subject<void>();
  // Session duration thresholds: 1min(60s)=warn, 5min(300s)=danger
  private readonly sessionDurationThresholds: MetricThresholds = { warn: 60, danger: 300 };
  
  // Filter state for sessions
  sessionFilter: {
    sleepOnly?: boolean;
    slowOnly?: boolean;
  } = {};

  settings = {
    hideSubHeader: false, // Enable search
    noDataMessage: this.i18n.instant('当前没有活动会话'),
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
      id: {
        title: 'Session ID',
        type: 'string',
        width: '13%',
      },
      user: {
        title: 'User',
        type: 'string',
        width: '11%',
      },
      host: {
        title: 'Host',
        type: 'string',
        width: '13%',
      },
      db: {
        title: 'Database',
        type: 'string',
        width: '12%',
        valuePrepareFunction: (value: any) => value || 'N/A',
      },
      command: {
        title: 'Command',
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '12%',
        valuePrepareFunction: withTableRow((value: string, row: Session) => this.renderCommandBadge(value, row)),
      },
      time: {
        title: 'Time (s)',
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '11%',
        valuePrepareFunction: (value: string | number) => renderMetricBadge(value, this.sessionDurationThresholds),
      },
      info: {
        title: 'Info',
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '16%',
        valuePrepareFunction: (value: any) => {
          if (!value) return 'N/A';
          return renderLongText(value, 80);
        },
      },
    },
  };

  constructor() {
    // Try to get clusterId from route first
    // Get clusterId from ClusterContextService
    this.clusterId = this.clusterContext.getActiveClusterId() || 0;
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
            this.loadSessions();
          }
        }
        // Backend will handle "no active cluster" case
      });

    // 筛选条件 URL 参数化（复制链接即复现视图）
    const qp = this.route.snapshot.queryParamMap;
    if (qp.get('sleep') === '1') {
      this.sessionFilter.sleepOnly = true;
    }
    if (qp.get('slow') === '1') {
      this.sessionFilter.slowOnly = true;
    }

    // Load data - backend will get active cluster automatically
    this.loadSessions();
  }

  /** 筛选变化同步到 URL（replaceUrl，不污染历史） */
  private syncFilterToUrl(): void {
    this.router.navigate([], {
      relativeTo: this.route,
      queryParams: {
        ...(this.sessionFilter.sleepOnly ? { sleep: '1' } : {}),
        ...(this.sessionFilter.slowOnly ? { slow: '1' } : {}),
      },
      replaceUrl: true,
    });
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadSessions(): void {
    // Backend will get active cluster automatically - no need to check clusterId
    this.loading = true;
    this.cdr.markForCheck();
    this.nodeService.getSessions().pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: (allSessions) => {
        this.updateSessionsData(allSessions).then(() => {
          this.loading = false;
          this.cdr.markForCheck();
        });
      },
      error: (error) => {
        console.error('[Sessions] Error loading sessions:', error);
        this.toastrService.danger(
          ErrorHandler.handleClusterError(error),
          '错误'
        );
        this.sessions = [];
        assignTableRows(this.source, []).then(() => {
          this.loading = false;
          this.cdr.markForCheck();
        });
      },
    });
  }

  // Load sessions silently (for auto-refresh, no loading spinner)
  loadSessionsSilently(): void {
    // Only update data, don't show loading spinner during auto-refresh
    this.nodeService.getSessions().subscribe({
      next: (allSessions) => {
        this.updateSessionsData(allSessions);
      },
      error: (error) => {
        // Silently handle errors during auto-refresh, don't show toast
        console.error('[Sessions] Auto-refresh error:', error);
      },
    });
  }

  refresh(): void {
    this.loadSessions();
  }

  private updateSessionsData(allSessions: Session[]): Promise<void> {
    // Apply filters
    let filteredSessions = allSessions;

    if (this.sessionFilter.sleepOnly) {
      filteredSessions = filteredSessions.filter(s => {
        const cmdLower = s.command?.toLowerCase() || '';
        const stateLower = s.state?.toLowerCase() || '';
        return cmdLower === 'sleep' ||
               stateLower.includes('sleep') ||
               cmdLower === 'daemon';
      });
    }

    if (this.sessionFilter.slowOnly) {
      filteredSessions = filteredSessions.filter(s => {
        const time = this.parseTime(s.time);
        return time >= 60; // 1 minute
      });
    }

    this.sessions = filteredSessions;
    return assignTableRows(this.source, filteredSessions);
  }

  onDeleteConfirm(event: any): void {
    const session = event.data as Session;

    this.confirmDialogService.confirm(
      '确认终止会话',
      `确定要终止会话 ${session.id} 吗？`,
      '终止',
      '取消',
      'danger'
    ).subscribe(confirmed => {
      if (!confirmed) {
        event.confirm.reject();
        return;
      }

      this.loading = true;
      this.nodeService.killSession(session.id).subscribe({
        next: () => {
          this.toastrService.success(`会话 ${session.id} 已成功终止`, '成功');
          event.confirm.resolve();
          this.loadSessions();
        },
        error: (error) => {
          this.toastrService.danger(
            error.error?.message || '终止会话失败',
            '错误'
          );
          event.confirm.reject();
          this.loading = false;
        },
      });
    });
  }

  // Render command badge
  renderCommandBadge(value: string, row: Session): string {
    const cmd = (value || '').toLowerCase();
    if (cmd === 'sleep') {
      return '<span class="badge badge-secondary">Sleep</span>';
    } else if (cmd === 'query') {
      return '<span class="badge badge-primary">Query</span>';
    } else if (cmd === 'connect') {
      return '<span class="badge badge-info">Connect</span>';
    }
    return value || 'N/A';
  }

  // Parse time from string
  parseTime(value: string | number): number {
    if (typeof value === 'number') {
      return value;
    }
    const num = parseFloat(value.toString().replace(/[^0-9.-]/g, ''));
    return isNaN(num) ? 0 : num;
  }

  // Apply filter
  applySessionFilter(): void {
    this.loadSessions();
  }

  toggleSleepFilter(): void {
    this.sessionFilter.sleepOnly = !this.sessionFilter.sleepOnly;
    this.syncFilterToUrl();
    this.applySessionFilter();
  }

  toggleSlowFilter(): void {
    this.sessionFilter.slowOnly = !this.sessionFilter.slowOnly;
    this.syncFilterToUrl();
    this.applySessionFilter();
  }

  // Reset filter
  resetSessionFilter(): void {
    this.sessionFilter = {};
    this.syncFilterToUrl();
    this.loadSessions();
  }

  // Clear sleeping connections
  clearSleepingConnections(): void {
    // Get all sessions first (not filtered)
    this.nodeService.getSessions().subscribe({
      next: (allSessions) => {
        // Fix: StarRocks returns Command as "Sleep" (capitalized) or "Daemon"
        // We should check both command and info fields
        const sleepingSessions = allSessions.filter(s => {
          const cmdLower = s.command?.toLowerCase() || '';
          const stateLower = s.state?.toLowerCase() || '';
          
          // Match sleep connections: command is "Sleep" or state contains "sleep"
          return cmdLower === 'sleep' || 
                 stateLower.includes('sleep') ||
                 cmdLower === 'daemon'; // Daemon connections are also idle
        });

        if (sleepingSessions.length === 0) {
          this.toastrService.info(this.i18n.instant('当前没有睡眠连接'), this.i18n.instant('提示'));
          return;
        }

        this.confirmDialogService.confirm(
          '确认清除睡眠连接',
          `确定要清除 ${sleepingSessions.length} 个睡眠连接吗？`,
          '清除',
          '取消',
          'warning'
        ).subscribe(confirmed => {
          if (!confirmed) {
            return;
          }

          this.loading = true;
          let successCount = 0;
          let failCount = 0;
          let completed = 0;

          sleepingSessions.forEach(session => {
            this.nodeService.killSession(session.id).subscribe({
              next: () => {
                successCount++;
                completed++;
                if (completed === sleepingSessions.length) {
                  this.loading = false;
                  if (failCount === 0) {
                    this.toastrService.success(`成功清除 ${successCount} 个睡眠连接`, '成功');
                  } else {
                    this.toastrService.warning(`成功清除 ${successCount} 个，失败 ${failCount} 个`, '部分成功');
                  }
                  this.loadSessions();
                }
              },
              error: (error) => {
                failCount++;
                completed++;
                if (completed === sleepingSessions.length) {
                  this.loading = false;
                  if (successCount > 0) {
                    this.toastrService.warning(`成功清除 ${successCount} 个，失败 ${failCount} 个`, '部分成功');
                  } else {
                    this.toastrService.danger(this.i18n.instant('清除睡眠连接失败'), this.i18n.instant('错误'));
                  }
                  this.loadSessions();
                }
              },
            });
          });
        });
      },
      error: (error) => {
        this.toastrService.danger(
          ErrorHandler.handleClusterError(error),
          '获取会话列表失败'
        );
      },
    });
  }

  // Batch kill all displayed sessions
  batchKillAllSessions(): void {
    if (this.sessions.length === 0) {
      this.toastrService.warning(this.i18n.instant('当前没有可查杀的会话'), this.i18n.instant('提示'));
      return;
    }

    this.confirmDialogService.confirm(
      '确认批量查杀',
      `确定要查杀当前显示的 ${this.sessions.length} 个会话吗？`,
      '查杀',
      '取消',
      'danger'
    ).subscribe(confirmed => {
      if (!confirmed) {
        return;
      }

      this.loading = true;
      let successCount = 0;
      let failCount = 0;
      let completed = 0;

      this.sessions.forEach(session => {
        this.nodeService.killSession(session.id).subscribe({
          next: () => {
            successCount++;
            completed++;
            if (completed === this.sessions.length) {
              this.loading = false;
              if (failCount === 0) {
                this.toastrService.success(`成功查杀 ${successCount} 个会话`, '成功');
              } else {
                this.toastrService.warning(`成功查杀 ${successCount} 个，失败 ${failCount} 个`, '部分成功');
              }
              this.loadSessions();
            }
          },
          error: (error) => {
            failCount++;
            completed++;
            if (completed === this.sessions.length) {
              this.loading = false;
              if (successCount > 0) {
                this.toastrService.warning(`成功查杀 ${successCount} 个，失败 ${failCount} 个`, '部分成功');
              } else {
                this.toastrService.danger(this.i18n.instant('批量查杀失败'), this.i18n.instant('错误'));
              }
              this.loadSessions();
            }
          },
        });
      });
    });
  }
}
