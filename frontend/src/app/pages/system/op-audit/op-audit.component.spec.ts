import { ComponentFixture, TestBed } from '@angular/core/testing';
import { of } from 'rxjs';
import { provideTranslateService, TranslateLoader, TranslationObject } from '@ngx-translate/core';
import { AuthService } from '../../../@core/data/auth.service';
import { OpAuditService } from '../../../@core/data/op-audit.service';
import { PermissionService } from '../../../@core/data/permission.service';
import { NbThemeModule, NbToastrService } from '@nebular/theme';
import { NbEvaIconsModule } from '@nebular/eva-icons';
import { OpAuditComponent } from './op-audit.component';

describe('OpAuditComponent', () => {
  let fixture: ComponentFixture<OpAuditComponent>;
  const opAuditServiceStub = {
    list: jasmine.createSpy('list').and.returnValue(of({ items: [] })),
    downloadLogArchive: jasmine.createSpy('downloadLogArchive'),
  };
  const authServiceStub = {
    isSuperAdmin: () => true,
    currentUser: of(null),
  };
  const permissionServiceStub = {
    hasPermission: () => true,
    permissions$: of([]),
  };
  const toastrStub = { success: () => {}, danger: () => {} };

  beforeEach(() => {
    TestBed.configureTestingModule({
      imports: [NbThemeModule.forRoot(), NbEvaIconsModule, OpAuditComponent],
      providers: [
        // translate pipe 需要 TranslateService；用内存词表避免发 HTTP 请求
        provideTranslateService({ loader: { provide: TranslateLoader, useValue: {
          getTranslation: (lang: string) => of({} as TranslationObject),
        } } }),
        { provide: OpAuditService, useValue: opAuditServiceStub },
        { provide: AuthService, useValue: authServiceStub },
        { provide: PermissionService, useValue: permissionServiceStub },
        { provide: NbToastrService, useValue: toastrStub },
      ],
    });
    fixture = TestBed.createComponent(OpAuditComponent);
    fixture.detectChanges();
  });

  it('renders the log archive action as an icon-only ghost button', () => {
    const button = fixture.nativeElement.querySelector('.card-toolbar button') as HTMLButtonElement;

    expect(button).toBeTruthy();
    expect(button.textContent?.trim()).toBe('');
    expect(button.getAttribute('title')).toBe('打包日志');
    expect(button.getAttribute('aria-label')).toBe('打包日志');
    expect(button.classList).toContain('appearance-ghost');
    expect(button.classList).toContain('size-small');
    expect(button.classList).toContain('status-basic');
    expect(button.querySelector('nb-icon svg')?.classList).toContain('eva-download-outline');
  });

  it('keeps both audit filters in the compact filter row', () => {
    const selects = fixture.nativeElement.querySelectorAll('.filter-row nb-select');

    expect(selects.length).toBe(2);
  });

  it('loads the audit list on init', () => {
    expect(opAuditServiceStub.list).toHaveBeenCalled();
  });
});
