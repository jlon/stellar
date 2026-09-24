import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Router } from '@angular/router';
import { NbAlertModule, NbButtonModule, NbCardModule, NbIconModule, NbOptionModule, NbSelectModule, NbSpinnerModule, NbTabsetModule, NbToastrService, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import { ResourceUsageAnalysis } from '../models/resource-group.model';


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

  goBack(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups']);
  }
}
