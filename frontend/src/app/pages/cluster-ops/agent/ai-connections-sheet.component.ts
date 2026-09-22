import { CommonModule } from '@angular/common';
import { ChangeDetectorRef, Component, ElementRef, HostListener, OnDestroy, OnInit, inject } from '@angular/core';
import { FormsModule, ReactiveFormsModule, FormBuilder, Validators } from '@angular/forms';
import {
  NbAccordionModule,
  NbAlertModule,
  NbButtonModule,
  NbCardModule,
  NbDialogRef,
  NbFormFieldModule,
  NbIconModule,
  NbInputModule,
  NbOptionModule,
  NbSelectModule,
  NbSpinnerModule,
  NbTooltipModule,
  NbToastrService,
} from '@nebular/theme';
import { Subject } from 'rxjs';
import { take, takeUntil, timeout } from 'rxjs/operators';
import { TranslatePipe } from '@ngx-translate/core';

import {
  CreateLLMProviderRequest,
  LLMProvider,
  LLMProviderService,
  UpdateLLMProviderRequest,
} from '../../../@core/data/llm-provider.service';
import { AuthService } from '../../../@core/data/auth.service';
import { PermissionService } from '../../../@core/data/permission.service';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';
import { I18nService } from '../../../@core/i18n/i18n.service';

type EditorMode = 'create' | 'edit' | null;

const PROVIDER_PRESETS = [
  { name: 'openai', display_name: 'OpenAI', api_base: 'https://api.openai.com/v1', model_name: 'gpt-4o' },
  { name: 'deepseek', display_name: 'DeepSeek', api_base: 'https://api.deepseek.com/v1', model_name: 'deepseek-chat' },
  { name: 'qwen', display_name: '通义千问', api_base: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model_name: 'qwen-plus' },
  { name: 'openrouter', display_name: 'OpenRouter', api_base: 'https://openrouter.ai/api/v1', model_name: 'openai/gpt-4o' },
];

/** Global AI connection configuration, opened from the assistant session rail. */
@Component({
  selector: 'ngx-ai-connections-sheet',
  standalone: true,
  templateUrl: './ai-connections-sheet.component.html',
  styleUrls: ['./ai-connections-sheet.component.scss'],
  imports: [
    CommonModule,
    FormsModule,
    ReactiveFormsModule,
    TranslatePipe,
    NbAccordionModule,
    NbAlertModule,
    NbButtonModule,
    NbCardModule,
    NbFormFieldModule,
    NbIconModule,
    NbInputModule,
    NbOptionModule,
    NbSelectModule,
    NbSpinnerModule,
    NbTooltipModule,
  ],
})
export class AiConnectionsSheetComponent implements OnInit, OnDestroy {
  private static readonly sheetExitDurationMs = 180;
  private readonly dialogRef = inject<NbDialogRef<AiConnectionsSheetComponent>>(NbDialogRef);
  private readonly elementRef = inject(ElementRef<HTMLElement>);
  private readonly formBuilder = inject(FormBuilder);
  private readonly llmProviders = inject(LLMProviderService);
  private readonly permissionService = inject(PermissionService);
  private readonly authService = inject(AuthService);
  private readonly confirmDialog = inject(ConfirmDialogService);
  private readonly toastr = inject(NbToastrService);
  private readonly i18n = inject(I18nService);
  private readonly changeDetectorRef = inject(ChangeDetectorRef);
  private readonly destroy$ = new Subject<void>();

  readonly presets = PROVIDER_PRESETS;
  readonly form = this.formBuilder.group({
    name: ['', [Validators.required, Validators.maxLength(50), Validators.pattern(/^[a-z0-9_-]+$/)]],
    display_name: ['', [Validators.required, Validators.maxLength(100)]],
    api_base: ['', [Validators.required, Validators.pattern(/^https?:\/\/.+/)]],
    model_name: ['', [Validators.required, Validators.maxLength(100)]],
    api_key: [''],
    max_tokens: [4096, [Validators.required, Validators.min(1), Validators.max(128000)]],
    temperature: [0.3, [Validators.required, Validators.min(0), Validators.max(2)]],
    timeout_seconds: [60, [Validators.required, Validators.min(5), Validators.max(600)]],
  });

  providers: LLMProvider[] = [];
  loading = false;
  saving = false;
  testingId: number | null = null;
  editorMode: EditorMode = null;
  editingProvider: LLMProvider | null = null;
  showApiKey = false;
  canList = false;
  canCreate = false;
  canUpdate = false;
  canDelete = false;
  canActivate = false;
  canTest = false;
  changed = false;
  private closing = false;

  ngOnInit(): void {
    this.permissionService.permissions$
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.applyPermissions());
    this.applyPermissions();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  close(): void {
    if (this.closing) {
      return;
    }

    const sheet = this.elementRef.nativeElement.closest('.cdk-overlay-pane.side-sheet') as HTMLElement | null;
    if (!sheet || this.prefersReducedMotion()) {
      this.dialogRef.close(this.changed);
      return;
    }

    this.closing = true;
    sheet.classList.add('side-sheet--closing');
    (this.elementRef.nativeElement.ownerDocument
      .querySelector('.cdk-overlay-backdrop.side-sheet-backdrop') as HTMLElement | null)
      ?.classList.add('side-sheet-backdrop--closing');
    this.elementRef.nativeElement.ownerDocument.defaultView?.setTimeout(
      () => this.dialogRef.close(this.changed),
      AiConnectionsSheetComponent.sheetExitDurationMs,
    );
  }

  @HostListener('document:keyup.escape')
  closeOnEscape(): void {
    this.close();
  }

  loadProviders(): void {
    if (!this.canList) {
      return;
    }
    this.loading = true;
    this.llmProviders.listProviders().pipe(takeUntil(this.destroy$), timeout(20_000)).subscribe({
      next: (providers) => {
        this.providers = providers;
        this.loading = false;
        this.changeDetectorRef.detectChanges();
      },
      error: (error) => {
        this.loading = false;
        ErrorHandler.handleHttpError(error, this.toastr);
        this.changeDetectorRef.detectChanges();
      },
    });
  }

  beginCreate(): void {
    if (!this.canCreate) {
      return;
    }
    this.editorMode = 'create';
    this.editingProvider = null;
    this.showApiKey = false;
    this.form.enable();
    this.form.reset({ max_tokens: 4096, temperature: 0.3, timeout_seconds: 60 });
    this.form.get('api_key')?.setValidators([Validators.required]);
    this.form.get('api_key')?.updateValueAndValidity();
  }

  beginEdit(provider: LLMProvider): void {
    if (!this.canUpdate) {
      return;
    }
    this.editorMode = 'edit';
    this.editingProvider = provider;
    this.showApiKey = false;
    this.form.enable();
    this.form.patchValue({
      name: provider.name,
      display_name: provider.display_name,
      api_base: provider.api_base,
      model_name: provider.model_name,
      api_key: '',
      max_tokens: provider.max_tokens,
      temperature: provider.temperature,
      timeout_seconds: provider.timeout_seconds,
    });
    this.form.get('name')?.disable();
    this.form.get('api_key')?.clearValidators();
    this.form.get('api_key')?.updateValueAndValidity();
  }

  cancelEditor(): void {
    this.editorMode = null;
    this.editingProvider = null;
    this.showApiKey = false;
    this.form.reset({ max_tokens: 4096, temperature: 0.3, timeout_seconds: 60 });
  }

  applyPreset(preset: (typeof PROVIDER_PRESETS)[number] | null): void {
    if (!preset || this.editorMode !== 'create') {
      return;
    }
    this.form.patchValue(preset);
  }

  save(): void {
    if (!this.editorMode || this.form.invalid || this.saving) {
      this.form.markAllAsTouched();
      return;
    }

    this.saving = true;
    const value = this.form.getRawValue();
    const request$ = this.editorMode === 'create'
      ? this.llmProviders.createProvider({
          name: value.name!,
          display_name: value.display_name!,
          api_base: value.api_base!,
          model_name: value.model_name!,
          api_key: value.api_key!,
          max_tokens: value.max_tokens!,
          temperature: value.temperature!,
          timeout_seconds: value.timeout_seconds!,
        } satisfies CreateLLMProviderRequest)
      : this.llmProviders.updateProvider(this.editingProvider!.id, this.updateRequest(value));

    request$.pipe(takeUntil(this.destroy$), timeout(20_000)).subscribe({
      next: () => {
        this.changed = true;
        this.saving = false;
        this.toastr.success(
          this.i18n.instant(this.editorMode === 'create' ? 'AI 模型供应商已添加' : 'AI 模型供应商已更新'),
          this.i18n.instant('成功'),
        );
        this.cancelEditor();
        this.loadProviders();
      },
      error: (error) => {
        this.saving = false;
        ErrorHandler.handleHttpError(error, this.toastr);
      },
    });
  }

  activate(provider: LLMProvider): void {
    if (!this.canActivate || provider.is_active) {
      return;
    }
    this.confirmDialog
      .confirm(
        '切换 AI 模型供应商',
        `切换为“${provider.display_name}”后，智能助手和 Profile 根因分析都会立即使用该模型。`,
        '设为当前',
      )
      .pipe(take(1), takeUntil(this.destroy$))
      .subscribe((confirmed) => {
        if (!confirmed) {
          return;
        }
        this.llmProviders.activateProvider(provider.id).pipe(takeUntil(this.destroy$), timeout(20_000)).subscribe({
          next: () => this.afterMutation('已切换当前 AI 模型供应商'),
          error: (error) => ErrorHandler.handleHttpError(error, this.toastr),
        });
      });
  }

  toggleEnabled(provider: LLMProvider): void {
    if (!this.canUpdate) {
      return;
    }
    this.llmProviders.updateProvider(provider.id, { enabled: !provider.enabled })
      .pipe(takeUntil(this.destroy$), timeout(20_000))
      .subscribe({
        next: () => this.afterMutation(provider.enabled ? 'AI 模型供应商已停用' : 'AI 模型供应商已启用'),
        error: (error) => ErrorHandler.handleHttpError(error, this.toastr),
      });
  }

  test(provider: LLMProvider): void {
    if (!this.canTest) {
      return;
    }
    this.testingId = provider.id;
    this.llmProviders.testConnection(provider.id).pipe(takeUntil(this.destroy$), timeout(20_000)).subscribe({
      next: (result) => {
        this.testingId = null;
        if (result.success) {
          this.toastr.success(`连接成功${result.latency_ms === undefined ? '' : `，延迟 ${result.latency_ms}ms`}`, '测试通过');
        } else {
          this.toastr.warning(result.message, '测试失败');
        }
      },
      error: (error) => {
        this.testingId = null;
        ErrorHandler.handleHttpError(error, this.toastr);
      },
    });
  }

  remove(provider: LLMProvider): void {
    if (!this.canDelete || provider.is_active) {
      return;
    }
    this.confirmDialog.confirmDelete(provider.display_name, '历史分析记录会保留，但不再关联此连接。')
      .pipe(take(1), takeUntil(this.destroy$))
      .subscribe((confirmed) => {
        if (!confirmed) {
          return;
        }
        this.llmProviders.deleteProvider(provider.id).pipe(takeUntil(this.destroy$), timeout(20_000)).subscribe({
          next: () => this.afterMutation('AI 模型供应商已删除'),
          error: (error) => ErrorHandler.handleHttpError(error, this.toastr),
        });
      });
  }

  trackProvider(_: number, provider: LLMProvider): number {
    return provider.id;
  }

  private applyPermissions(): void {
    const superAdmin = this.authService.isSuperAdmin();
    this.canList = superAdmin || this.permissionService.hasPermission('api:llm:providers:list');
    this.canCreate = superAdmin || this.permissionService.hasPermission('api:llm:providers:create');
    this.canUpdate = superAdmin || this.permissionService.hasPermission('api:llm:providers:update');
    this.canDelete = superAdmin || this.permissionService.hasPermission('api:llm:providers:delete');
    this.canActivate = superAdmin || this.permissionService.hasPermission('api:llm:providers:activate');
    this.canTest = superAdmin || this.permissionService.hasPermission('api:llm:providers:test');
    if (this.canList && !this.loading && this.providers.length === 0) {
      this.loadProviders();
    }
  }

  private updateRequest(value: ReturnType<typeof this.form.getRawValue>): UpdateLLMProviderRequest {
    const request: UpdateLLMProviderRequest = {
      display_name: value.display_name!,
      api_base: value.api_base!,
      model_name: value.model_name!,
      max_tokens: value.max_tokens!,
      temperature: value.temperature!,
      timeout_seconds: value.timeout_seconds!,
    };
    if (value.api_key) {
      request.api_key = value.api_key;
    }
    return request;
  }

  private afterMutation(message: string): void {
    this.changed = true;
    this.toastr.success(this.i18n.instant(message), this.i18n.instant('成功'));
    this.loadProviders();
  }

  private prefersReducedMotion(): boolean {
    return this.elementRef.nativeElement.ownerDocument.defaultView
      ?.matchMedia('(prefers-reduced-motion: reduce)').matches ?? false;
  }
}
