import { ChangeDetectorRef, DOCUMENT, TemplateRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { RowSelectionEvent } from 'angular2-smart-table';
import { BehaviorSubject, Subject, of } from 'rxjs';

import { AuthService } from '../../../../@core/data/auth.service';
import { ClusterService } from '../../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { MaterializedView, MaterializedViewService } from '../../../../@core/data/materialized-view.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { ActiveToggleRenderComponent } from '../active-toggle-render.component';
import { MaterializedViewsComponent } from '../materialized-views.component';

describe('MaterializedViewsComponent', () => {
  let component: MaterializedViewsComponent;
  const dialogService = { open: jasmine.createSpy('open') };
  const toastrService = {
    danger: jasmine.createSpy('danger'),
    success: jasmine.createSpy('success'),
  };
  const materializedViewService = {
    getMaterializedViewDDL: jasmine.createSpy('getMaterializedViewDDL').and.returnValue(of({ ddl: 'CREATE MATERIALIZED VIEW sales_mv' })),
    getDependencies: jasmine.createSpy('getDependencies').and.returnValue(of({
      object: { database: 'analytics', name: 'sales_mv', kind: 'async' },
      dependencies: [],
      complete: true,
      warnings: [],
    })),
    createMaterializedView: jasmine.createSpy('createMaterializedView').and.returnValue(of({})),
  };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: MaterializedViewService, useValue: materializedViewService },
        { provide: ClusterService, useValue: {} },
        { provide: ClusterContextService, useValue: { activeCluster$: new BehaviorSubject(null), getActiveClusterId: () => null } },
        { provide: AuthService, useValue: { isAuthenticated: () => true } },
        { provide: I18nService, useValue: { instant: (key: string) => key } },
        { provide: NbToastrService, useValue: toastrService },
        { provide: ConfirmDialogService, useValue: {} },
        { provide: NbDialogService, useValue: dialogService },
        { provide: ChangeDetectorRef, useValue: { detectChanges: () => undefined } },
        { provide: DOCUMENT, useValue: document },
      ],
    });
    component = TestBed.runInInjectionContext(() => new MaterializedViewsComponent());
    dialogService.open.calls.reset();
    materializedViewService.getMaterializedViewDDL.calls.reset();
    materializedViewService.getDependencies.calls.reset();
    materializedViewService.createMaterializedView.calls.reset();
    toastrService.success.calls.reset();
  });

  it('uses a single selected table row as the details entry point', () => {
    const view = testView();
    const openDetail = spyOn(component, 'viewDetail');

    component.onRowSelect({ data: view, row: { index: 4 } } as unknown as RowSelectionEvent);

    expect(component.settings.selectMode).toBe('single');
    expect(component.settings.actions.edit).toBeFalse();
    expect(openDetail).toHaveBeenCalledWith(view, 4);
  });

  it('opens selected views with the load-management side-sheet configuration', () => {
    const onBackdropClick = new Subject<void>();
    const onClose = new Subject<void>();
    dialogService.open.and.returnValue({ close: jasmine.createSpy('close'), onBackdropClick, onClose });
    const template = {} as TemplateRef<unknown>;
    (component as unknown as { detailDialogTemplate: TemplateRef<unknown> }).detailDialogTemplate = template;
    const view = testView();

    component.viewDetail(view);

    expect(dialogService.open).toHaveBeenCalledWith(template, jasmine.objectContaining({
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: false,
      dialogClass: 'side-sheet',
    }));
    const reference = { database: 'analytics', name: 'sales_mv', kind: 'async' };
    expect(materializedViewService.getMaterializedViewDDL).toHaveBeenCalledWith(reference);
    expect(materializedViewService.getDependencies).toHaveBeenCalledWith(reference);
    expect(component.mvDDL).toBe('CREATE MATERIALIZED VIEW sales_mv');
  });

  it('does not open row details when the inline state control is clicked', () => {
    const toggle = new ActiveToggleRenderComponent();
    const event = { stopPropagation: jasmine.createSpy('stopPropagation') } as unknown as MouseEvent;
    spyOn(toggle.toggleActive, 'emit');
    toggle.rowData = { name: 'sales_mv' };

    toggle.onToggle(event);

    expect(event.stopPropagation).toHaveBeenCalled();
    expect(toggle.toggleActive.emit).toHaveBeenCalledWith(toggle.rowData);
  });

  it('identifies synchronous materialized views by their explicit kind', () => {
    const toggle = new ActiveToggleRenderComponent();
    toggle.value = true;
    toggle.rowData = { kind: 'rollup', refresh_type: 'SYNC' };

    toggle.ngOnInit();

    expect(toggle.isRollup).toBeTrue();
    expect(toggle.isActive).toBeTrue();
  });

  it('normalizes a picker value without changing its local minute', () => {
    const pickerValue = new Date(2026, 8, 21, 9, 8);

    expect((component as any).normalizeTime(pickerValue)).toBe('2026-09-21T09:08');
  });

  it('reports a successful create as a submitted task', () => {
    component.createSQL = 'CREATE MATERIALIZED VIEW sales_mv REFRESH MANUAL AS SELECT 1';
    spyOn(component, 'closeCreateDialog');
    spyOn(component, 'loadMaterializedViews');

    component.createMV();

    expect(materializedViewService.createMaterializedView).toHaveBeenCalledOnceWith({
      sql: component.createSQL,
    });
    expect(toastrService.success).toHaveBeenCalledOnceWith(
      '物化视图创建任务已提交',
      '成功',
    );
    expect(component.closeCreateDialog).toHaveBeenCalled();
    expect(component.loadMaterializedViews).toHaveBeenCalled();
  });
});

function testView(): MaterializedView {
  return {
    id: 'async:analytics:sales_mv',
    name: 'sales_mv',
    database_name: 'analytics',
    kind: 'async',
    definition: 'SELECT 1',
    refresh_type: 'MANUAL',
    is_active: true,
  };
}
