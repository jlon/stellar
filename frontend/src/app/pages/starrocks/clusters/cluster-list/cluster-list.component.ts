import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, inject } from '@angular/core';
import { Router } from '@angular/router';
import { timeout } from 'rxjs/operators';
import { NbDialogService, NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { ClusterService, Cluster } from '../../../../@core/data/cluster.service';
import { ErrorHandler } from '../../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { assignTableRows } from '../../../../@core/utils/table-rows';
import { ClusterFormComponent } from '../cluster-form/cluster-form.component';


@Component({
    selector: 'ngx-cluster-list',
    templateUrl: './cluster-list.component.html',
    styleUrls: ['./cluster-list.component.scss'],
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
export class ClusterListComponent implements OnInit {
  private clusterService = inject(ClusterService)
  private i18n = inject(I18nService);
  private router = inject(Router);
  private dialogService = inject(NbDialogService);
  private toastrService = inject(NbToastrService);
  private confirmDialogService = inject(ConfirmDialogService);

  source: LocalDataSource = new LocalDataSource();
  loading = true;

  settings = {
    mode: 'external',
    hideSubHeader: false,  // Enable search
    noDataMessage: this.i18n.instant('暂无集群数据，点击上方按钮添加集群'),
    actions: {
      columnTitle: this.i18n.instant('操作'),
      add: false,
      edit: true,
      delete: true,
      position: 'right',
    },
    edit: {
      editButtonContent: '<i class="nb-edit" title="编辑"></i>',
    },
    delete: {
      deleteButtonContent: '<i class="nb-trash" title="删除"></i>',
      confirmDelete: false,
    },
    pager: {
      display: true,
      perPage: 10,
    },
    columns: {
      id: {
        title: 'ID',
        type: 'number',
        width: '5%',
      },
      name: {
        title: this.i18n.instant('集群名称'),
        type: 'string',
      },
      fe_host: {
        title: this.i18n.instant('FE 地址'),
        type: 'string',
      },
      fe_http_port: {
        title: this.i18n.instant('HTTP 端口'),
        type: 'number',
        width: '10%',
      },
      fe_query_port: {
        title: this.i18n.instant('查询端口'),
        type: 'number',
        width: '10%',
      },
      username: {
        title: this.i18n.instant('用户名'),
        type: 'string',
        width: '10%',
      },
      description: {
        title: this.i18n.instant('描述'),
        type: 'string',
      },
      created_at: {
        title: this.i18n.instant('创建时间'),
        type: 'string',
        valuePrepareFunction: (date: string) => {
          return new Date(date).toLocaleString('zh-CN');
        },
      },
    },
  };

  ngOnInit(): void {
    this.loadClusters();
  }

  loadClusters(): void {
    this.loading = true;
    this.clusterService.listClusters().subscribe({
      next: (clusters) => {
        assignTableRows(this.source, clusters).then(() => {
          this.loading = false;
        });
      },
      error: (error) => {
        this.toastrService.danger(
          ErrorHandler.extractErrorMessage(error),
          '错误',
        );
        assignTableRows(this.source, []).then(() => {
          this.loading = false;
        });
      },
    });
  }

  onCreate(): void {
    this.dialogService
      .open(ClusterFormComponent, { context: { clusterId: null }, dialogClass: 'side-sheet' })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadClusters();
        }
      });
  }

  onEdit(event: any): void {
    this.dialogService
      .open(ClusterFormComponent, { context: { clusterId: event.data.id }, dialogClass: 'side-sheet' })
      .onClose.subscribe((saved) => {
        if (saved) {
          this.loadClusters();
        }
      });
  }

  onDelete(event: any): void {
    const cluster = event.data as Cluster;

    this.confirmDialogService.confirmDelete(cluster.name)
      .subscribe(confirmed => {
        if (!confirmed) {
          return;
        }

        this.clusterService.deleteCluster(cluster.id).subscribe({
          next: () => {
            this.toastrService.success(this.i18n.instant('集群删除成功'), this.i18n.instant('成功'));
            this.loadClusters();
          },
          error: (error) => {
            this.toastrService.danger(
              ErrorHandler.extractErrorMessage(error),
              '错误',
            );
          },
        });
      });
  }

  onRowSelect(event: any): void {
    this.router.navigate(['/pages/starrocks/clusters', event.data.id]);
  }

  testConnection(cluster: Cluster): void {
    this.clusterService.getHealth(cluster.id).pipe(timeout(20000)).subscribe({
      next: (health) => {
        if (health.status === 'healthy') {
          const details = health.checks.map(c => `${c.name}: ${c.message}`).join('\n');
          this.toastrService.success(details, '健康检查通过');
        } else if (health.status === 'warning') {
          const details = health.checks.map(c => `${c.name}: ${c.message}`).join('\n');
          this.toastrService.warning(details, '健康检查警告');
        } else {
          const details = health.checks.map(c => `${c.name}: ${c.message}`).join('\n');
          this.toastrService.danger(details, '健康检查失败');
        }
      },
      error: (error) => {
        this.toastrService.danger(
          ErrorHandler.extractErrorMessage(error),
          '错误',
        );
      },
    });
  }
}

