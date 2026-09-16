import { Component, ChangeDetectorRef, OnInit, OnDestroy, inject } from '@angular/core';
import { Router } from '@angular/router';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbToastrService, NbSelectModule, NbOptionModule, NbButtonModule, NbIconModule } from '@nebular/theme';
import { ClusterService, Cluster } from '../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';


@Component({
    selector: 'ngx-cluster-selector',
    templateUrl: './cluster-selector.component.html',
    styleUrls: ['./cluster-selector.component.scss'],
    imports: [
    NbSelectModule,
    NbOptionModule,
    NbButtonModule,
    NbIconModule,
],
})
export class ClusterSelectorComponent implements OnInit, OnDestroy {
  private clusterService = inject(ClusterService);
  private cdRef = inject(ChangeDetectorRef);
  private clusterContext = inject(ClusterContextService);
  private router = inject(Router);
  private toastr = inject(NbToastrService);

  clusters: Cluster[] = [];
  activeCluster: Cluster | null = null;
  loading = false;
  private destroy$ = new Subject<void>();

  ngOnInit(): void {
    // Subscribe to active cluster changes
    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe(cluster => {
        this.activeCluster = cluster;
        this.cdRef.detectChanges();
      });

    // Load clusters
    this.loadClusters();

    // 集群增删改后刷新（否则 header 状态残留：有集群还显示“添加集群”）
    this.clusterService.clustersChanged$
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.loadClusters(true));
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadClusters(silent = false): void {
    if (!silent) {
      this.loading = true;
    }
    this.clusterService.listClusters().subscribe({
      next: (clusters) => {
        this.clusters = clusters;
        this.loading = false;
        this.cdRef.detectChanges(); // NG0100：异步赋值后手动检测（nb-select selectedIndex 同步）

        // The active cluster status comes from backend via the is_active field
        // Just need to refresh if no active cluster is shown
        if (clusters.length > 0 && !this.activeCluster) {
          // Refresh active cluster from backend
          this.clusterContext.refreshActiveCluster();
        }
      },
      error: (error) => {
        this.toastr.danger('加载集群列表失败', '错误');
        this.loading = false;
      },
    });
  }

  selectCluster(cluster: Cluster): void {
    this.clusterContext.setActiveCluster(cluster);
    this.toastr.success(`已切换到集群: ${cluster.name}`, '成功');
  }

  onClusterChange(cluster: Cluster): void {
    if (cluster) {
      this.selectCluster(cluster);
    }
  }

  compareById(c1: Cluster, c2: Cluster): boolean {
    return c1 && c2 ? c1.id === c2.id : c1 === c2;
  }

  goToClusterManagement(): void {
    this.router.navigate(['/pages/starrocks/clusters']);
  }

  refreshClusters(): void {
    this.loadClusters();
  }
}
