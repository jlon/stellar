import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';

import { I18nService } from '../../i18n/i18n.service';
import { TabReuseService } from '../tab-reuse.service';
import { TabService } from '../tab.service';

describe('TabService', () => {
  let activeLanguage: 'zh' | 'en';
  let service: TabService;

  beforeEach(() => {
    localStorage.removeItem('stellar_tabs');
    activeLanguage = 'zh';

    TestBed.configureTestingModule({
      providers: [
        TabService,
        {
          provide: Router,
          useValue: {
            url: '/',
            navigate: jasmine.createSpy('navigate'),
            navigateByUrl: jasmine.createSpy('navigateByUrl'),
          },
        },
        {
          provide: I18nService,
          useValue: {
            instant: (key: string) => activeLanguage === 'en' && key === '导入管理' ? 'Load Management' : key,
          },
        },
        {
          provide: TabReuseService,
          useValue: {
            markForRefresh: jasmine.createSpy('markForRefresh'),
            markManyForRefresh: jasmine.createSpy('markManyForRefresh'),
          },
        },
      ],
    });
  });

  afterEach(() => localStorage.removeItem('stellar_tabs'));

  it('migrates legacy tab titles from their URL and persists the translation key', () => {
    localStorage.setItem('stellar_tabs', JSON.stringify([
      {
        id: 'tab_loads',
        title: '导入管理',
        titleKey: 'legacy-load-title',
        url: '/pages/starrocks/loads',
        active: true,
        closable: true,
        pinned: false,
      },
    ]));
    service = TestBed.inject(TabService);

    activeLanguage = 'en';
    service.relocalizeTabs(url => url === '/pages/starrocks/loads' ? '导入管理' : null);

    const loadTab = service.getTabs().find(tab => tab.id === 'tab_loads');
    const savedTabs = JSON.parse(localStorage.getItem('stellar_tabs') || '[]');

    expect(loadTab).toEqual(jasmine.objectContaining({
      title: 'Load Management',
      titleKey: '导入管理',
    }));
    expect(savedTabs.find((tab: { id: string }) => tab.id === 'tab_loads')).toEqual(jasmine.objectContaining({
      titleKey: '导入管理',
    }));
  });
});
