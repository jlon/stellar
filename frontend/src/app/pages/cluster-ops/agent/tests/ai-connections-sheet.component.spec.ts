import { ChangeDetectorRef, ElementRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { NbDialogRef, NbToastrService } from '@nebular/theme';
import { BehaviorSubject, of } from 'rxjs';

import { AuthService } from '../../../../@core/data/auth.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { LLMProvider, LLMProviderService } from '../../../../@core/data/llm-provider.service';
import { PermissionService } from '../../../../@core/data/permission.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { AiConnectionsSheetComponent } from '../ai-connections-sheet.component';

const provider: LLMProvider = {
  id: 1,
  name: 'deepseek',
  display_name: 'DeepSeek',
  api_base: 'https://api.deepseek.com/v1',
  model_name: 'deepseek-chat',
  has_api_key: true,
  is_active: false,
  enabled: true,
  max_tokens: 4096,
  temperature: 0.3,
  timeout_seconds: 60,
  priority: 100,
  created_at: '2026-09-22T00:00:00Z',
};

describe('AiConnectionsSheetComponent', () => {
  let component: AiConnectionsSheetComponent;
  let granted: Set<string>;
  let permissions$: BehaviorSubject<unknown[]>;
  const llmProviders = {
    listProviders: jasmine.createSpy('listProviders').and.returnValue(of([provider])),
    testConnection: jasmine.createSpy('testConnection').and.returnValue(of({ success: true, message: 'ok' })),
  };
  const changeDetectorRef = { detectChanges: jasmine.createSpy('detectChanges') };
  const dialogRef = { close: jasmine.createSpy('close') };

  beforeEach(() => {
    granted = new Set<string>();
    permissions$ = new BehaviorSubject<unknown[]>([]);
    llmProviders.listProviders.calls.reset();
    llmProviders.testConnection.calls.reset();
    changeDetectorRef.detectChanges.calls.reset();
    dialogRef.close.calls.reset();
    TestBed.configureTestingModule({
      providers: [
        { provide: NbDialogRef, useValue: dialogRef },
        { provide: ElementRef, useValue: new ElementRef(document.createElement('div')) },
        { provide: ChangeDetectorRef, useValue: changeDetectorRef },
        { provide: LLMProviderService, useValue: llmProviders },
        {
          provide: PermissionService,
          useValue: { permissions$, hasPermission: (code: string) => granted.has(code) },
        },
        { provide: AuthService, useValue: { isSuperAdmin: () => false } },
        { provide: ConfirmDialogService, useValue: { confirm: () => of(false), confirmDelete: () => of(false) } },
        { provide: NbToastrService, useValue: { success: jasmine.createSpy('success') } },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
      ],
    });
    component = TestBed.runInInjectionContext(() => new AiConnectionsSheetComponent());
  });

  afterEach(() => component.ngOnDestroy());

  it('does not request connection metadata without list permission', () => {
    component.ngOnInit();

    expect(component.canList).toBeFalse();
    expect(llmProviders.listProviders).not.toHaveBeenCalled();
  });

  it('refreshes the sheet after provider metadata arrives', () => {
    granted.add('api:llm:providers:list');

    component.ngOnInit();

    expect(component.loading).toBeFalse();
    expect(component.providers).toEqual([provider]);
    expect(changeDetectorRef.detectChanges).toHaveBeenCalled();
  });

  it('closes immediately when no sheet element is available for animation', () => {
    component.close();

    expect(dialogRef.close).toHaveBeenCalledWith(false);
  });

  it('requires an API key when creating a connection', () => {
    granted.add('api:llm:providers:list');
    granted.add('api:llm:providers:create');
    component.ngOnInit();
    component.beginCreate();
    component.form.patchValue({
      name: 'custom',
      display_name: 'Custom',
      api_base: 'https://api.example.com/v1',
      model_name: 'custom-model',
    });

    expect(component.form.invalid).toBeTrue();
    component.form.patchValue({ api_key: 'sk-test' });
    expect(component.form.valid).toBeTrue();
  });

  it('keeps the existing API key when an edit leaves it empty', () => {
    granted.add('api:llm:providers:update');
    component.ngOnInit();
    component.beginEdit(provider);

    const request = (component as any).updateRequest(component.form.getRawValue());

    expect(request.api_key).toBeUndefined();
  });

  it('requires the dedicated test permission before sending a connection test', () => {
    component.ngOnInit();
    component.test(provider);
    expect(llmProviders.testConnection).not.toHaveBeenCalled();

    granted.add('api:llm:providers:test');
    permissions$.next([]);
    component.test(provider);

    expect(llmProviders.testConnection).toHaveBeenCalledWith(provider.id);
  });
});
