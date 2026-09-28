import { QueryExecutionComponent } from '../query-execution/query-execution.component';
import { Query, SchemaObjectDependencies } from '../../../../@core/data/node.service';
import { NEVER, Subject, of } from 'rxjs';

describe('QueryExecutionComponent database statistics', () => {
  it('does not let detail dialogs close on the same Escape as nested confirmations', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const dialogService = { open: jasmine.createSpy('open').and.returnValue({ close: jasmine.createSpy('close') }) };
    const internal = component as unknown as {
      currentQueryDetail: Query | null;
      queryDetailDialogRef: unknown;
      queryDetailDialogTemplate: unknown;
      dialogService: typeof dialogService;
      onQueryEdit: (event: { data: Query }) => void;
    };
    internal.queryDetailDialogTemplate = {};
    internal.queryDetailDialogRef = null;
    internal.dialogService = dialogService;

    internal.onQueryEdit({ data: { QueryId: 'query-1' } as Query });

    expect(dialogService.open).toHaveBeenCalledWith(
      internal.queryDetailDialogTemplate,
      jasmine.objectContaining({ closeOnEsc: false }),
    );
  });

  it('keeps query details open until the nested kill confirmation succeeds', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const confirm = jasmine.createSpy('confirm').and.returnValue(of(false));
    const detailRef = { close: jasmine.createSpy('close') };
    const internal = component as unknown as {
      currentQueryDetail: Query;
      queryDetailDialogRef: typeof detailRef;
      confirmDialogService: { confirm: typeof confirm };
      destroy$: Subject<void>;
      killQueryFromDetail: () => void;
    };
    internal.currentQueryDetail = { QueryId: 'query-1' } as Query;
    internal.queryDetailDialogRef = detailRef;
    internal.confirmDialogService = { confirm };
    internal.destroy$ = new Subject<void>();

    internal.killQueryFromDetail();

    expect(confirm).toHaveBeenCalledWith(
      '确认查杀查询',
      '确定要查杀查询 query-1 吗？',
      '查杀',
      '取消',
      'danger',
      { nested: true },
    );
    expect(detailRef.close).not.toHaveBeenCalled();
  });

  it('opens compaction confirmation above its trigger dialog', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const confirm = jasmine.createSpy('confirm').and.returnValue(of(false));
    const internal = component as unknown as {
      compactionTriggerTable: string;
      compactionTriggerDatabase: string;
      compactionTriggerCatalog: string;
      compactionTriggerMode: 'table';
      compactionSelectedPartitions: string[];
      confirmDialogService: { confirm: typeof confirm };
      destroy$: Subject<void>;
      buildQualifiedTableName: (catalog: string, database: string, table: string) => string;
      triggerCompaction: () => void;
    };
    internal.compactionTriggerTable = 'orders';
    internal.compactionTriggerDatabase = 'analytics';
    internal.compactionTriggerCatalog = 'default_catalog';
    internal.compactionTriggerMode = 'table';
    internal.compactionSelectedPartitions = [];
    internal.confirmDialogService = { confirm };
    internal.destroy$ = new Subject<void>();
    internal.buildQualifiedTableName = () => 'default_catalog.analytics.orders';

    internal.triggerCompaction();

    expect(confirm).toHaveBeenCalledWith(
      '确认触发Compaction',
      '确定要对整个表 "orders" 执行Compaction吗？\n\n该操作需要目标表的 ALTER 权限；SQL 成功仅表示提交成功，请继续查看 Compaction 信息。',
      '确认触发',
      '取消',
      'primary',
      { nested: true },
    );
  });

  it('parses StarRocks current-query durations into milliseconds', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;

    expect(component.parseExecTime('3.153 s')).toBeCloseTo(3153, 6);
    expect(component.parseExecTime('3804')).toBe(3804);
    expect(component.parseExecTime('2.5m')).toBe(150_000);
  });

  it('filters running queries by displayed duration and scan measurements', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const internal = component as unknown as {
      runningQueryFilter: { longRunningOnly?: boolean; largeScanOnly?: boolean };
      longRunningQueryThresholdMs: number;
      largeScanQueryThresholdBytes: number;
      filterRunningQueries: (queries: Query[]) => Query[];
    };
    internal.runningQueryFilter = { longRunningOnly: true, largeScanOnly: true };
    internal.longRunningQueryThresholdMs = 300_000;
    internal.largeScanQueryThresholdBytes = 1024 ** 3;

    const queries = [
      { QueryId: 'short', ExecTime: '299 s', ScanBytes: '1.2 GB' },
      { QueryId: 'small', ExecTime: '300 s', ScanBytes: '1023 MB' },
      { QueryId: 'match', ExecTime: '300 s', ScanBytes: '1 GB' },
    ] as Query[];

    expect(internal.filterRunningQueries(queries).map((query) => query.QueryId)).toEqual([
      'match',
    ]);
  });

  it('aggregates numeric partition sizes without string unit parsing', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const executeSQL = jasmine.createSpy('executeSQL').and.returnValue({});
    const internal = component as unknown as {
      extractNodeInfo: jasmine.Spy;
      validateNodeInfo: jasmine.Spy;
      openInfoDialog: jasmine.Spy;
      nodeService: { executeSQL: typeof executeSQL };
      i18n: { instant: (key: string) => string };
      viewDatabaseStats: (node: unknown) => void;
    };

    internal.extractNodeInfo = jasmine.createSpy().and.returnValue({
      catalogName: 'default_catalog',
      databaseName: 'analytics',
    });
    internal.validateNodeInfo = jasmine.createSpy().and.returnValue(true);
    internal.nodeService = { executeSQL };
    internal.i18n = { instant: key => key };
    internal.openInfoDialog = jasmine.createSpy().and.callFake(
      (_title: string, _type: string, load: () => unknown) => load(),
    );

    internal.viewDatabaseStats({});

    const [sql] = executeSQL.calls.mostRecent().args;
    expect(sql).toContain('SUM(COALESCE(DATA_SIZE, 0)) / 1024 / 1024');
    expect(sql).not.toContain('DATA_SIZE LIKE');
    expect(sql).not.toContain('REPLACE(DATA_SIZE');
  });

  it('uses the same numeric aggregation for table storage statistics', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const executeSQL = jasmine.createSpy('executeSQL').and.returnValue({
      pipe: () => ({ subscribe: () => undefined }),
    });
    const internal = component as unknown as {
      tableStatsLoadingState: Record<string, boolean>;
      infoDialogPageLoading: boolean;
      infoDialogError: string | null;
      tableStatsDatabaseName: string;
      tableStatsTableName: string;
      tableStatsCatalogName: string;
      nodeService: { executeSQL: typeof executeSQL };
      destroy$: unknown;
      loadTableStatsTabData: (tab: 'storage') => void;
    };

    internal.tableStatsLoadingState = { partition: false, compaction: false, storage: false };
    internal.tableStatsDatabaseName = 'analytics';
    internal.tableStatsTableName = 'sales';
    internal.tableStatsCatalogName = 'default_catalog';
    internal.nodeService = { executeSQL };
    internal.destroy$ = {};

    internal.loadTableStatsTabData('storage');

    const [sql] = executeSQL.calls.mostRecent().args;
    expect(sql).toContain('SUM(COALESCE(DATA_SIZE, 0)) / 1024 / 1024');
    expect(sql).not.toContain('DATA_SIZE LIKE');
    expect(sql).not.toContain('REPLACE(DATA_SIZE');
  });

  it('reloads expired Schema Explorer object references instead of reusing the tree cache', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const getSchemaObjects = jasmine.createSpy('getSchemaObjects').and.returnValue(NEVER);
    const internal = component as unknown as {
      tableCache: Record<string, unknown[]>;
      tableCacheExpiresAt: Record<string, number>;
      schemaObjectReferenceCacheMs: number;
      clusterId: number;
      extractNodeInfo: jasmine.Spy;
      getDatabaseCacheKey: jasmine.Spy;
      nodeService: { getSchemaObjects: typeof getSchemaObjects };
      loadTablesForDatabase: (node: { children: unknown[]; loading?: boolean }) => void;
    };

    internal.tableCache = { 'default_catalog|analytics': [] };
    internal.tableCacheExpiresAt = { 'default_catalog|analytics': Date.now() - 1 };
    internal.schemaObjectReferenceCacheMs = 8 * 60 * 1000;
    internal.clusterId = 7;
    internal.extractNodeInfo = jasmine.createSpy().and.returnValue({
      catalogName: 'default_catalog',
      databaseName: 'analytics',
    });
    internal.getDatabaseCacheKey = jasmine.createSpy().and.returnValue('default_catalog|analytics');
    internal.nodeService = { getSchemaObjects };

    internal.loadTablesForDatabase({ children: [] });

    expect(getSchemaObjects).toHaveBeenCalledWith(7, 'default_catalog', 'analytics');
  });

  it('does not extend a cached Schema Explorer object reference lifetime', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const expiresAt = Date.now() + 60_000;
    const internal = component as unknown as {
      tableCache: Record<string, Array<{ name: string; object_type: string }>>;
      tableCacheExpiresAt: Record<string, number>;
      extractNodeInfo: jasmine.Spy;
      getDatabaseCacheKey: jasmine.Spy;
      refreshSqlSchema: jasmine.Spy;
      loadTablesForDatabase: (node: { children: unknown[]; data: { originalName: string }; loading?: boolean }) => void;
    };

    internal.tableCache = { 'default_catalog|analytics': [{ name: 'orders', object_type: 'TABLE' }] };
    internal.tableCacheExpiresAt = { 'default_catalog|analytics': expiresAt };
    internal.extractNodeInfo = jasmine.createSpy().and.returnValue({
      catalogName: 'default_catalog',
      databaseName: 'analytics',
    });
    internal.getDatabaseCacheKey = jasmine.createSpy().and.returnValue('default_catalog|analytics');
    internal.refreshSqlSchema = jasmine.createSpy();

    internal.loadTablesForDatabase({ children: [], data: { originalName: 'analytics' } });

    expect(internal.tableCacheExpiresAt['default_catalog|analytics']).toBe(expiresAt);
  });

  it('uses collision-free cache keys for catalog and database identifiers', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const internal = component as unknown as {
      getDatabaseCacheKey: (catalog: string, database: string) => string;
    };

    expect(internal.getDatabaseCacheKey('catalog|sales', 'orders')).not.toBe(
      internal.getDatabaseCacheKey('catalog', 'sales|orders'),
    );
  });

  it('keeps partial Schema Explorer relationships out of the local graph until requested', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const internal = component as unknown as {
      showPartialSchemaDependencies: boolean;
      schemaDependencies: SchemaObjectDependencies | null;
      schemaDependencyGraphNodes: Array<{ id: string }>;
      schemaDependencyGraphEdges: Array<{ label: string }>;
      schemaDependencyGraphWidth: number;
      schemaDependencyGraphHeight: number;
      buildSchemaDependencyGraph: (dependencies: SchemaObjectDependencies) => void;
      hasVisibleSchemaDependencies: () => boolean;
    };
    const dependencies: SchemaObjectDependencies = {
      object: {
        cluster_id: 7,
        catalog: 'default_catalog',
        database: 'analytics',
        name: 'daily_summary',
        object_kind: 'materialized_view',
      },
      dependencies: [
        {
          direction: 'upstream',
          relation_kind: 'mv_reads',
          object: {
            cluster_id: 7,
            catalog: 'default_catalog',
            database: 'analytics',
            name: 'orders',
            object_kind: 'table',
          },
          object_ref: 'orders-ref',
          evidence: 'verified',
          source: 'star_rocks_object_dependencies',
          observed_at: '2026-09-26T00:00:00Z',
        },
        {
          direction: 'downstream',
          relation_kind: 'mv_reads',
          object: {
            cluster_id: 7,
            catalog: 'default_catalog',
            database: 'analytics',
            name: 'monthly_summary',
            object_kind: 'materialized_view',
          },
          object_ref: 'monthly-ref',
          evidence: 'partial',
          source: 'doris_definition',
          observed_at: '2026-09-26T00:00:00Z',
        },
      ],
      complete: false,
      warnings: [],
      read_at: '2026-09-26T00:00:00Z',
    };

    internal.showPartialSchemaDependencies = false;
    internal.schemaDependencies = dependencies;
    internal.buildSchemaDependencyGraph(dependencies);

    expect(internal.schemaDependencyGraphNodes.length).toBe(2);
    expect(internal.schemaDependencyGraphEdges.map((edge) => edge.label)).toEqual(['mv reads']);
    expect(internal.hasVisibleSchemaDependencies()).toBeTrue();

    internal.showPartialSchemaDependencies = true;
    internal.buildSchemaDependencyGraph(dependencies);

    expect(internal.schemaDependencyGraphNodes.length).toBe(3);
    expect(internal.schemaDependencyGraphEdges.length).toBe(2);
    expect(internal.hasVisibleSchemaDependencies()).toBeTrue();
  });

  it('reports an empty graph when the current filter hides every partial relationship', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const internal = component as unknown as {
      showPartialSchemaDependencies: boolean;
      schemaDependencies: SchemaObjectDependencies | null;
      hasVisibleSchemaDependencies: () => boolean;
    };
    internal.showPartialSchemaDependencies = false;
    internal.schemaDependencies = {
      object: {
        cluster_id: 7,
        catalog: 'default_catalog',
        database: 'analytics',
        name: 'daily_summary',
        object_kind: 'materialized_view',
      },
      dependencies: [{
        direction: 'upstream',
        relation_kind: 'mv_reads',
        object: {
          cluster_id: 7,
          catalog: 'default_catalog',
          database: 'analytics',
          name: 'orders',
          object_kind: 'table',
        },
        object_ref: 'orders-ref',
        evidence: 'partial',
        source: 'doris_definition',
        observed_at: '2026-09-26T00:00:00Z',
      }],
      complete: false,
      warnings: [],
      read_at: '2026-09-26T00:00:00Z',
    };

    expect(internal.hasVisibleSchemaDependencies()).toBeFalse();

    internal.showPartialSchemaDependencies = true;

    expect(internal.hasVisibleSchemaDependencies()).toBeTrue();
  });

  it('keeps a newly opened Schema Explorer reference when closing the previous dialog', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const firstClose = new Subject<void>();
    const secondClose = new Subject<void>();
    const firstDialog = { close: () => firstClose.next(), onClose: firstClose };
    const secondDialog = { close: () => secondClose.next(), onClose: secondClose };
    const internal = component as unknown as {
      clusterId: number;
      schemaDialogRef: unknown;
      currentSchemaObjectRef: string | null;
      destroySchemaEditor: jasmine.Spy;
      extractNodeInfo: jasmine.Spy;
      validateNodeInfo: jasmine.Spy;
      nodeService: { getSchemaObject: jasmine.Spy };
      dialogService: { open: jasmine.Spy };
      destroy$: Subject<void>;
      viewTableSchema: (node: unknown) => void;
    };

    internal.clusterId = 7;
    internal.schemaDialogRef = null;
    internal.destroySchemaEditor = jasmine.createSpy();
    internal.extractNodeInfo = jasmine.createSpy().and.callFake((node: { data: { table: string } }) => ({
      catalogName: 'default_catalog',
      databaseName: 'analytics',
      tableName: node.data.table,
    }));
    internal.validateNodeInfo = jasmine.createSpy().and.returnValue(true);
    internal.nodeService = { getSchemaObject: jasmine.createSpy().and.returnValue(NEVER) };
    internal.dialogService = { open: jasmine.createSpy().and.returnValues(firstDialog, secondDialog) };
    internal.destroy$ = new Subject<void>();

    internal.viewTableSchema({ data: { table: 'first', schemaObjectRef: 'first-ref' } });
    internal.viewTableSchema({ data: { table: 'second', schemaObjectRef: 'second-ref' } });

    expect(internal.currentSchemaObjectRef).toBe('second-ref');
  });

  it('renews an expired reference for the same object kind only', () => {
    const component = Object.create(QueryExecutionComponent.prototype) as QueryExecutionComponent;
    const internal = component as unknown as {
      clusterId: number;
      schemaDialogId: number;
      schemaDialogRef: unknown;
      currentSchemaNode: { data: Record<string, string> };
      currentSchemaObjectRef: string | null;
      tableCache: Record<string, unknown[]>;
      tableCacheExpiresAt: Record<string, number>;
      schemaObjectReferenceCacheMs: number;
      destroy$: Subject<void>;
      extractNodeInfo: jasmine.Spy;
      getDatabaseCacheKey: jasmine.Spy;
      nodeService: { getSchemaObjects: jasmine.Spy };
      renewSchemaObjectReference: (dialogId: number, continueWith: (objectRef: string) => void) => void;
    };

    internal.clusterId = 7;
    internal.schemaDialogId = 3;
    internal.schemaDialogRef = {};
    internal.currentSchemaNode = {
      data: {
        table: 'daily_orders',
        tableType: 'TABLE',
        schemaObjectKind: 'table',
        schemaObjectRef: 'expired-ref',
      },
    };
    internal.currentSchemaObjectRef = 'expired-ref';
    internal.tableCache = {};
    internal.tableCacheExpiresAt = {};
    internal.schemaObjectReferenceCacheMs = 8 * 60 * 1000;
    internal.destroy$ = new Subject<void>();
    internal.extractNodeInfo = jasmine.createSpy().and.returnValue({
      catalogName: 'default_catalog',
      databaseName: 'analytics',
      tableName: 'daily_orders',
    });
    internal.getDatabaseCacheKey = jasmine.createSpy().and.returnValue('cache-key');
    internal.nodeService = {
      getSchemaObjects: jasmine.createSpy().and.returnValue(of([
        { name: 'daily_orders', object_kind: 'view', object_ref: 'view-ref' },
        { name: 'daily_orders', object_kind: 'table', object_ref: 'table-ref' },
      ])),
    };
    const continueWith = jasmine.createSpy('continueWith');

    internal.renewSchemaObjectReference(3, continueWith);

    expect(continueWith).toHaveBeenCalledWith('table-ref');
    expect(internal.currentSchemaObjectRef).toBe('table-ref');
  });
});
