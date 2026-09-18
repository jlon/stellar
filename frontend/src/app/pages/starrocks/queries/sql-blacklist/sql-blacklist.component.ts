import { I18nService } from '../../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, TemplateRef, ViewChild, ChangeDetectorRef, inject } from '@angular/core';
import { NbToastrService, NbDialogService, NbDialogRef, NbCardModule, NbButtonModule, NbIconModule, NbSpinnerModule, NbAccordionModule, NbBadgeModule, NbInputModule, NbTooltipModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';
import { NodeService, SqlBlacklistItem } from '../../../../@core/data/node.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Cluster } from '../../../../@core/data/cluster.service';
import { ErrorHandler } from '../../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { assignTableRows } from '../../../../@core/utils/table-rows';

import { FormsModule } from '@angular/forms';

@Component({
    selector: 'ngx-sql-blacklist',
    templateUrl: './sql-blacklist.component.html',
    styleUrls: ['./sql-blacklist.component.scss'],
    imports: [
    TranslatePipe,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbSpinnerModule,
    NbTooltipModule,
    Angular2SmartTableModule,
    NbAccordionModule,
    NbBadgeModule,
    NbInputModule,
    FormsModule
],
})
export class SqlBlacklistComponent implements OnInit, OnDestroy {
  private nodeService = inject(NodeService)
  private i18n = inject(I18nService);
  private toastrService = inject(NbToastrService);
  private clusterContext = inject(ClusterContextService);
  private dialogService = inject(NbDialogService);
  private confirmDialogService = inject(ConfirmDialogService);
  private cdr = inject(ChangeDetectorRef);

  blacklistSource: LocalDataSource = new LocalDataSource();
  activeCluster: Cluster | null = null;
  loading = true;
  private destroy$ = new Subject<void>();

  @ViewChild('blacklistDialog', { static: false }) blacklistDialogTemplate!: TemplateRef<any>;
  blacklistDialogRef: NbDialogRef<any> | null = null;
  newBlacklistPattern = '';

  exampleTemplates = [
    {
      title: this.i18n.instant('禁止 SELECT *'),
      description: '防止全表扫描，提升查询性能',
      pattern: 'select\\\\s+\\\\*\\\\s+from',
      icon: 'eye-off-outline'
    },
    {
      title: this.i18n.instant('禁止 COUNT(*)'),
      description: '避免大表计数操作',
      pattern: 'select\\\\s+count\\\\(\\\\*\\\\)\\\\s+from',
      icon: 'hash-outline'
    },
    {
      title: this.i18n.instant('禁止 DROP 操作'),
      description: '防止误删表、库、视图',
      pattern: '.*DROP\\\\s+(TABLE|DATABASE|VIEW).*',
      icon: 'trash-2-outline'
    },
    {
      title: this.i18n.instant('禁止无条件 DELETE'),
      description: '防止误删全表数据',
      pattern: 'DELETE\\\\s+FROM\\\\s+\\\\w+\\\\s*$',
      icon: 'alert-triangle-outline'
    },
    {
      title: this.i18n.instant('禁止敏感表查询'),
      description: '保护特定敏感数据表',
      pattern: '.*FROM\\\\s+sensitive_table.*',
      icon: 'shield-outline'
    },
    {
      title: this.i18n.instant('禁止大批量 INSERT'),
      description: '限制单次插入数据量',
      pattern: 'INSERT\\\\s+INTO.*VALUES\\\\s*\\\\(.*\\\\)\\\\s*,.*\\\\)\\\\s*,.*\\\\)',
      icon: 'download-outline'
    }
  ];

  blacklistSettings = {
    mode: 'external',
    hideSubHeader: true,
    noDataMessage: '暂无 SQL 黑名单规则',
    actions: { add: false, edit: false, delete: true, position: 'right', columnTitle: this.i18n.instant('操作') },
    delete: { deleteButtonContent: '<i class="nb-trash" title="删除"></i>', confirmDelete: true },
    columns: {
      Id: { title: 'ID', type: 'string', width: '10%' },
      Pattern: { title: this.i18n.instant('正则表达式'), type: 'string', width: '90%' },
    },
  };

  ngOnInit(): void {
    this.clusterContext.activeCluster$.pipe(takeUntil(this.destroy$)).subscribe(cluster => {
      this.activeCluster = cluster;
      if (cluster) {
        this.loadBlacklist();
      }
    });
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadBlacklist(): void {
    this.loading = true;
    this.nodeService.listSqlBlacklist().pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: items => {
        console.log('SQL Blacklist loaded:', items);
        console.log('Loading into table source...');
        assignTableRows(this.blacklistSource, items).then(() => {
          this.loading = false;
        });
      },
      error: error => {
        console.error('SQL Blacklist load error:', error);
        this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '加载失败');
        assignTableRows(this.blacklistSource, []).then(() => {
          this.loading = false;
        });
      },
    });
  }

  openAddDialog(): void {
    this.newBlacklistPattern = '';
    this.blacklistDialogRef = this.dialogService.open(this.blacklistDialogTemplate, { closeOnBackdropClick: false, closeOnEsc: true });
  }

  submitPattern(): void {
    const pattern = this.newBlacklistPattern.trim();
    if (!pattern) { this.toastrService.warning('请输入正则表达式', '提示'); return; }
    this.nodeService.addSqlBlacklist(pattern).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success(this.i18n.instant('SQL 黑名单规则添加成功'), this.i18n.instant('成功'));
        this.blacklistDialogRef?.close();
        this.loadBlacklist();
      },
      error: error => { this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '添加失败'); },
    });
  }

  cancelDialog(): void { this.blacklistDialogRef?.close(); }

  useTemplate(template: any): void {
    this.newBlacklistPattern = template.pattern;
  }

  trackTemplate(index: number, template: any): string {
    return template.title;
  }

  onDeleteConfirm(event: any): void {
    const item = event.data;
    this.confirmDialogService.confirm('确认删除', `确定要删除黑名单规则 #${item.Id} 吗？`, '删除', '取消', 'danger').subscribe(confirmed => {
      if (!confirmed) { event.confirm.reject(); return; }
      this.nodeService.deleteSqlBlacklist(item.Id).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
        next: () => { this.toastrService.success(this.i18n.instant('SQL 黑名单规则删除成功'), this.i18n.instant('成功')); this.loadBlacklist(); event.confirm.resolve(); },
        error: error => { this.toastrService.danger(ErrorHandler.extractErrorMessage(error), '删除失败'); event.confirm.reject(); },
      });
    });
  }
}
