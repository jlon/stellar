import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { interval, Subject } from 'rxjs';
import { takeUntil, switchMap } from 'rxjs/operators';
import { NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { NodeService } from '../../../@core/data/node.service';
import { ClusterService, Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';


@Component({
    selector: 'ngx-frontends',
    templateUrl: './frontends.component.html',
    styleUrls: ['./frontends.component.scss'],
    imports: [
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbSpinnerModule,
    Angular2SmartTableModule
],
})
export class FrontendsComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService);
  private clusterService = inject(ClusterService);
  private clusterContext = inject(ClusterContextService);
  private toastrService = inject(NbToastrService);

  source: LocalDataSource = new LocalDataSource();
  clusterId: number;
  activeCluster: Cluster | null = null;
  clusterName: string = '';
  loading = true;
  private destroy$ = new Subject<void>();

  settings = {
    mode: 'external',
    hideSubHeader: false, // Enable search
    noDataMessage: '暂无Frontend节点数据',
    actions: false,
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      IP: { 
        title: '主机地址', 
        type: 'string',
        width: '15%',
      },
      HttpPort: { 
        title: 'HTTP端口', 
        type: 'string',
        width: '8%',
      },
      QueryPort: { 
        title: '查询端口', 
        type: 'string',
        width: '8%',
      },
      Role: { 
        title: '角色', 
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
        title: '状态',
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
        title: '日志进度ID', 
        type: 'string',
        width: '10%',
      },
      LastHeartbeat: { 
        title: '最后心跳', 
        type: 'string',
        width: '11%',
      },
      StartTime: { 
        title: '启动时间', 
        type: 'string',
        width: '11%',
      },
      Version: { 
        title: '版本', 
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
    this.loadClusterInfo();
    this.loadFrontends();
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
    this.loading = true;
    this.nodeService.listFrontends().subscribe({
      next: (frontends) => {
        this.source.load(frontends);
        this.loading = false;
      },
      error: (error) => {
        this.toastrService.danger(ErrorHandler.handleClusterError(error), '加载失败');
        this.loading = false;
      },
    });
  }
}
