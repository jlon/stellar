import { Component, OnInit, OnDestroy } from '@angular/core';
import { Router } from '@angular/router';
import { NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { Subject, interval } from 'rxjs';
import { takeUntil, startWith, switchMap } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import { ResourceGroupUsage } from '../models/resource-group.model';

@Component({
  selector: 'ngx-resource-group-usage',
  templateUrl: './resource-group-usage.component.html',
  styleUrls: ['./resource-group-usage.component.scss'],
})
export class ResourceGroupUsageComponent implements OnInit, OnDestroy {
  private destroy$ = new Subject<void>();

  source: LocalDataSource = new LocalDataSource();
  loading = false;
  autoRefresh = true;
  refreshInterval = 30; // seconds

  settings = {
    actions: false,
    columns: {
      id: {
        title: '资源组 ID',
        type: 'number',
      },
      backend: {
        title: 'BE 节点',
        type: 'string',
      },
      be_in_use_cpu_cores: {
        title: 'CPU 使用核数',
        type: 'number',
        valuePrepareFunction: (value: number) => value.toFixed(2),
      },
      be_in_use_mem_bytes: {
        title: '内存使用量',
        type: 'string',
        valuePrepareFunction: (value: number) => this.formatBytes(value),
      },
      be_running_queries: {
        title: '运行中查询数',
        type: 'number',
      },
    },
  };

  constructor(
    private resourceGroupService: ResourceGroupService,
    private router: Router,
    private toastrService: NbToastrService,
  ) {}

  ngOnInit(): void {
    this.startAutoRefresh();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  private startAutoRefresh(): void {
    interval(this.refreshInterval * 1000)
      .pipe(
        startWith(0),
        switchMap(() => this.resourceGroupService.getResourceGroupUsage()),
        takeUntil(this.destroy$),
      )
      .subscribe({
        next: (usage) => {
          this.source.load(usage);
          this.loading = false;
        },
        error: (error) => {
          console.error('Failed to load resource group usage:', error);
          this.toastrService.danger('加载资源组使用情况失败', '错误');
          this.loading = false;
        },
      });
  }

  refreshData(): void {
    this.loading = true;
    this.resourceGroupService.getResourceGroupUsage()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (usage) => {
          this.source.load(usage);
          this.loading = false;
        },
        error: (error) => {
          console.error('Failed to load resource group usage:', error);
          this.toastrService.danger('加载资源组使用情况失败', '错误');
          this.loading = false;
        },
      });
  }

  toggleAutoRefresh(): void {
    this.autoRefresh = !this.autoRefresh;
    if (this.autoRefresh) {
      this.startAutoRefresh();
    }
  }

  goBack(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups']);
  }

  private formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';

    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));

    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  }
}