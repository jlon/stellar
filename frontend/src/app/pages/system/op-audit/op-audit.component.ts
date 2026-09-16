import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbCardModule, NbSelectModule, NbOptionModule, NbSpinnerModule } from '@nebular/theme';
import { FormsModule } from '@angular/forms';
import { OpAuditService } from '../../../@core/data/op-audit.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { NbToastrService } from '@nebular/theme';

const ACTION_LABEL: Record<string, string> = {
  create: '创建',
  update: '更新',
  delete: '删除',
};

const TARGET_LABEL: Record<string, string> = {
  cluster: '集群',
  user: '用户',
  role: '角色',
  organization: '组织',
};

@Component({
  selector: 'ngx-op-audit',
  templateUrl: './op-audit.component.html',
  imports: [
    NbCardModule,
    NbSelectModule,
    NbOptionModule,
    NbSpinnerModule,
    FormsModule,
    Angular2SmartTableModule,
  ],
})
export class OpAuditComponent implements OnInit, OnDestroy {
  private opAuditService = inject(OpAuditService);
  private toastrService = inject(NbToastrService);

  source: LocalDataSource = new LocalDataSource();
  loading = false;
  filterTarget = '';
  filterAction = '';

  settings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: '暂无操作记录',
    actions: { add: false, edit: false, delete: false },
    pager: { display: true, perPage: 15 },
    columns: {
      id: { title: 'ID', type: 'number', width: '6%' },
      created_at: { title: '时间', type: 'string', width: '16%' },
      username: { title: '操作人', type: 'string', width: '10%' },
      action: {
        title: '动作',
        type: 'string',
        width: '8%',
        valuePrepareFunction: (v: string) => ACTION_LABEL[v] ?? v,
      },
      target_type: {
        title: '对象',
        type: 'string',
        width: '8%',
        valuePrepareFunction: (v: string) => TARGET_LABEL[v] ?? v,
      },
      target_name: { title: '对象名', type: 'string', width: '18%' },
    },
  };

  private destroy$ = new Subject<void>();

  ngOnInit(): void {
    this.load();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  load(): void {
    this.loading = true;
    this.opAuditService
      .list({
        ...(this.filterTarget ? { target_type: this.filterTarget } : {}),
        ...(this.filterAction ? { action: this.filterAction } : {}),
        limit: 200,
      })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (res) => {
          this.source.load(res.items);
          this.loading = false;
        },
        error: (e) => {
          ErrorHandler.handleHttpError(e, this.toastrService);
          this.loading = false;
        },
      });
  }
}
