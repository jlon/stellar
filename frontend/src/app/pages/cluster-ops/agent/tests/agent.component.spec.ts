import { ChangeDetectorRef, ElementRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute } from '@angular/router';
import { NbToastrService } from '@nebular/theme';

import { AgentService } from '../../../../@core/data/agent.service';
import { ClusterContextService } from '../../../../@core/data/cluster-context.service';
import { Cluster } from '../../../../@core/data/cluster.service';
import { I18nService } from '../../../../@core/i18n/i18n.service';
import { AgentChatService } from '../../../../@core/data/agent-chat.service';
import { ConfirmDialogService } from '../../../../@core/services/confirm-dialog.service';
import { AgentComponent } from '../agent.component';

describe('AgentComponent', () => {
  let component: AgentComponent;
  const toastr = {
    danger: jasmine.createSpy('danger'),
    warning: jasmine.createSpy('warning'),
  };
  const chatService = {
    isRunning: () => false,
  };

  beforeEach(() => {
    toastr.danger.calls.reset();
    toastr.warning.calls.reset();
    TestBed.configureTestingModule({
      providers: [
        { provide: ActivatedRoute, useValue: {} },
        { provide: AgentService, useValue: {} },
        { provide: AgentChatService, useValue: chatService },
        { provide: ConfirmDialogService, useValue: {} },
        { provide: ChangeDetectorRef, useValue: { detectChanges: () => undefined } },
        { provide: ClusterContextService, useValue: {} },
        { provide: I18nService, useValue: { instant: (value: string) => value } },
        { provide: NbToastrService, useValue: toastr },
        { provide: ElementRef, useValue: new ElementRef(document.createElement('div')) },
      ],
    });

    component = TestBed.runInInjectionContext(() => new AgentComponent());
  });

  it('uses storage and cache guidance for shared-data clusters', () => {
    component.cluster = { deployment_mode: 'shared_data' } as Cluster;

    expect(component.presets[2]).toBe('对象存储或缓存是否影响查询性能？');
    expect(component.emptyStateHint).toContain('存储与缓存性能');
  });

  it('retains disk guidance for shared-nothing clusters', () => {
    component.cluster = { deployment_mode: 'shared_nothing' } as Cluster;

    expect(component.presets[2]).toBe('磁盘快满了吗，还能撑多久？');
    expect(component.emptyStateHint).toContain('磁盘告急');
  });

  it('keeps structured tools while hiding legacy freeform reasoning from diagnostic activity', () => {
    const timeline = (component as any).buildTimeline([
      {
        kind: 'reasoning',
        label: 'reasoning',
        detail: '模型的自由推理草稿不应显示给用户',
        duration_ms: 0,
        status: 'ok',
      },
      {
        kind: 'tool',
        label: 'query_nodes',
        detail: '',
        args: { node_id: 'be-1' },
        duration_ms: 0,
        status: 'pending',
      },
      {
        kind: 'tool_result',
        label: 'query_nodes',
        detail: '',
        result: '发现 1 个异常节点',
        duration_ms: 420,
        status: 'ok',
      },
    ]);

    expect(timeline.length).toBe(1);
    expect(timeline[0].title).toBe('查询节点状态');
    expect(timeline[0].args).toEqual({ node_id: 'be-1' });
    expect(timeline[0].detail).toBe('发现 1 个异常节点');
  });

  it('restores an action card from a persisted action tool result', () => {
    const message = {
      role: 'assistant',
      content: '',
      steps: [
        {
          kind: 'tool_result',
          label: 'propose_action',
          detail: '',
          result: 'ACTION_PENDING:{"id":7,"kind":"kill_query","title":"KILL QUERY a1","params":{"query_id":"a1"},"expires_at":"2099-01-01 00:00:00"}',
          duration_ms: 12,
          status: 'ok',
        },
      ],
    };

    const actions = (component as any).messageActions(message);

    expect(actions.length).toBe(1);
    expect(actions[0].id).toBe(7);
    expect(component.actionPreview(actions[0])).toBe('KILL QUERY a1');
  });

  it('opens the matching diagnostic activity when an evidence reference is selected', () => {
    const message = {
      id: 9,
      role: 'assistant',
      content: '',
      steps: [
        {
          kind: 'tool',
          label: 'query_nodes',
          detail: '',
          args: {},
          duration_ms: 0,
          status: 'pending',
        },
        {
          kind: 'tool_result',
          label: 'query_nodes',
          detail: '',
          result: '发现 1 个异常节点',
          duration_ms: 10,
          status: 'ok',
        },
      ],
    } as any;
    const evidence = component.evidence(message)[0];

    component.showEvidence(message, evidence);

    expect(message.evidenceOpen).toBeTrue();
    expect(message.traceOpen).toBeTrue();
    expect(evidence.resultOpen).toBeTrue();
    expect(component.traceAnchorId(message, 0)).toBe('agent-trace-9-0');
    expect(component.evidenceListId(message)).toBe('agent-evidence-9');
  });

  it('treats a pending action past its deadline as expired and non-interactive', () => {
    const action = {
      id: 8,
      session_id: 1,
      kind: 'update_variable',
      title: 'SET GLOBAL query_timeout = 10',
      params: { scope: 'global', key: 'query_timeout', value: '10' },
      status: 'pending' as const,
      action_uuid: 'action-8',
      created_by: 'ops',
      created_at: '2020-01-01 00:00:00',
      expires_at: '2020-01-01 00:01:00',
    };

    expect(component.actionStatus(action)).toBe('expired');
    expect(component.actionCanRespond(action)).toBeFalse();
    expect(component.actionPreview(action)).toBe('SET GLOBAL query_timeout = 10');
  });

  it('opens the next session when navigating the session list with ArrowDown', () => {
    component.sessions = [
      { id: 11, title: 'first', last_active_at: '2026-09-20 00:00:00' },
      { id: 12, title: 'second', last_active_at: '2026-09-20 00:00:00' },
    ] as any;
    component.activeSessionId = 11;
    const event = new KeyboardEvent('keydown', { key: 'ArrowDown' });
    const openSession = spyOn(component, 'openSession');
    const preventDefault = spyOn(event, 'preventDefault').and.callThrough();

    component.onSessionListKeydown(event);

    expect(preventDefault).toHaveBeenCalled();
    expect(openSession).toHaveBeenCalledWith(12);
  });

  it('explains why a message is not sent while another turn is still running', () => {
    spyOn(chatService, 'isRunning').and.returnValue(true);
    component.cluster = { id: 3 } as Cluster;
    component.input = '检查当前状态';

    component.send();

    expect(toastr.warning).toHaveBeenCalledWith('正在处理上一条消息，请等待完成或停止当前诊断', '智能运维助手');
  });

  it('shows an error notification and ends the assistant placeholder on stream failure', () => {
    const assistant = { role: 'assistant', content: '', steps: [], streaming: true } as any;
    component.sending = true;
    (component as any).turnMessage = assistant;

    (component as any).applyStreamEvent(assistant, { type: 'error', message: '模型服务暂时不可用（HTTP 503）' });

    expect(assistant.content).toBe('⚠️ 模型服务暂时不可用（HTTP 503）');
    expect(assistant.streaming).toBeFalse();
    expect(component.sending).toBeFalse();
    expect(toastr.danger).toHaveBeenCalledWith('模型服务暂时不可用（HTTP 503）', '智能运维助手');
  });
});
