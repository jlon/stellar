import { QueryExecutionComponent } from '../query-execution/query-execution.component';

describe('QueryExecutionComponent database statistics', () => {
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
});
