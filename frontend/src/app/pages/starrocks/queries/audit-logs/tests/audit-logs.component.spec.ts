import { ChangeDetectorRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router } from '@angular/router';
import { NbDialogService, NbToastrService } from '@nebular/theme';

import { ClusterContextService } from '../../../../../@core/data/cluster-context.service';
import { NodeService } from '../../../../../@core/data/node.service';
import { I18nService } from '../../../../../@core/i18n/i18n.service';
import { AuditLogsComponent } from '../audit-logs.component';

describe('AuditLogsComponent', () => {
  let component: AuditLogsComponent;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: NodeService, useValue: {} },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
        { provide: ChangeDetectorRef, useValue: {} },
        { provide: ActivatedRoute, useValue: { snapshot: { paramMap: { get: () => null } } } },
        { provide: Router, useValue: {} },
        { provide: NbToastrService, useValue: {} },
        { provide: ClusterContextService, useValue: {} },
        { provide: NbDialogService, useValue: {} },
      ],
    });
    component = TestBed.runInInjectionContext(() => new AuditLogsComponent());
  });

  it('keeps the query contract when converting a picker value', () => {
    expect(component.dateTimeValue(new Date(2026, 8, 21, 9, 8))).toBe('2026-09-21T09:08');
    expect(component.dateTimeValue(null)).toBe('');
  });
});
