import { TestBed } from '@angular/core/testing';
import { Subject, of } from 'rxjs';

import { AgentChatService } from '../agent-chat.service';
import { AgentService, ChatStreamEvent } from '../agent.service';
import { NotificationService } from '../notification.service';

describe('AgentChatService', () => {
  let service: AgentChatService;
  let stream: Subject<ChatStreamEvent>;

  beforeEach(() => {
    stream = new Subject<ChatStreamEvent>();
    TestBed.configureTestingModule({
      providers: [
        { provide: AgentService, useValue: { chatStream: () => stream.asObservable() } },
        { provide: NotificationService, useValue: { create: () => of({ id: 1 }) } },
      ],
    });
    service = TestBed.runInInjectionContext(() => new AgentChatService());
  });

  it('releases the global turn lock after an SSE error event', () => {
    const events: ChatStreamEvent[] = [];
    service.events().subscribe((event) => events.push(event));

    service.start({ cluster_id: 1, message: '检查集群' });
    stream.next({ type: 'error', message: 'LLM API error 503' });
    stream.next({ type: 'error', message: 'duplicate error' });

    expect(service.isRunning()).toBeFalse();
    expect(events).toEqual([{ type: 'error', message: 'LLM API error 503' }]);
  });

  it('reports an unexpected stream completion and releases the global turn lock', () => {
    const events: ChatStreamEvent[] = [];
    service.events().subscribe((event) => events.push(event));

    service.start({ cluster_id: 1, message: '检查集群' });
    stream.complete();

    expect(service.isRunning()).toBeFalse();
    expect(events).toEqual([{ type: 'error', message: '诊断流意外中断，请重试' }]);
  });

  it('releases the global turn lock after a request error', () => {
    const events: ChatStreamEvent[] = [];
    service.events().subscribe((event) => events.push(event));

    service.start({ cluster_id: 1, message: '检查集群' });
    stream.error(new Error('HTTP 503'));

    expect(service.isRunning()).toBeFalse();
    expect(events).toEqual([{ type: 'error', message: 'HTTP 503' }]);
  });

  it('releases the global turn lock when the user stops the turn', () => {
    service.start({ cluster_id: 1, message: '检查集群' });

    service.stop();

    expect(service.isRunning()).toBeFalse();
  });

  it('retains the active session across assistant surfaces', () => {
    const selected: Array<number | null> = [];
    service.activeSession$.subscribe((sessionId) => selected.push(sessionId));

    service.setActiveSession(42);
    service.setActiveSession(42);
    service.setActiveSession(null);

    expect(selected).toEqual([null, 42, null]);
    expect(service.getActiveSession()).toBeNull();
  });

  it('records a newly created session when a turn finishes off the assistant page', () => {
    service.start({ cluster_id: 1, message: '检查集群' });

    stream.next({ type: 'done', session_id: 42 });

    expect(service.getActiveSession()).toBe(42);
  });
});
