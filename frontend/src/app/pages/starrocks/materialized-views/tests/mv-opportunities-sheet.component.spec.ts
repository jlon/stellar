import { ChangeDetectorRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { NbDialogRef, NbSidebarService } from '@nebular/theme';
import { of, throwError } from 'rxjs';

import { MaterializedViewService } from '../../../../@core/data/materialized-view.service';
import { MvOpportunitiesSheetComponent } from '../mv-opportunities-sheet.component';
import { AgentChatService } from '../../../../@core/data/agent-chat.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';

describe('MvOpportunitiesSheetComponent', () => {
  const materializedViews = {
    getOptimizationOpportunities: jasmine.createSpy('getOptimizationOpportunities'),
  };
  const dialogRef = { close: jasmine.createSpy('close') };
  const changeDetector = { detectChanges: jasmine.createSpy('detectChanges') };
  const chatService = {
    isRunning: jasmine.createSpy('isRunning').and.returnValue(false),
    queueMemoryProfile: jasmine.createSpy('queueMemoryProfile'),
  };
  const clusterContext = { getActiveCluster: jasmine.createSpy('getActiveCluster').and.returnValue({ id: 7 }) };
  const sidebar = { expand: jasmine.createSpy('expand') };

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: MaterializedViewService, useValue: materializedViews },
        { provide: NbDialogRef, useValue: dialogRef },
        { provide: NbSidebarService, useValue: sidebar },
        { provide: ChangeDetectorRef, useValue: changeDetector },
        { provide: AgentChatService, useValue: chatService },
        { provide: ClusterContextService, useValue: clusterContext },
      ],
    });
    materializedViews.getOptimizationOpportunities.calls.reset();
    dialogRef.close.calls.reset();
    changeDetector.detectChanges.calls.reset();
    chatService.isRunning.calls.reset();
    chatService.isRunning.and.returnValue(false);
    chatService.queueMemoryProfile.calls.reset();
    clusterContext.getActiveCluster.calls.reset();
    clusterContext.getActiveCluster.and.returnValue({ id: 7 });
    sidebar.expand.calls.reset();
  });

  it('loads the default bounded audit window', () => {
    materializedViews.getOptimizationOpportunities.and.returnValue(of({
      supported: true,
      engine: 'StarRocks',
      source: 'starrocks_audit_log',
      observed_at: '2026-09-28T00:00:00Z',
      window_hours: 24,
      sampled_query_count: 3,
      truncated: false,
      candidates: [],
      warnings: [],
    }));
    const component = TestBed.runInInjectionContext(() => new MvOpportunitiesSheetComponent());

    component.ngOnInit();

    expect(materializedViews.getOptimizationOpportunities).toHaveBeenCalledOnceWith(24, false);
    expect(component.result?.sampled_query_count).toBe(3);
  });

  it('exposes an audit-read error instead of treating it as an empty result', () => {
    materializedViews.getOptimizationOpportunities.and.returnValue(
      throwError(() => ({ error: { message: '审计日志不可用' } })),
    );
    const component = TestBed.runInInjectionContext(() => new MvOpportunitiesSheetComponent());

    component.load();

    expect(component.result).toBeNull();
    expect(component.error).toContain('审计日志不可用');
  });

  it('hands a redacted candidate to the existing assistant confirmation flow', () => {
    const component = TestBed.runInInjectionContext(() => new MvOpportunitiesSheetComponent());
    component.analyze({
      sql_pattern: 'SELECT day, SUM(amount) FROM orders WHERE country = ? GROUP BY day',
      source_database: 'analytics',
      source_table: 'orders',
      execution_count: 9,
      total_duration_ms: 1200,
      average_duration_ms: 133,
      p95_duration_ms: 300,
      first_seen: '2026-09-28 08:00:00',
      last_seen: '2026-09-28 10:00:00',
    });

    expect(chatService.queueMemoryProfile).toHaveBeenCalledOnceWith(7, jasmine.stringContaining('country = ?'));
    expect(sidebar.expand).toHaveBeenCalledOnceWith('assistant-drawer');
    expect(dialogRef.close).toHaveBeenCalled();
  });

  it('keeps the opportunity sheet open with feedback while an assistant turn is running', () => {
    chatService.isRunning.and.returnValue(true);
    const component = TestBed.runInInjectionContext(() => new MvOpportunitiesSheetComponent());

    component.analyze({
      sql_pattern: 'SELECT day, SUM(amount) FROM orders GROUP BY day',
      source_database: 'analytics',
      source_table: 'orders',
      execution_count: 3,
      total_duration_ms: 600,
      average_duration_ms: 200,
      p95_duration_ms: 300,
      first_seen: '2026-09-28 08:00:00',
      last_seen: '2026-09-28 10:00:00',
    });

    expect(component.error).toContain('正在处理');
    expect(chatService.queueMemoryProfile).not.toHaveBeenCalled();
    expect(dialogRef.close).not.toHaveBeenCalled();
  });
});
