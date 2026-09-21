import { ComponentFixture, TestBed } from '@angular/core/testing';
import { BehaviorSubject, of } from 'rxjs';
import { Cluster, ClusterService } from '../../../../@core/data/cluster.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { NodeService } from '../../../../@core/data/node.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { NbToastrService } from '@nebular/theme';
import { BackendsComponent } from '../backends.component';

describe('BackendsComponent', () => {
  let fixture: ComponentFixture<BackendsComponent>;
  let activeCluster$: BehaviorSubject<Cluster | null>;
  let listBackends: jasmine.Spy;

  beforeEach(async () => {
    activeCluster$ = new BehaviorSubject<Cluster | null>(null);
    listBackends = jasmine.createSpy().and.returnValue(of([{ BackendId: '2', IP: 'cn-0' }]));

    await TestBed.configureTestingModule({
      imports: [BackendsComponent],
      providers: [
        { provide: NodeService, useValue: { listBackends } },
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
        { provide: ConfirmDialogService, useValue: {} },
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
});
