import { Component, OnInit, OnDestroy } from '@angular/core';
import { Router } from '@angular/router';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';

import { ResourceGroupService } from '../resource-group.service';
import { ResourceGroup } from '../models/resource-group.model';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Cluster } from '../../../../@core/data/cluster.service';

@Component({
  selector: 'ngx-resource-groups-list',
  templateUrl: './resource-groups-list.component.html',
  styleUrls: ['./resource-groups-list.component.scss'],
})
export class ResourceGroupsListComponent implements OnInit, OnDestroy {
  private destroy$ = new Subject<void>();

  source: LocalDataSource = new LocalDataSource();
  loading = false;
  activeCluster: Cluster | null = null;

  settings = {
    add: {
      addButtonContent: '<i class="nb-plus"></i>',
      createButtonContent: '<i class="nb-checkmark"></i>',
      cancelButtonContent: '<i class="nb-close"></i>',
    },
    edit: {
      editButtonContent: '<i class="nb-edit"></i>',
      saveButtonContent: '<i class="nb-checkmark"></i>',
      cancelButtonContent: '<i class="nb-close"></i>',
    },
    delete: {
      deleteButtonContent: '<i class="nb-trash"></i>',
      confirmDelete: true,
    },
    actions: {
      columnTitle: '操作',
      add: false,
      edit: false,
      delete: false,
      position: 'right',
      custom: [
        {
          name: 'edit',
          title: '<i class="nb-edit" title="编辑"></i>',
        },
        {
          name: 'delete',
          title: '<i class="nb-trash" title="删除"></i>',
        },
      ],
    },
    columns: {
      name: {
        title: '资源组名称',
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
        title: 'CPU权重',
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      exclusive_cpu_cores: {
        title: '独占CPU核数',
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      mem_limit: {
        title: '内存限制',
        type: 'string',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      concurrency_limit: {
        title: '并发限制',
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any) => value || '-',
      },
      classifiers_count: {
        title: '分类器数量',
        type: 'number',
        editable: false,
        valuePrepareFunction: (value: any, row: ResourceGroup) => row.classifiers?.length || 0,
      },
    },
  };

  constructor(
    private resourceGroupService: ResourceGroupService,
    private router: Router,
    private dialogService: NbDialogService,
    private toastrService: NbToastrService,
    private clusterContext: ClusterContextService,
  ) {}

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
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (groups) => {
          this.source.load(groups);
          this.loading = false;
        },
        error: (error) => {
          console.error('Failed to load resource groups:', error);
          this.toastrService.danger('加载资源组列表失败', '错误');
          this.loading = false;
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
    this.router.navigate(['/pages/cluster-ops/resource-groups/create']);
  }

  editResourceGroup(group: ResourceGroup): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups/edit', group.name]);
  }

  deleteResourceGroup(group: ResourceGroup): void {
    this.dialogService
      .open(require('@angular/core').TemplateRef, {
        context: {
          title: '确认删除',
          message: `确定要删除资源组 "${group.name}" 吗？此操作不可撤销。`,
        },
      })
      .onClose.subscribe((confirmed) => {
        if (confirmed) {
          this.resourceGroupService
            .deleteResourceGroup(group.name)
            .pipe(takeUntil(this.destroy$))
            .subscribe({
              next: () => {
                this.toastrService.success('资源组删除成功', '成功');
                this.loadResourceGroups();
              },
              error: (error) => {
                console.error('Failed to delete resource group:', error);
                this.toastrService.danger('删除资源组失败', '错误');
              },
            });
        }
      });
  }

  viewUsage(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups/usage']);
  }

  viewAnalysis(): void {
    this.router.navigate(['/pages/cluster-ops/resource-groups/analysis']);
  }
}