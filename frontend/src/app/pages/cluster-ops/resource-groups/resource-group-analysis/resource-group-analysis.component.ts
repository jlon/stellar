import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Router } from '@angular/router';
import { NbAlertModule, NbButtonModule, NbCardModule, NbIconModule, NbOptionModule, NbSelectModule, NbSpinnerModule, NbTabsetModule, NbToastrService, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import {
  ResourceUsageAnalysis,
  UserCpuUsage,
  UserMemoryUsage,
  UserConcurrency,
} from '../models/resource-group.model';


@Component({
    selector: 'ngx-resource-group-analysis',
    templateUrl: './resource-group-analysis.component.html',
    styleUrls: ['./resource-group-analysis.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbSelectModule,
    NbOptionModule,
    NbButtonModule,
    NbTooltipModule,
    NbIconModule,
    NbSpinnerModule,
    NbAlertModule,
    NbTabsetModule,
    Angular2SmartTableModule
],
})
export class ResourceGroupAnalysisComponent implements OnInit, OnDestroy {
  private resourceGroupService = inject(ResourceGroupService)
  private i18n = inject(I18nService);
  private router = inject(Router);
  private toastrService = inject(NbToastrService);

  private destroy$ = new Subject<void>();

  loading = false;
  analysisData: ResourceUsageAnalysis | null = null;
  selectedDays = 30;

  cpuSource: LocalDataSource = new LocalDataSource();
  memorySource: LocalDataSource = new LocalDataSource();
  concurrencySource: LocalDataSource = new LocalDataSource();

  daysOptions = [
    { value: 7, label: '最近 7 天' },
    { value: 30, label: '最近 30 天' },
    { value: 90, label: '最近 90 天' },
  ];

  cpuSettings = {
    actions: false,
    columns: {
      user: {
        title: this.i18n.instant('用户'),
        type: 'string',
      },
      total_cpu_seconds: {
        title: this.i18n.instant('CPU 总时间 (秒)'),
        type: 'number',
        valuePrepareFunction: (value: number) => value.toFixed(2),
      },
      cpu_usage_percentage: {
        title: this.i18n.instant('CPU 使用占比 (%)'),
        type: 'number',
        valuePrepareFunction: (value: number) => value.toFixed(2) + '%',
      },
      suggested_cpu_weight: {
        title: this.i18n.instant('建议 CPU 权重'),
        type: 'number',
      },
      suggested_exclusive_cores: {
        title: this.i18n.instant('建议独占核数'),
        type: 'number',
      },
    },
  };

  memorySettings = {
    actions: false,
    columns: {
      user: {
        title: this.i18n.instant('用户'),
        type: 'string',
      },
      max_mem_mb: {
        title: this.i18n.instant('最大内存 (MB)'),
        type: 'number',
        valuePrepareFunction: (value: number) => value.toFixed(2),
      },
      suggested_mem_limit: {
        title: this.i18n.instant('建议内存限制'),
        type: 'string',
      },
      suggested_big_query_mem_limit: {
        title: this.i18n.instant('建议大查询内存限制'),
        type: 'string',
      },
    },
  };

  concurrencySettings = {
    actions: false,
    columns: {
      user: {
        title: this.i18n.instant('用户'),
        type: 'string',
      },
      max_concurrency_per_second: {
        title: this.i18n.instant('最大并发 (每秒)'),
        type: 'number',
        valuePrepareFunction: (value: number) => value.toFixed(2),
      },
      suggested_concurrency_limit: {
        title: this.i18n.instant('建议并发限制'),
        type: 'number',
      },
    },
  };

  ngOnInit(): void {
    this.loadAnalysis();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadAnalysis(): void {
    this.loading = true;
    this.resourceGroupService
      .getResourceUsageAnalysis(this.selectedDays)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (analysis) => {
          this.analysisData = analysis;
          this.cpuSource.load(analysis.cpu_analysis);
          this.memorySource.load(analysis.memory_analysis);
          this.concurrencySource.load(analysis.concurrency_analysis);
          this.loading = false;
        },
        error: (error) => {
          console.error('Failed to load resource usage analysis:', error);
          this.toastrService.danger(this.i18n.instant('加载资源使用分析失败'), this.i18n.instant('错误'));
          this.loading = false;
        },
      });
  }

  onDaysChange(): void {
    this.loadAnalysis();
  }

  generateResourceGroupSQL(user: string): void {
    if (!this.analysisData) return;

    const cpuData = this.analysisData.cpu_analysis.find((c) => c.user === user);
    const memoryData = this.analysisData.memory_analysis.find((m) => m.user === user);
    const concurrencyData = this.analysisData.concurrency_analysis.find((c) => c.user === user);

    let sql = `CREATE RESOURCE GROUP ${user}_group\nTO (\n  user='${user}'\n)\nWITH (\n`;

    const withClauses = [];

    if (cpuData) {
      if (cpuData.suggested_exclusive_cores > 0) {
        withClauses.push(`  'exclusive_cpu_cores' = '${cpuData.suggested_exclusive_cores}'`);
      } else {
        withClauses.push(`  'cpu_weight' = '${cpuData.suggested_cpu_weight}'`);
      }
    }

    if (memoryData) {
      withClauses.push(`  'mem_limit' = '${memoryData.suggested_mem_limit}'`);
      withClauses.push(`  'big_query_mem_limit' = '${memoryData.suggested_big_query_mem_limit}'`);
    }

    if (concurrencyData) {
      withClauses.push(`  'concurrency_limit' = '${concurrencyData.suggested_concurrency_limit}'`);
    }

    sql += withClauses.join(',\n') + '\n);';

    // 复制到剪贴板
    navigator.clipboard.writeText(sql).then(() => {
      this.toastrService.success(`用户 ${user} 的资源组 SQL 已复制到剪贴板`, '成功');
    });
  }

  goBack(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups']);
  }
}