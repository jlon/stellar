import { TestBed } from '@angular/core/testing';
import { BehaviorSubject, of, throwError } from 'rxjs';

import { AuthService, User } from './auth.service';
import { Cluster, ClusterService } from './cluster.service';
import { ClusterContextService } from './cluster-context.service';
import { PermissionService } from './permission.service';

describe('ClusterContextService', () => {
  const activeCluster: Cluster = {
    id: 1,
    name: 'primary',
    fe_host: 'fe.example.com',
    fe_http_port: 8030,
    fe_query_port: 9030,
    username: 'stellar',
    enable_ssl: false,
    connection_timeout: 10,
    tags: [],
    catalog: 'default_catalog',
    is_active: true,
    deployment_mode: 'shared_nothing',
    cluster_type: 'starrocks',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  };
  const permissions$ = new BehaviorSubject([]);
  const currentUser$ = new BehaviorSubject<User | null>({
    id: 1,
    username: 'admin',
    created_at: '2026-01-01T00:00:00Z',
  });
  const clusterService = {
    getActiveCluster: jasmine.createSpy('getActiveCluster'),
    activateCluster: jasmine.createSpy('activateCluster'),
  };

  beforeEach(() => {
    clusterService.getActiveCluster.and.returnValue(of(activeCluster));

    TestBed.configureTestingModule({
      providers: [
        ClusterContextService,
        { provide: ClusterService, useValue: clusterService },
        {
          provide: PermissionService,
          useValue: {
            permissions$: permissions$.asObservable(),
            hasPermission: () => true,
          },
        },
        {
          provide: AuthService,
          useValue: {
            currentUser: currentUser$.asObservable(),
            isAuthenticated: () => true,
          },
        },
      ],
    });
  });

  afterEach(() => {
    clusterService.getActiveCluster.calls.reset();
  });

  it('keeps the last active cluster when refresh fails at the transport layer', () => {
    const service = TestBed.inject(ClusterContextService);
    expect(service.getActiveCluster()).toEqual(activeCluster);

    clusterService.getActiveCluster.and.returnValue(
      throwError(() => ({ status: 0 })),
    );
    service.refreshActiveCluster();

    expect(service.getActiveCluster()).toEqual(activeCluster);
  });
});
