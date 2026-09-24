import { ChangeDetectorRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router } from '@angular/router';
import { NbDialogService, NbToastrService } from '@nebular/theme';
import { of } from 'rxjs';

import { ClusterContextService } from '../../../../../@core/data/cluster-context.service';
import { NodeService } from '../../../../../@core/data/node.service';
import { I18nService } from '../../../../../@core/i18n/i18n.service';
import { AuditLogsComponent } from '../audit-logs.component';

describe('AuditLogsComponent', () => {
  let component: AuditLogsComponent;
  let nodeService: jasmine.SpyObj<NodeService>;
  let dialogService: jasmine.SpyObj<NbDialogService>;

  beforeEach(() => {
    nodeService = jasmine.createSpyObj<NodeService>('NodeService', ['getProfile']);
    dialogService = jasmine.createSpyObj<NbDialogService>('NbDialogService', ['open']);
    TestBed.configureTestingModule({
      providers: [
        { provide: NodeService, useValue: nodeService },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
        { provide: ChangeDetectorRef, useValue: {} },
        { provide: ActivatedRoute, useValue: { snapshot: { paramMap: { get: () => null } } } },
        { provide: Router, useValue: {} },
        { provide: NbToastrService, useValue: {} },
        { provide: ClusterContextService, useValue: {} },
        { provide: NbDialogService, useValue: dialogService },
      ],
    });
    component = TestBed.runInInjectionContext(() => new AuditLogsComponent());
  });

  it('keeps the query contract when converting a picker value', () => {
    expect(component.dateTimeValue(new Date(2026, 8, 21, 9, 8))).toBe('2026-09-21T09:08');
    expect(component.dateTimeValue(null)).toBe('');
  });

  it('opens a profile through the registered profile route service', () => {
    const query = {
      query_id: 'query-1',
      user: 'root',
      default_db: 'analytics',
      sql_statement: 'SELECT 1',
      query_type: 'Query',
      start_time: '2026-09-24 10:00:00',
      end_time: '2026-09-24 10:00:01',
      total_ms: 1000,
      query_state: 'Finished',
      warehouse: 'default_warehouse',
    };
    nodeService.getProfile.and.returnValue(of({
      query_id: query.query_id,
      profile_content: 'Profile content',
    }));

    component.onEditProfile({ data: query });

    expect(nodeService.getProfile).toHaveBeenCalledOnceWith('query-1');
    expect(component.currentProfile).toEqual(jasmine.objectContaining({
      sql: 'SELECT 1',
      execution_time_ms: 1000,
      profile_content: 'Profile content',
      status: 'Finished',
    }));
    expect(dialogService.open).toHaveBeenCalledOnceWith(undefined);
  });
});
