import { Component, OnInit, OnDestroy, inject } from '@angular/core';

import { NbToastrService, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbSelectModule, NbOptionModule, NbSpinnerModule, NbAlertModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { NodeService, Variable } from '../../../@core/data/node.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { FormsModule } from '@angular/forms';
import { assignTableRows } from '../../../@core/utils/table-rows';

@Component({
    selector: 'ngx-variables',
    templateUrl: './variables.component.html',
    styleUrls: ['./variables.component.scss'],
    imports: [
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbInputModule,
    FormsModule,
    NbSelectModule,
    NbOptionModule,
    NbSpinnerModule,
    NbAlertModule,
    Angular2SmartTableModule
],
})
export class VariablesComponent implements OnInit, OnDestroy {
  private toastrService = inject(NbToastrService);
  private dialogService = inject(NbDialogService);
  private clusterContext = inject(ClusterContextService);
  private nodeService = inject(NodeService);

  clusterId: number;
  activeCluster: Cluster | null = null;
  variables: Variable[] = [];
  source: LocalDataSource = new LocalDataSource();
  loading = true;
  searchText = '';
  variableType = 'global'; // 'global' or 'session'
  private destroy$ = new Subject<void>();

  settings = {
    hideSubHeader: false, // Enable search
    noDataMessage: '未找到匹配的变量',
    actions: {
      add: false,
      edit: true,
      delete: false,
      position: 'right',
    },
    edit: {
      editButtonContent: '<i class="nb-edit"></i>',
    },
    pager: {
      display: true,
      perPage: 20,
    },
    columns: {
      name: {
        title: 'Variable Name',
        type: 'string',
        width: '40%',
      },
      value: {
        title: 'Value',
        type: 'string',
        width: '60%',
        valuePrepareFunction: (value: any) => {
          if (!value) return 'NULL';
          return value.length > 200 ? value.substring(0, 200) + '...' : value;
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
            this.loadVariables();
          }
        }
        // Backend will handle "no active cluster" case
      });

    // Load variables - backend will get active cluster automatically
    this.loadVariables();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadVariables(): void {
    this.loading = true;
    this.nodeService.getVariables(
      
      this.variableType,
      this.searchText || undefined
    ).subscribe({
      next: (variables) => {
        this.variables = variables;
        assignTableRows(this.source, variables).then(() => {
          this.loading = false;
        });
      },
      error: (error) => {
        console.error('[Variables] Error loading variables:', error);
        this.toastrService.danger(
          error.error?.message || '加载变量失败',
          '错误'
        );
        this.variables = [];
        assignTableRows(this.source, []).then(() => {
          this.loading = false;
        });
      },
    });
  }

  onEdit(event: any): void {
    this.editVariable(event.data);
  }

  editVariable(variable: Variable): void {
    const newValue = prompt(`修改变量 "${variable.name}":`, variable.value);
    if (newValue !== null && newValue !== variable.value) {
      this.loading = true;
      this.nodeService.updateVariable(variable.name, {
        value: newValue,
        scope: this.variableType.toUpperCase(),
      }).subscribe({
        next: () => {
          this.toastrService.success(`变量 "${variable.name}" 更新成功`, '成功');
          this.loadVariables();
        },
        error: (error) => {
          this.toastrService.danger(
            error.error?.message || '更新变量失败',
            '错误'
          );
          this.loading = false;
        },
      });
    }
  }

  onTypeChange(): void {
    this.loadVariables();
  }

  onSearch(): void {
    this.loadVariables();
  }

  refresh(): void {
    this.loadVariables();
  }
}
