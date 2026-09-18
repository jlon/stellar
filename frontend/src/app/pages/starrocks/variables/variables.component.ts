import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, ChangeDetectorRef, inject } from '@angular/core';

import { NbToastrService, NbDialogService, NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbFormFieldModule, NbSelectModule, NbOptionModule, NbSpinnerModule, NbAlertModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { NodeService, Variable } from '../../../@core/data/node.service';
import { FormsModule } from '@angular/forms';
import { assignTableRows } from '../../../@core/utils/table-rows';
import { VariableEditDialogComponent } from './variable-edit-dialog/variable-edit-dialog.component';
import { VariableActionsCellComponent } from './variable-actions-cell.component';

@Component({
    selector: 'ngx-variables',
    templateUrl: './variables.component.html',
    styleUrls: ['./variables.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbInputModule,
    FormsModule,
    NbSelectModule,
    NbOptionModule,
    NbSpinnerModule,
    NbAlertModule,
    NbTooltipModule,
    Angular2SmartTableModule,
    NbFormFieldModule,
    VariableActionsCellComponent
],
})
export class VariablesComponent implements OnInit, OnDestroy {
  private toastrService = inject(NbToastrService)
  private i18n = inject(I18nService);
  private dialogService = inject(NbDialogService);
  private cdRef = inject(ChangeDetectorRef);
  private clusterContext = inject(ClusterContextService);
  private nodeService = inject(NodeService);

  clusterId: number;
  activeCluster: Cluster | null = null;
  variables: Variable[] = [];
  source: LocalDataSource = new LocalDataSource();
  loading = true;
  loadError = '';
  searchText = '';
  variableType = 'global'; // 'global' or 'session'
  private destroy$ = new Subject<void>();

  settings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: this.i18n.instant('暂无变量数据'),
    actions: false,
    pager: {
      display: true,
      perPage: 20,
    },
    columns: {
      name: {
        title: this.i18n.instant('变量名'),
        type: 'string',
        width: '35%',
      },
      value: {
        title: this.i18n.instant('当前值'),
        type: 'string',
        width: 'auto',
        valuePrepareFunction: (value: string) => {
          if (value === null || value === undefined || value === '') return 'NULL';
          return value.length > 200 ? `${value.slice(0, 200)}…` : value;
        },
      },
      action: {
        title: this.i18n.instant('操作'),
        type: 'custom',
        width: '5rem',
        isFilterable: false,
        isSortable: false,
        renderComponent: VariableActionsCellComponent,
        componentInitFunction: (instance: VariableActionsCellComponent, cell: any) => {
          instance.variable = cell.getRow().getData() as Variable;
          instance.edit.subscribe((variable: Variable) => this.editVariable(variable));
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
    this.loadError = '';
    this.nodeService.getVariables(
      
      this.variableType,
      this.searchText || undefined
    ).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: (variables) => {
        this.variables = variables;
        this.loading = false;
        // variables.length 会创建表格；先完成本轮检测，再写数据源，避免 smart-table
        // 在宿主尚未创建时丢失首批行。
        this.cdRef.detectChanges();
        assignTableRows(this.source, variables).then(() => this.cdRef.detectChanges());
      },
      error: (error) => {
        console.error('[Variables] Error loading variables:', error);
        this.loadError = error.error?.message || '变量加载超时或服务不可用';
        this.toastrService.danger(this.loadError, this.i18n.instant('加载失败'));
        this.variables = [];
        assignTableRows(this.source, []).then(() => {
          this.loading = false;
          this.cdRef.detectChanges();
        });
      },
    });
  }

  editVariable(variable: Variable): void {
    this.dialogService
      .open(VariableEditDialogComponent, {
        context: {
          name: variable.name,
          value: variable.value,
        },
      })
      .onClose.subscribe((newValue: string | undefined) => {
        if (newValue === undefined || newValue === variable.value) {
          return;
        }
        this.loading = true;
        this.nodeService.updateVariable(variable.name, {
          value: newValue,
          scope: this.variableType.toUpperCase(),
        }).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
          next: () => {
            this.toastrService.success(`变量“${variable.name}”已更新`, '更新成功');
            this.loadVariables();
          },
          error: (error) => {
            this.toastrService.danger(
              error.error?.message || '变量更新超时或失败',
              '更新失败'
            );
            this.loading = false;
            this.cdRef.detectChanges();
          },
        });
      });
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
