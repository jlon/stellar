import { ComponentFixture, TestBed } from '@angular/core/testing';
import { BehaviorSubject, of } from 'rxjs';
import { Cluster, ClusterService } from '../../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Backend, NodeService } from '../../../../@core/data/node.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { BackendActionsCellComponent, BackendsComponent } from '../backends.component';

describe('BackendsComponent', () => {
  let fixture: ComponentFixture<BackendsComponent>;
  let activeCluster$: BehaviorSubject<Cluster | null>;
  let listBackends: jasmine.Spy;
  let getBackendDiagnostics: jasmine.Spy;

  beforeEach(async () => {
    activeCluster$ = new BehaviorSubject<Cluster | null>(null);
    listBackends = jasmine.createSpy().and.returnValue(of([{ BackendId: '2', IP: 'cn-0' }]));
    getBackendDiagnostics = jasmine.createSpy().and.returnValue(of({
      deployment_mode: 'shared_data',
      captured_at: '2026-01-01T00:00:00Z',
      memory: { data: null, error: 'unavailable' },
      data_cache: { data: null, error: 'unavailable' },
      blocking_drivers: null,
      compaction: null,
    }));

    await TestBed.configureTestingModule({
      imports: [BackendsComponent],
      providers: [
        { provide: NodeService, useValue: { listBackends, getBackendDiagnostics } },
        { provide: ClusterService, useValue: {} },
        {
          provide: ClusterContextService,
          useValue: {
            activeCluster$: activeCluster$.asObservable(),
            getActiveClusterId: () => activeCluster$.value?.id ?? null,
          },
        },
        { provide: I18nService, useValue: { instant: (key: string) => key } },
        { provide: NbToastrService, useValue: { danger: jasmine.createSpy() } },
        { provide: NbDialogService, useValue: {} },
      ],
    })
      .overrideComponent(BackendsComponent, { set: { template: '', imports: [] } })
      .compileComponents();

    fixture = TestBed.createComponent(BackendsComponent);
    fixture.detectChanges();
  });

  it('loads nodes when the active cluster first becomes available', async () => {
    activeCluster$.next({
      id: 2,
      name: 'shared-data',
      fe_host: 'fe.example.com',
      fe_http_port: 8030,
      fe_query_port: 9030,
      username: 'admin',
      enable_ssl: false,
      connection_timeout: 10,
      tags: [],
      catalog: 'default_catalog',
      is_active: true,
      deployment_mode: 'shared_data',
      cluster_type: 'starrocks',
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    });

    await fixture.whenStable();

    expect(listBackends).toHaveBeenCalledTimes(1);
    await expectAsync(fixture.componentInstance.source.getAll()).toBeResolvedTo([{ BackendId: '2', IP: 'cn-0' }]);
    expect(fixture.componentInstance.settings.columns.DataUsedCapacity.title).toBe('缓存已用');
    expect(fixture.componentInstance.settings.columns.TotalCapacity.title).toBe('缓存配额');
    expect(fixture.componentInstance.settings.columns.UsedPct.title).toBe('缓存使用率');
  });

  it('uses discovered CN identity for node diagnostics only', () => {
    const component = fixture.componentInstance;
    component.deploymentMode = 'shared_data';
    component.selectedBackend = {
      BackendId: '2',
      IP: 'cn-0',
      HeartbeatPort: '9050',
      HttpPort: '8040',
    } as Backend;
    (component as any).detailClusterId = 2;

    component.loadDiagnostics('compaction');
    expect(getBackendDiagnostics).not.toHaveBeenCalled();

    component.loadDiagnostics('blocking_drivers');
    expect(getBackendDiagnostics).toHaveBeenCalledOnceWith({
      cluster_id: 2,
      backend_id: '2',
      host: 'cn-0',
      heartbeat_port: '9050',
      http_port: '8040',
      include: 'blocking_drivers',
    });
  });

  it('explains memory trackers and blocked-driver states without hiding engine identifiers', () => {
    const component = fixture.componentInstance;

    expect(component.memoryTrackerDescription('query_pool')).toContain('正在执行的查询');
    expect(component.memoryTrackerDescription('jemalloc_metadata')).toContain('内存分配器');
    expect(component.memoryTrackerDescription('future_tracker')).toContain('引擎内部内存分类');
    expect(component.blockingDriverStateLabel('INPUT_EMPTY')).toBe('等待上游数据（INPUT_EMPTY）');
    expect(component.blockingDriverDescription({
      query_id: 'q1', fragment_id: 'f1', driver_id: 1, state: 'OUTPUT_FULL', fragment_status: 'OK',
    })).toContain('下游算子的输出缓冲');
    expect(component.blockingDriverCountText(2, 101, 100)).toBe('已发现 2 条查询中的 101 个等待步骤；当前显示前 100 个');
  });

  it('keeps backend row actions read-only', () => {
    const action = new BackendActionsCellComponent();
    const backend = { BackendId: '2', IP: 'cn-0' } as Backend;
    const diagnose = jasmine.createSpy('diagnose');
    action.backend = backend;
    action.diagnose.subscribe(diagnose);

    action.openDiagnostics(new Event('click'));

    expect(diagnose).toHaveBeenCalledOnceWith(backend);
    expect((action as unknown as { remove?: unknown }).remove).toBeUndefined();
  });
});
