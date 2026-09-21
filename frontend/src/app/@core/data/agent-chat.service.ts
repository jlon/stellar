import { Injectable, inject } from '@angular/core';
import { Observable, Subject, Subscription } from 'rxjs';
import { AgentService, ChatRequest, ChatStreamEvent } from './agent.service';
import { NotificationService } from './notification.service';

/**
 * 全局聊天回合通道（app 级单例）：
 *
 * 为什么需要全局？回合在用户切走页面后必须**继续完成**，并在完成后通知。
 * 若回合订阅挂在组件上，组件销毁（路由切换）会取消 fetch → 服务端回合中断，永远没有"完成"。
 * 本服务持有回合订阅，组件只订阅广播事件；页面/浮窗销毁不打断回合。
 *
 * 通知判定（产品语义）：回合完成（done）时，若**没有任何会话 UI 在前台**
 * （全量页面未打开且浮窗未展开）→ 右上角铃铛通知，点击可回到对应会话。
 */
@Injectable({ providedIn: 'root' })
export class AgentChatService {
  private agentService = inject(AgentService);
  private notificationService = inject(NotificationService);

  private turn$ = new Subject<ChatStreamEvent>();
  private turnSub: Subscription | null = null;
  /** 任何会话 UI（全量页面 / 展开的浮窗）是否在前台。 */
  private uiFront = false;
  private running = false;
  private lastAnswer = '';

  events(): Observable<ChatStreamEvent> {
    return this.turn$.asObservable();
  }

  isUiFront(): boolean {
    return this.uiFront;
  }

  /** 会话 UI 显隐（全量页面 / 浮窗展开）由组件上报。 */
  setUiFront(front: boolean): void {
    this.uiFront = front;
  }

  isRunning(): boolean {
    return this.running;
  }

  /** 停止当前回合（GPT 同款）：断开 SSE，后端在轮次边界取消，不再烧 token。 */
  stop(): void {
    this.turnSub?.unsubscribe();
    this.turnSub = null;
    this.running = false;
  }

  /** 发起一个回合（同一时间只允许一个回合在跑，多入口共用一条通道）。 */
  start(req: ChatRequest): void {
    if (this.running) {
      return;
    }
    this.running = true;
    this.lastAnswer = '';
    this.turnSub?.unsubscribe();

    this.turnSub = this.agentService.chatStream(req).subscribe({
      next: (ev: ChatStreamEvent) => {
        if (ev.type === 'error') {
          this.finishWithError(ev.message ?? '诊断失败');
          return;
        }
        if (ev.type === 'answer' && ev.final_answer) {
          this.lastAnswer = ev.final_answer;
        }
        this.turn$.next(ev);
        if (ev.type === 'done') {
          this.running = false;
          this.turnSub = null;
          // 完成时才通知：用户若仍在前台（页面/浮窗展开）正在看答案，不打扰
          if (!this.uiFront && this.lastAnswer) {
            const body =
              this.lastAnswer.length > 120
                ? this.lastAnswer.slice(0, 120) + '…'
                : this.lastAnswer;
            const link = `/pages/cluster-ops/agent?session=${ev.session_id ?? ''}`;
            this.notificationService
              .create('agent_chat_done', '智能运维诊断完成', body, link)
              .subscribe({ error: () => {} });
          }
        }
      },
      error: (err) => this.finishWithError(err?.error?.message ?? err?.message ?? '请求失败'),
      complete: () => {
        if (this.running) {
          this.finishWithError('诊断流意外中断，请重试');
        }
      },
    });
  }

  /** 所有非成功终止路径都必须释放全局回合锁，避免后续发送被静默拦截。 */
  private finishWithError(message: string): void {
    if (!this.running) {
      return;
    }
    this.running = false;
    this.turnSub = null;
    this.turn$.next({ type: 'error', message });
    // 失败也要通知（产品语义：完成或出错都需要触达）
    if (!this.uiFront) {
      this.notificationService
        .create(
          'agent_chat_error',
          '智能运维诊断失败',
          message.length > 120 ? message.slice(0, 120) + '…' : message,
          '/pages/cluster-ops/agent',
          'critical',
        )
        .subscribe({ error: () => {} });
    }
  }
}
