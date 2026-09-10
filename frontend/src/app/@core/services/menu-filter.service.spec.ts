import { NbMenuItem } from '@nebular/theme';

import { PermissionService } from '../data/permission.service';
import { MenuFilterService } from './menu-filter.service';

describe('MenuFilterService', () => {
  it('uses the deployment menu permission for deployment links', () => {
    const permissions = jasmine.createSpyObj<Pick<PermissionService, 'hasMenuPermission'>>(
      'PermissionService',
      ['hasMenuPermission'],
    );
    permissions.hasMenuPermission.and.callFake((code) => code === 'deployment');
    const service = new MenuFilterService(permissions as unknown as PermissionService);
    const items: NbMenuItem[] = [
      { title: '主机管理', link: '/pages/starrocks/deployment/hosts' },
      { title: '集群列表', link: '/pages/starrocks/dashboard' },
    ];

    expect(service.filterMenuItems(items).map((item) => item.title)).toEqual(['主机管理']);
    expect(permissions.hasMenuPermission.calls.allArgs()).toEqual([['deployment'], ['dashboard']]);
  });
});
