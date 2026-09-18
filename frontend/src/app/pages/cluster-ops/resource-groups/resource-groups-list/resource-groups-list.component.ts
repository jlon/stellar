import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Router } from '@angular/router';
import { NbToastrService, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import { ResourceGroup } from '../models/resource-group.model';
import { withTableRow } from '../../../../@core/utils/smart-table';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Cluster } from '../../../../@core/data/cluster.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { ResourceGroupFormComponent } from '../resource-group-form/resource-group-form.component';
import { assignTableRows } from '../../../../@core/utils/table-rows';


@Component({
    selector: 'ngx-resource-groups-list',
    templateUrl: './resource-groups-list.component.html',
    styleUrls: ['./resource-groups-list.component.scss'],
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
export class ResourceGroupsListComponent implements OnInit, OnDestroy {
  private resourceGroupService = inject(ResourceGroupService)
  private i18n = inject(I18nService);
  private router = inject(Router);
  private dialogService = inject(NbDialogService);
  private confirmDialog = inject(ConfirmDialogService);
  private toastrService = inject(NbToastrService);
  private clusterContext = inject(ClusterContextService);

  private destroy$ = new Subject<void>();

  source: LocalDataSource = new LocalDataSource();
  loading = true;
  activeCluster: Cluster | null = null;

  settings = {
    edit: {
      saveButtonContent: '<i class="nb-checkmark" title="保存"></i>',
      cancelButtonContent: '<i class="nb-close" title="取消"></i>',
    },
    delete: {
      deleteButtonContent: '<i class="nb-trash" title="删除"></i>',
      confirmDelete: true,
    },
    actions: {
      columnTitle: this.i18n.instant('操作'),
      add: false,
      edit: false,
      delete: false,
      position: 'right',
      custom: [
        {
          name: 'edit',
          title: this.i18n.instant('<i class="nb-edit" title="编辑"></i>'),
        },
        {
          name: 'delete',
          title: this.i18n.instant('<i class="nb-trash" title="删除"></i>'),
        },
      ],
    },
    columns: {
      name: {
        title: this.i18n.instant('资源组名称'),
        type: 'string',
        editable: false,
      },
      id: {
        title: 'ID',
        type: 'number',
        editable: false,
        width: '5%',
      },
      cpu_weight: {
        title: this.i18n.instant('CPU权重'),
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      exclusive_cpu_cores: {
        title: this.i18n.instant('独占CPU核数'),
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      mem_limit: {
        title: this.i18n.instant('内存限制'),
        type: 'string',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      concurrency_limit: {
        title: this.i18n.instant('并发限制'),
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      classifiers_count: {
        title: this.i18n.instant('分类器数量'),
        type: 'number',
        editable: false,
        valuePrepareFunction: withTableRow((value: any, row: ResourceGroup) => row.classifiers?.length || 0),
      },
    },
  };

  ngOnInit(): void {
    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe((cluster) => {
        this.activeCluster = cluster;
      });
    
    this.loadResourceGroups();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadResourceGroups(): void {
    this.loading = true;
    this.resourceGroupService
      .getResourceGroups()
      .pipe(takeUntil(this.destroy$), timeout(20000))
      .subscribe({
        next: (groups) => {
          assignTableRows(this.source, groups).then(() => {
            this.loading = false;
          });
        },
        error: (error) => {
          console.error('Failed to load resource groups:', error);
          this.toastrService.danger(this.i18n.instant('加载资源组列表失败'), this.i18n.instant('错误'));
          assignTableRows(this.source, []).then(() => {
            this.loading = false;
          });
        },
      });
  }

  onCustomAction(event: any): void {
    const { action, data } = event;

    switch (action) {
      case 'edit':
        this.editResourceGroup(data);
        break;
      case 'delete':
        this.deleteResourceGroup(data);
        break;
    }
  }

  createResourceGroup(): void {
    this.dialogService
      .open(ResourceGroupFormComponent, { context: { groupName: null } })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadResourceGroups();
        }
      });
  }

  editResourceGroup(group: ResourceGroup): void {
    this.dialogService
      .open(ResourceGroupFormComponent, { context: { groupName: group.name } })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadResourceGroups();
        }
      });
  }

  deleteResourceGroup(group: ResourceGroup): void {
    this.confirmDialog.confirmDelete(group.name, '此操作不可撤销。').subscribe((confirmed) => {
      if (!confirmed) {
        return;
      }
      this.resourceGroupService
        .deleteResourceGroup(group.name)
        .pipe(takeUntil(this.destroy$), timeout(20000))
        .subscribe({
          next: () => {
            this.toastrService.success(this.i18n.instant('资源组删除成功'), this.i18n.instant('成功'));
            this.loadResourceGroups();
          },
          error: () => {
            this.toastrService.danger(this.i18n.instant('删除资源组失败'), this.i18n.instant('错误'));
          },
        });
    });
  }

  viewUsage(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups/usage']);
  }

  viewAnalysis(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups/analysis']);
  }
}