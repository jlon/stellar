import { TestBed } from '@angular/core/testing';
import { throwError } from 'rxjs';

import { ApiService } from './api.service';
import { Permission, PermissionService } from './permission.service';

describe('PermissionService', () => {
  const cachedPermissions: Permission[] = [
    {
      id: 1,
      code: 'menu:dashboard',
      name: '集群列表',
      type: 'menu',
      action: 'view',
    },
  ];
  const api = {
    get: jasmine.createSpy('get'),
  };

  beforeEach(() => {
    localStorage.setItem('user_permissions', JSON.stringify(cachedPermissions));
    api.get.and.returnValue(throwError(() => new Error('offline')));
    spyOn(console, 'error');

    TestBed.configureTestingModule({
      providers: [
        PermissionService,
        { provide: ApiService, useValue: api },
      ],
    });
  });

  afterEach(() => {
    localStorage.removeItem('user_permissions');
    api.get.calls.reset();
  });

  it('keeps cached permissions when the refresh request fails', (done) => {
    const service = TestBed.inject(PermissionService);

    service.initPermissions().subscribe((permissions) => {
      expect(permissions).toEqual(cachedPermissions);
      expect(service.getCurrentPermissions()).toEqual(cachedPermissions);
      expect(service.hasPermission('menu:dashboard')).toBeTrue();
      done();
    });
  });

  it('clears cached permissions when the server denies access', (done) => {
    api.get.and.returnValue(throwError(() => ({ status: 403 })));
    const service = TestBed.inject(PermissionService);

    service.initPermissions().subscribe((permissions) => {
      expect(permissions).toEqual([]);
      expect(service.getCurrentPermissions()).toEqual([]);
      expect(localStorage.getItem('user_permissions')).toBeNull();
      done();
    });
  });
});
