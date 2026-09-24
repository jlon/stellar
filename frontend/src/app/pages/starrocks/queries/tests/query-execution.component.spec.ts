import { QueryExecutionComponent } from '../query-execution/query-execution.component';
import { Query } from '../../../../@core/data/node.service';

describe('QueryExecutionComponent database statistics', () => {
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
});
