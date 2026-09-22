import { TestBed } from '@angular/core/testing';

import { PermissionService } from '../../data/permission.service';
import { MenuFilterService } from '../menu-filter.service';
import { MENU_ITEMS } from '../../../pages/pages-menu';

describe('MenuFilterService', () => {
  let allowedMenuCodes: Set<string>;
  let service: MenuFilterService;

  const clusterOperations = MENU_ITEMS.filter((item) => item.title === '集群运维');

  beforeEach(() => {
    allowedMenuCodes = new Set<string>();
    TestBed.configureTestingModule({
      providers: [
        MenuFilterService,
        {
          provide: PermissionService,
          useValue: {
            hasMenuPermission: (code: string) => allowedMenuCodes.has(code),
          },
        },
      ],
    });
    service = TestBed.inject(MenuFilterService);
  });

  it('keeps an authorized child without adding a parent permission requirement', () => {
    allowedMenuCodes.add('sessions');

    const filtered = service.filterMenuItems(clusterOperations);

    expect(filtered.length).toBe(1);
    expect(filtered[0].children?.map((item) => item.title)).toEqual(['会话管理']);
    expect(clusterOperations[0].children?.length).toBe(5);
  });

  it('hides a grouping item when none of its children are authorized', () => {
    expect(service.filterMenuItems(clusterOperations)).toEqual([]);
  });

  it('places the assistant directly after cluster overview', () => {
    const overviewIndex = MENU_ITEMS.findIndex((item) => item.title === '集群概览');
    const assistant = MENU_ITEMS[overviewIndex + 1];

    expect(assistant).toEqual(jasmine.objectContaining({
      title: '智能助手',
      link: '/pages/cluster-ops/agent',
    }));
    expect((assistant as any).data.permission).toBe('menu:agent');
  });
});
