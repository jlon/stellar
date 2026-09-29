import { ChangeDetectorRef, DOCUMENT, TemplateRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { RowSelectionEvent } from 'angular2-smart-table';
import { BehaviorSubject, Subject, of, throwError } from 'rxjs';

import { AuthService } from '../../../../@core/data/auth.service';
import { ClusterService } from '../../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { MaterializedView, MaterializedViewService } from '../../../../@core/data/materialized-view.service';
import { NodeService } from '../../../../@core/data/node.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { ActiveToggleRenderComponent } from '../active-toggle-render.component';
import { MaterializedViewsComponent } from '../materialized-views.component';
import { MvOpportunitiesSheetComponent } from '../mv-opportunities-sheet.component';

describe('MaterializedViewsComponent', () => {
  let component: MaterializedViewsComponent;
  const dialogService = { open: jasmine.createSpy('open') };
  const toastrService = {
    danger: jasmine.createSpy('danger'),
    success: jasmine.createSpy('success'),
    warning: jasmine.createSpy('warning'),
  };
  const changeDetector = {
    detectChanges: jasmine.createSpy('detectChanges'),
  };
  const materializedViewService = {
    getMaterializedViewDDL: jasmine.createSpy('getMaterializedViewDDL').and.returnValue(of({ ddl: 'CREATE MATERIALIZED VIEW sales_mv' })),
    getDependencies: jasmine.createSpy('getDependencies').and.returnValue(of({
      object: { database: 'analytics', name: 'sales_mv', kind: 'async' },
      dependencies: [],
      complete: true,
      warnings: [],
      read_at: '2026-09-28T00:00:00Z',
    })),
    createMaterializedView: jasmine.createSpy('createMaterializedView').and.returnValue(of({})),
    previewMaterializedView: jasmine.createSpy('previewMaterializedView').and.returnValue(of({ ddl: 'CREATE MATERIALIZED VIEW analytics.sales_mv AS SELECT order_date, total FROM analytics.sales' })),
    refreshMaterializedView: jasmine.createSpy('refreshMaterializedView').and.returnValue(of({})),
    renameMaterializedView: jasmine.createSpy('renameMaterializedView').and.returnValue(of({})),
    getMaterializedView: jasmine.createSpy('getMaterializedView').and.returnValue(of(testView())),
  };
  const nodeService = {
    getDatabases: jasmine.createSpy('getDatabases').and.returnValue(of(['analytics'])),
    getSchemaObjects: jasmine.createSpy('getSchemaObjects').and.returnValue(of([])),
    getSchemaObject: jasmine.createSpy('getSchemaObject').and.returnValue(of(testSchemaObject())),
  };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: MaterializedViewService, useValue: materializedViewService },
        { provide: NodeService, useValue: nodeService },
        { provide: ClusterService, useValue: {} },
        { provide: ClusterContextService, useValue: { activeCluster$: new BehaviorSubject(null), getActiveClusterId: () => null } },
        { provide: AuthService, useValue: { isAuthenticated: () => true } },
        { provide: I18nService, useValue: { instant: (key: string) => key } },
        { provide: NbToastrService, useValue: toastrService },
        { provide: ConfirmDialogService, useValue: {} },
        { provide: NbDialogService, useValue: dialogService },
        { provide: ChangeDetectorRef, useValue: changeDetector },
        { provide: DOCUMENT, useValue: document },
      ],
    });
    component = TestBed.runInInjectionContext(() => new MaterializedViewsComponent());
    dialogService.open.calls.reset();
    materializedViewService.getMaterializedViewDDL.calls.reset();
    materializedViewService.getDependencies.calls.reset();
    materializedViewService.createMaterializedView.calls.reset();
    materializedViewService.previewMaterializedView.calls.reset();
    materializedViewService.refreshMaterializedView.calls.reset();
    materializedViewService.renameMaterializedView.calls.reset();
    materializedViewService.getMaterializedView.calls.reset();
    nodeService.getSchemaObjects.calls.reset();
    nodeService.getSchemaObjects.and.returnValue(of([]));
    nodeService.getDatabases.calls.reset();
    nodeService.getSchemaObject.calls.reset();
    toastrService.success.calls.reset();
    toastrService.warning.calls.reset();
    changeDetector.detectChanges.calls.reset();
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
    expect(changeDetector.detectChanges).toHaveBeenCalled();
  });

  it('opens creation in the standard side sheet and loads databases', () => {
    const template = {} as TemplateRef<unknown>;
    (component as unknown as { createDialogTemplate: TemplateRef<unknown> }).createDialogTemplate = template;
    dialogService.open.and.returnValue({ close: jasmine.createSpy('close') });

    component.openCreateDialog();

    expect(dialogService.open).toHaveBeenCalledWith(template, jasmine.objectContaining({
      dialogClass: 'side-sheet',
      backdropClass: 'side-sheet-backdrop',
      hasBackdrop: true,
    }));
    expect(nodeService.getDatabases).toHaveBeenCalled();
    expect(component.createDatabases).toEqual(['analytics']);
  });

  it('allows retry when database discovery returns no options', () => {
    nodeService.getDatabases.and.returnValue(of([]));
    component.loadCreateDatabases();
    expect(component.createDatabases).toEqual([]);
    expect(component.createDatabasesLoading).toBeFalse();

    nodeService.getDatabases.and.returnValue(of(['analytics']));
    component.loadCreateDatabases();
    expect(component.createDatabases).toEqual(['analytics']);
  });

  it('opens the read-only opportunity sheet from the MV toolbar', () => {
    component.openOptimizationOpportunities();

    expect(dialogService.open).toHaveBeenCalledWith(MvOpportunitiesSheetComponent, jasmine.objectContaining({
      autoFocus: false,
      backdropClass: 'side-sheet-backdrop',
      closeOnBackdropClick: false,
      closeOnEsc: true,
      dialogClass: 'side-sheet',
    }));
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

  it('reports a successful create as a completed operation', () => {
    component.clusterId = 7;
    component.createDatabase = 'analytics';
    component.createName = 'sales_mv';
    component.createSourceDatabase = 'analytics';
    component.createSourceTable = 'sales';
    component.createColumns = ['order_date', 'total'];
    spyOn(component, 'closeCreateDialog');
    spyOn(component, 'loadMaterializedViews');

    component.previewMV();
    expect(materializedViewService.previewMaterializedView).toHaveBeenCalledOnceWith(jasmine.objectContaining({
      source_database: 'analytics', source_table: 'sales', columns: ['order_date', 'total'],
    }));
    expect(component.createStep).toBe('review');
    component.createMV();

    expect(materializedViewService.createMaterializedView).toHaveBeenCalledOnceWith(jasmine.objectContaining({
      database: 'analytics',
      name: 'sales_mv',
      confirmed_ddl: 'CREATE MATERIALIZED VIEW analytics.sales_mv AS SELECT order_date, total FROM analytics.sales',
      schedule: { kind: 'manual' },
    }));
    expect(toastrService.success).toHaveBeenCalledOnceWith(
      '物化视图已创建；首次刷新需单独发起。',
      '创建成功',
    );
    expect(component.closeCreateDialog).toHaveBeenCalled();
    expect(component.loadMaterializedViews).toHaveBeenCalled();
  });

  it('ignores a preview response after the draft drawer closes', () => {
    const pending = new Subject<{ ddl: string }>();
    materializedViewService.previewMaterializedView.and.returnValue(pending);
    component.createDatabase = 'analytics';
    component.createName = 'daily_sales';
    component.createMode = 'sql';
    component.createQuerySql = 'SELECT id FROM orders';
    component.createDialogRef = { close: jasmine.createSpy('close') };
    component.previewMV();
    component.closeCreateDialog();
    pending.next({ ddl: 'stale ddl' });
    expect(component.createStep).toBe('configure');
    expect(component.createPreviewRequest).toBeNull();
  });

  it('loads only ordinary source tables and their columns for the create form', () => {
    component.clusterId = 7;
    component.activeCluster = { catalog: 'default_catalog' } as any;
    nodeService.getSchemaObjects.and.returnValue(of([
      { name: 'orders', object_kind: 'table', object_ref: 'orders-ref' },
      { name: 'orders_mv', object_kind: 'materialized_view', object_ref: 'orders-mv-ref' },
    ]));
    nodeService.getSchemaObject.and.returnValue(of({
      ...testSchemaObject(),
      columns: [{
        name: 'order_date',
        data_type: 'date',
        nullable: false,
        default_value: null,
        comment: null,
        key: null,
      }],
    }));

    component.onCreateSourceDatabaseChange('analytics');
    component.onCreateSourceTableChange('orders');

    expect(component.createDatabase).toBe('analytics');
    expect(component.createSourceTables).toEqual([
      { name: 'orders', object_kind: 'table', object_ref: 'orders-ref' },
    ]);
    expect(nodeService.getSchemaObject).toHaveBeenCalledOnceWith(7, 'orders-ref');
    expect(component.createAvailableColumns).toEqual([{
      name: 'order_date',
      data_type: 'date',
      nullable: false,
      default_value: null,
      comment: null,
      key: null,
    }]);
    expect(changeDetector.detectChanges).toHaveBeenCalled();
  });

  it('ignores stale table metadata when selections change back', () => {
    component.clusterId = 7;
    const firstTables = new Subject<any[]>();
    nodeService.getSchemaObjects.and.returnValues(firstTables, of([]), of([]));
    component.onCreateSourceDatabaseChange('analytics');
    component.onCreateSourceDatabaseChange('warehouse');
    component.onCreateSourceDatabaseChange('analytics');
    firstTables.next([{ name: 'stale', object_kind: 'table', object_ref: 'old' }]);
    expect(component.createSourceTables).toEqual([]);
    expect(component.createSourceTablesLoading).toBeFalse();
  });

  it('requires a fresh preview before creation and keeps validation visible', () => {
    component.clusterId = 7;
    component.createMV();
    expect(component.createError).toBe('请先预览并确认实际执行的 DDL。');
    component.previewMV();
    expect(component.createError).toBe('请填写目标库与名称，并选择源表和列或输入 SELECT 查询。');
    expect(materializedViewService.createMaterializedView).not.toHaveBeenCalled();
  });

  it('formats SQL for an active creation draft', async () => {
    component.createDialogRef = { close: jasmine.createSpy('close') };
    component.createQuerySql = 'select order_date,sum(amount) from orders group by order_date';
    await component.formatCreateQuery();
    expect(component.createQuerySql).toContain('SELECT');
    expect(component.createQuerySql).toContain('sum(amount)');
  });

  it('previews a complex SELECT but never posts edited, unreviewed SQL', () => {
    component.clusterId = 7;
    component.createDatabase = 'analytics';
    component.createName = 'daily_sales';
    component.createMode = 'sql';
    component.createQuerySql = 'SELECT day, SUM(amount) AS total FROM warehouse.orders GROUP BY day';
    component.createPartitionBy = 'day';
    component.createDistribution = 'hash';
    component.createHashColumns = 'day';
    component.createBuckets = '8';
    component.createSortColumns = 'day';
    component.createReplicationNum = '2';
    component.previewMV();
    expect(materializedViewService.previewMaterializedView).toHaveBeenCalledWith(jasmine.objectContaining({
      cluster_id: 7,
      query_sql: component.createQuerySql,
      partition_by: 'day',
      distribution: { kind: 'hash', columns: ['day'], buckets: 8 },
      sort_columns: ['day'],
      replication_num: 2,
    }));
    component.editCreateDraft();
    component.createQuerySql = 'SELECT * FROM secret';
    component.createMV();
    expect(materializedViewService.createMaterializedView).not.toHaveBeenCalled();
  });

  it('submits a refresh only once while the request is in flight', () => {
    const refreshRequest$ = new Subject<void>();
    materializedViewService.refreshMaterializedView.and.returnValue(refreshRequest$);
    component.selectedMV = testView();

    component.refreshMV();
    component.refreshMV();

    expect(materializedViewService.refreshMaterializedView).toHaveBeenCalledOnceWith(
      { database: 'analytics', name: 'sales_mv', kind: 'async' },
      { mode: 'async', force: false, partition: undefined },
    );
  });

  it('submits an edit only once while the request is in flight', () => {
    const editRequest$ = new Subject<void>();
    materializedViewService.renameMaterializedView.and.returnValue(editRequest$);
    component.selectedMV = testView();
    component.editNewName = 'sales_mv_renamed';

    component.editMV();
    component.editMV();

    expect(materializedViewService.renameMaterializedView).toHaveBeenCalledOnceWith(
      { database: 'analytics', name: 'sales_mv', kind: 'async' },
      'sales_mv_renamed',
    );
  });

  it('opens footer actions without closing the current detail sheet', () => {
    const view = testView();
    component.selectedMV = view;
    const openEdit = spyOn(component, 'openEditDialog');
    const openRefresh = spyOn(component, 'openRefreshDialog');
    const toggleState = spyOn(component, 'toggleActiveState');
    const closeDetail = spyOn(component, 'closeDetailDialog');

    component.editSelectedMV();
    component.refreshSelectedMV();
    component.toggleSelectedMV();

    expect(openEdit).toHaveBeenCalledOnceWith(view);
    expect(openRefresh).toHaveBeenCalledOnceWith(view);
    expect(toggleState).toHaveBeenCalledOnceWith(view, true);
    expect(closeDetail).not.toHaveBeenCalled();
  });

  it('renders direct reads as directed graph edges and resolves opaque references', () => {
    const onBackdropClick = new Subject<void>();
    const onClose = new Subject<void>();
    dialogService.open.and.returnValue({ close: jasmine.createSpy('close'), onBackdropClick, onClose });
    (component as unknown as { detailDialogTemplate: TemplateRef<unknown> }).detailDialogTemplate = {} as TemplateRef<unknown>;
    component.clusterId = 7;
    component.activeCluster = { catalog: 'default_catalog' } as any;
    materializedViewService.getDependencies.and.returnValue(of({
      object: { database: 'analytics', name: 'sales_mv', kind: 'async' },
      dependencies: [{
        object: { catalog: 'default_catalog', database: 'analytics', name: 'orders', kind: 'table' },
        evidence: 'verified',
        source: 'star_rocks_object_dependencies',
        observed_at: '2026-09-28T00:00:00Z',
      }],
      complete: true,
      warnings: [],
      read_at: '2026-09-28T00:00:00Z',
    }));
    nodeService.getSchemaObjects.and.returnValue(of([
      { name: 'orders', object_kind: 'table', object_ref: 'opaque-orders-ref' },
    ]));

    component.viewDetail(testView());

    expect(component.dependencyGraphEdges).toHaveSize(1);
    expect(component.dependencyGraphEdges[0].label).toBe('mv_reads');
    expect(component.dependencyGraphEdges[0].path).toMatch(/^M \d+ \d+ C /);
    expect(component.dependencyGraphNodes.find((node) => node.id === 'dependency-0')?.objectRef)
      .toBe('opaque-orders-ref');
  });

  it('renews a rejected object reference before opening dependency details', () => {
    component.clusterId = 7;
    nodeService.getSchemaObject.and.returnValues(
      throwError(() => ({ status: 404 })),
      of(testSchemaObject()),
    );
    nodeService.getSchemaObjects.and.returnValue(of([
      { name: 'orders', object_kind: 'table', object_ref: 'renewed-orders-ref' },
    ]));

    component.openDependencyNode({
      id: 'dependency-0',
      label: 'analytics.orders',
      subtitle: 'TABLE',
      type: 'table',
      catalog: 'default_catalog',
      database: 'analytics',
      objectName: 'orders',
      objectKind: 'table',
      objectRef: 'expired-orders-ref',
      x: 0,
      y: 0,
      width: 220,
      height: 72,
    });

    expect(nodeService.getSchemaObject.calls.allArgs()).toEqual([
      [7, 'expired-orders-ref'],
      [7, 'renewed-orders-ref'],
    ]);
    expect(component.dependencyObject?.identity.name).toBe('orders');
  });

  it('ignores graph nodes without an object reference', () => {
    component.openDependencyNode({
      id: 'current',
      label: 'analytics.sales_mv',
      subtitle: 'MATERIALIZED VIEW',
      type: 'current',
      x: 0,
      y: 0,
      width: 220,
      height: 72,
    });

    expect(nodeService.getSchemaObject).not.toHaveBeenCalled();
    expect(toastrService.warning).not.toHaveBeenCalled();
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

function testSchemaObject() {
  return {
    identity: {
      cluster_id: 7,
      catalog: 'default_catalog',
      database: 'analytics',
      name: 'orders',
      object_kind: 'table' as const,
    },
    columns: [],
    physical_properties: null,
    ddl_raw: 'CREATE TABLE analytics.orders (order_id BIGINT)',
    parse_status: 'parsed' as const,
    read_at: '2026-09-28T00:00:00Z',
    warnings: [],
  };
}
