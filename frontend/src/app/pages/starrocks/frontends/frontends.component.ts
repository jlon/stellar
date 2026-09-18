import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { interval, Subject } from 'rxjs';
import { takeUntil, switchMap, timeout } from 'rxjs/operators';
import { NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { NodeService } from '../../../@core/data/node.service';
import { ClusterService, Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { assignTableRows } from '../../../@core/utils/table-rows';


@Component({
    selector: 'ngx-frontends',
    templateUrl: './frontends.component.html',
    styleUrls: ['./frontends.component.scss'],
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
export class FrontendsComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService)
  private i18n = inject(I18nService);
  private clusterService = inject(ClusterService);
  private clusterContext = inject(ClusterContextService);
  private toastrService = inject(NbToastrService);

  source: LocalDataSource = new LocalDataSource();
  clusterId: number;
  activeCluster: Cluster | null = null;
  clusterName: string = '';
  loading = true;
  private destroy$ = new Subject<void>();
  private loadSeq = 0;

  settings = {
    mode: 'external',
    hideSubHeader: false, // Enable search
    noDataMessage: this.i18n.instant('暂无Frontend节点数据'),
    actions: false,
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      IP: { 
        title: this.i18n.instant('主机地址'), 
        type: 'string',
        width: '15%',
      },
      HttpPort: { 
        title: this.i18n.instant('HTTP端口'), 
        type: 'string',
        width: '8%',
      },
      QueryPort: { 
        title: this.i18n.instant('查询端口'), 
        type: 'string',
        width: '8%',
      },
      Role: { 
        title: this.i18n.instant('角色'), 
        type: 'html', 
        sanitizer: { bypassHtml: true },
        width: '9%',
        valuePrepareFunction: (value: string) => {
          if (value === 'LEADER') {
            return '<span class="badge badge-primary">LEADER</span>';
          } else if (value === 'FOLLOWER') {
            return '<span class="badge badge-info">FOLLOWER</span>';
          } else if (value === 'OBSERVER') {
            return '<span class="badge badge-warning">OBSERVER</span>';
          }
          return `<span class="badge badge-secondary">${value}</span>`;
        },
      },
      Alive: {
        title: this.i18n.instant('状态'),
        type: 'html',
        sanitizer: { bypassHtml: true },
        width: '7%',
        valuePrepareFunction: (value: string) => {
          const status = value === 'true' ? 'success' : 'danger';
          const text = value === 'true' ? '在线' : '离线';
          return `<span class="badge badge-${status}">${text}</span>`;
        },
      },
      ReplayedJournalId: { 
        title: this.i18n.instant('日志进度ID'), 
        type: 'string',
        width: '10%',
      },
      LastHeartbeat: { 
        title: this.i18n.instant('最后心跳'), 
        type: 'string',
        width: '11%',
      },
      StartTime: { 
        title: this.i18n.instant('启动时间'), 
        type: 'string',
        width: '11%',
      },
      Version: { 
        title: this.i18n.instant('版本'), 
        type: 'string',
        width: '9%',
      },
    },
  };

  constructor() {
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
            this.loadClusterInfo();
            this.loadFrontends();
          }
        }
        // Backend will handle "no active cluster" case
        
      });

    // Load data - backend will get active cluster automatically
    // clusterId 为 0 说明活动集群尚未就绪（此前会打出 clusters/0 的无效请求），
    // 等 activeCluster$ 推送后再加载，避免无效请求与 pageerror 噪音。
    if (this.clusterId) {
      this.loadClusterInfo();
      this.loadFrontends();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadClusterInfo(): void {
    this.clusterService.getCluster(this.clusterId).subscribe({
      next: (cluster) => {
        this.clusterName = cluster.name;
      },
    });
  }

  loadFrontends(): void {
    const seq = ++this.loadSeq;
    this.loading = true;
    // 与 backends 页一致：20s 超时兜底，hung 住时给明确错误而不是无限转圈。
    this.nodeService.listFrontends()
      .pipe(takeUntil(this.destroy$), timeout(20000))
      .subscribe({
      next: (frontends) => {
        if (seq !== this.loadSeq) {
          return;
        }
        assignTableRows(this.source, frontends).then(() => {
          if (seq === this.loadSeq) {
            this.loading = false;
          }
        });
      },
      error: (error) => {
        if (seq !== this.loadSeq) {
          return;
        }
        this.toastrService.danger(ErrorHandler.handleClusterError(error), '加载失败');
        assignTableRows(this.source, []).then(() => {
          if (seq === this.loadSeq) {
            this.loading = false;
          }
        });
      },
    });
  }
}
