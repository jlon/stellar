import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbButtonModule, NbCardModule, NbIconModule, NbOptionModule, NbSelectModule, NbSpinnerModule, NbTooltipModule } from '@nebular/theme';
import { FormsModule } from '@angular/forms';
import { OpAuditService } from '../../../@core/data/op-audit.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { NbToastrService } from '@nebular/theme';
import { HasPermissionDirective } from '../../../@core/directives/has-permission.directive';

const ACTION_LABEL: Record<string, string> = {
  create: '创建',
  update: '更新',
  delete: '删除',
  download: '下载',
};

const TARGET_LABEL: Record<string, string> = {
  cluster: '集群',
  user: '用户',
  role: '角色',
  organization: '组织',
  system_log: '日志包',
};

@Component({
  selector: 'ngx-op-audit',
  templateUrl: './op-audit.component.html',
  styleUrls: ['./op-audit.component.scss'],
  imports: [
    TranslatePipe,
    NbButtonModule,
    NbCardModule,
    NbIconModule,
    NbSelectModule,
    NbOptionModule,
    NbSpinnerModule,
    NbTooltipModule,
    FormsModule,
    Angular2SmartTableModule,
    HasPermissionDirective,
  ],
})
export class OpAuditComponent implements OnInit, OnDestroy {
  private opAuditService = inject(OpAuditService)
  private i18n = inject(I18nService);
  private toastrService = inject(NbToastrService);

  source: LocalDataSource = new LocalDataSource();
  loading = false;
  filterTarget = '';
  filterAction = '';
  downloading = false;

  settings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: this.i18n.instant('暂无操作记录'),
    actions: { add: false, edit: false, delete: false },
    pager: { display: true, perPage: 15 },
    columns: {
      id: { title: 'ID', type: 'number', width: '6%' },
      created_at: { title: this.i18n.instant('时间'), type: 'string', width: '16%' },
      username: { title: this.i18n.instant('操作人'), type: 'string', width: '10%' },
      action: {
        title: this.i18n.instant('动作'),
        type: 'string',
        width: '8%',
        valuePrepareFunction: (v: string) => ACTION_LABEL[v] ?? v,
      },
      target_type: {
        title: this.i18n.instant('对象'),
        type: 'string',
        width: '8%',
        valuePrepareFunction: (v: string) => TARGET_LABEL[v] ?? v,
      },
      target_name: { title: this.i18n.instant('对象名'), type: 'string', width: '18%' },
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

  downloadLogArchive(): void {
    this.downloading = true;
    this.opAuditService.downloadLogArchive().subscribe({
      next: (response) => {
        const url = URL.createObjectURL(response.body!);
        const link = document.createElement('a');
        link.href = url;
        link.download = this.getArchiveFilename(response.headers.get('content-disposition'));
        link.click();
        // Revoke on the next tick: Safari cancels the download when the object
        // URL disappears in the same task as the click.
        setTimeout(() => URL.revokeObjectURL(url));
        this.toastrService.success(this.i18n.instant('日志包已生成'), this.i18n.instant('下载成功'));
        this.downloading = false;
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.downloading = false;
      },
    });
  }

  private getArchiveFilename(contentDisposition: string | null): string {
    return contentDisposition?.match(/filename="?([^";]+)"?/)?.[1] ?? 'stellar-logs.zip';
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
