import { ChangeDetectorRef, Component, OnDestroy, OnInit, inject } from '@angular/core';
import { NbDialogService, NbToastrService, NbCardModule, NbButtonModule, NbIconModule, NbAlertModule, NbSpinnerModule } from '@nebular/theme';
import { LocalDataSource, Angular2SmartTableModule } from 'angular2-smart-table';
import { Subject } from 'rxjs';
import { takeUntil, timeout } from 'rxjs/operators';

import {
  LLMProvider,
  LLMProviderService,
  CreateLLMProviderRequest,
  UpdateLLMProviderRequest,
} from '../../../@core/data/llm-provider.service';
import { PermissionService } from '../../../@core/data/permission.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { LLMProvidersActionsCellComponent } from './table/actions-cell.component';
import { LLMProviderStatusCellComponent } from './table/status-cell.component';
import {
  LLMProviderFormDialogComponent,
  LLMProviderFormDialogResult,
} from './llm-provider-form/llm-provider-form-dialog.component';
import { AuthService } from '../../../@core/data/auth.service';
import { assignTableRows } from '../../../@core/utils/table-rows';


@Component({
    selector: 'ngx-llm-providers',
    templateUrl: './llm-providers.component.html',
    styleUrls: ['./llm-providers.component.scss'],
    imports: [
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbAlertModule,
    NbSpinnerModule,
    Angular2SmartTableModule
],
})
export class LLMProvidersComponent implements OnInit, OnDestroy {
  private llmService = inject(LLMProviderService);
  private permissionService = inject(PermissionService);
  private dialogService = inject(NbDialogService);
  private confirmDialog = inject(ConfirmDialogService);
  private toastrService = inject(NbToastrService);
  private authService = inject(AuthService);
  private cdRef = inject(ChangeDetectorRef);

  source: LocalDataSource = new LocalDataSource();
  loading = false;
  loadError = '';
  testingId: number | null = null;
  private destroy$ = new Subject<void>();

  isSuperAdmin = false;
  hasListPermission = false;
  canCreate = false;
  canUpdate = false;
  canDelete = false;

  settings = this.buildTableSettings();

  ngOnInit(): void {
    this.permissionService.permissions$
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.applyPermissionState());

    this.applyPermissionState();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadProviders(): void {
    if (!this.hasListPermission) {
      this.loading = false;
      this.source.load([]);
      return;
    }

    this.loading = true;
    this.loadError = '';
    this.llmService.listProviders().pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: (providers) => {
        this.loading = false;
        // 首次权限初始化时表格会在同一轮检测后创建，先完成检测再写入。
        this.cdRef.detectChanges();
        assignTableRows(this.source, providers).then(() => this.cdRef.detectChanges());
      },
      error: (error) => {
        this.loadError = error.error?.message || 'LLM 提供商加载超时或服务不可用';
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
        this.cdRef.detectChanges();
        assignTableRows(this.source, []).then(() => this.cdRef.detectChanges());
      },
    });
  }

  openCreateProvider(): void {
    if (!this.canCreate) return;

    const dialogRef = this.dialogService.open(LLMProviderFormDialogComponent, {
      context: { mode: 'create' },
      closeOnBackdropClick: false,
      autoFocus: false,
    });

    dialogRef.onClose.subscribe((result?: LLMProviderFormDialogResult) => {
      if (result) this.createProvider(result);
    });
  }

  openEditProvider(provider: LLMProvider): void {
    if (!this.canUpdate) return;

    const dialogRef = this.dialogService.open(LLMProviderFormDialogComponent, {
      context: { mode: 'edit', provider },
      closeOnBackdropClick: false,
      autoFocus: false,
    });

    dialogRef.onClose.subscribe((result?: LLMProviderFormDialogResult) => {
      if (result) this.updateProvider(provider.id, result);
    });
  }

  deleteProvider(provider: LLMProvider): void {
    if (!this.canDelete) return;

    this.confirmDialog
      .confirmDelete(provider.display_name, '删除后将无法恢复，历史分析记录将保留但无法关联到此提供商')
      .subscribe((confirmed) => {
        if (confirmed) this.performDelete(provider.id);
      });
  }

  activateProvider(provider: LLMProvider): void {
    if (!this.canUpdate) return;

    this.loading = true;
    this.llmService.activateProvider(provider.id).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success(`已激活 ${provider.display_name}`, '成功');
        this.loadProviders();
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
      },
    });
  }

  toggleEnabled(provider: LLMProvider): void {
    if (!this.canUpdate) return;

    const newEnabled = !provider.enabled;
    this.loading = true;
    this.llmService.updateProvider(provider.id, { enabled: newEnabled }).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success(
          `已${newEnabled ? '启用' : '禁用'} ${provider.display_name}`,
          '成功'
        );
        this.loadProviders();
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
      },
    });
  }

  testConnection(provider: LLMProvider): void {
    this.testingId = provider.id;
    this.llmService.testConnection(provider.id).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: (result) => {
        if (result.success) {
          this.toastrService.success(
            `连接成功，延迟 ${result.latency_ms}ms`,
            '测试通过'
          );
        } else {
          this.toastrService.warning(result.message, '测试失败');
        }
        this.testingId = null;
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.testingId = null;
      },
    });
  }

  private createProvider(result: LLMProviderFormDialogResult): void {
    const payload: CreateLLMProviderRequest = {
      name: result.name,
      display_name: result.display_name,
      api_base: result.api_base,
      model_name: result.model_name,
      api_key: result.api_key!,
      max_tokens: result.max_tokens,
      temperature: result.temperature,
      timeout_seconds: result.timeout_seconds,
      priority: result.priority,
    };

    this.loading = true;
    this.llmService.createProvider(payload).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success('LLM 提供商创建成功', '成功');
        this.loadProviders();
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
      },
    });
  }

  private updateProvider(id: number, result: LLMProviderFormDialogResult): void {
    const payload: UpdateLLMProviderRequest = {
      display_name: result.display_name,
      api_base: result.api_base,
      model_name: result.model_name,
      max_tokens: result.max_tokens,
      temperature: result.temperature,
      timeout_seconds: result.timeout_seconds,
      priority: result.priority,
    };

    // Only include api_key if it was changed
    if (result.api_key) {
      payload.api_key = result.api_key;
    }

    this.loading = true;
    this.llmService.updateProvider(id, payload).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success('LLM 提供商更新成功', '成功');
        this.loadProviders();
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
      },
    });
  }

  private performDelete(id: number): void {
    this.loading = true;
    this.llmService.deleteProvider(id).pipe(takeUntil(this.destroy$), timeout(20000)).subscribe({
      next: () => {
        this.toastrService.success('LLM 提供商删除成功', '成功');
        this.loadProviders();
      },
      error: (error) => {
        ErrorHandler.handleHttpError(error, this.toastrService);
        this.loading = false;
      },
    });
  }

  private applyPermissionState(): void {
    this.isSuperAdmin = this.authService.isSuperAdmin();
    this.hasListPermission =
      this.permissionService.hasPermission('api:llm:providers:list') || this.isSuperAdmin;
    this.canCreate =
      this.permissionService.hasPermission('api:llm:providers:create') || this.isSuperAdmin;
    this.canUpdate =
      this.permissionService.hasPermission('api:llm:providers:update') || this.isSuperAdmin;
    this.canDelete =
      this.permissionService.hasPermission('api:llm:providers:delete') || this.isSuperAdmin;

    this.settings = this.buildTableSettings();

    if (this.hasListPermission) {
      this.loadProviders();
    }
  }

  private buildTableSettings(): any {
    return {
      mode: 'external',
      hideSubHeader: true,
      noDataMessage: '暂无 LLM 提供商',
      actions: false,
      columns: {
        display_name: {
          title: '名称',
          type: 'string',
          width: '14%',
        },
        name: {
          title: '标识',
          type: 'string',
          width: '10%',
        },
        model_name: {
          title: '模型',
          type: 'string',
          width: '14%',
        },
        api_base: {
          title: 'API 地址',
          type: 'string',
          width: '18%',
          valuePrepareFunction: (cell: string) => {
            // Truncate long URLs
            return cell.length > 40 ? cell.substring(0, 40) + '...' : cell;
          },
        },
        status: {
          title: '状态',
          type: 'custom',
          width: '15%',
          isFilterable: false,
          renderComponent: LLMProviderStatusCellComponent,
          componentInitFunction: (instance: LLMProviderStatusCellComponent, cell: any) => {
            instance.rowData = cell.getRow().getData() as LLMProvider;
          },
        },
        priority: {
          title: '优先级',
          type: 'number',
          width: '10%',
        },
        actions: {
          title: '操作',
          type: 'custom',
          width: '17%',
          isFilterable: false,
          isSortable: false,
          renderComponent: LLMProvidersActionsCellComponent,
          componentInitFunction: (instance: LLMProvidersActionsCellComponent, cell: any) => {
            instance.rowData = cell.getRow().getData() as LLMProvider;
            instance.canUpdate = this.canUpdate;
            instance.canDelete = this.canDelete;
            instance.testingId = this.testingId;
            instance.edit.subscribe((provider) => this.openEditProvider(provider));
            instance.delete.subscribe((provider) => this.deleteProvider(provider));
            instance.activate.subscribe((provider) => this.activateProvider(provider));
            instance.toggleState.subscribe((provider) => this.toggleEnabled(provider));
            instance.test.subscribe((provider) => this.testConnection(provider));
          },
        },
      },
    };
  }
}
