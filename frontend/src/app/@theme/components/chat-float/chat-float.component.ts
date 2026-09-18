import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
//! 全局浮动聊天入口（Intercom 式）：任意页面右下角气泡 → 展开迷你聊天窗。
//! 复用 AgentService（同一后端 /api/agent/chat/stream），独立轻量逻辑，
//! 完整工具链/会话管理仍在 /pages/cluster-ops/agent 全量页面。

import { ChangeDetectorRef, Component, OnInit, OnDestroy, inject } from '@angular/core';
import { Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbButtonModule, NbCardModule, NbIconModule, NbInputModule, NbSelectModule, NbTooltipModule } from '@nebular/theme';
import { NbEvaIconsModule } from '@nebular/eva-icons';
import { NbToastrModule } from '@nebular/theme';
import { NbToastrService } from '@nebular/theme';
import { MarkdownModule } from 'ngx-markdown';
import { AgentService, AgentSession, AgentMessage, ChatActionRequest } from '../../../@core/data/agent.service';
import { AgentChatService } from '../../../@core/data/agent-chat.service';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { AiIllustrationComponent } from '../ai-illustration/ai-illustration.component';
import { Cluster } from '../../../@core/data/cluster.service';
import { PermissionService } from '../../../@core/data/permission.service';

interface FloatMsg {
  id?: number;
  role: 'user' | 'assistant';
  content: string;
  liveText?: string;
  phase?: 'reasoning' | 'answer';
  streaming?: boolean;
  feedback?: string | null;
  copied?: boolean;
  stopped?: boolean;
}

@Component({
  selector: 'ngx-chat-float',
  templateUrl: './chat-float.component.html',
  styleUrls: ['./chat-float.component.scss'],
  standalone: true,
  imports: [
    TranslatePipe,
    CommonModule,
    FormsModule,
    NbIconModule,
    NbButtonModule,
    NbInputModule,
    NbSelectModule,
    NbTooltipModule,
    NbCardModule,
    NbEvaIconsModule,
    NbToastrModule,
    MarkdownModule,
    AiIllustrationComponent,
  ],
})
export class ChatFloatComponent implements OnInit, OnDestroy {
  private agentService = inject(AgentService)
  private i18n = inject(I18nService);;
  private chatService = inject(AgentChatService);
  private router = inject(Router);
  private clusterContext = inject(ClusterContextService);
  private permissionService = inject(PermissionService);
  private toastr = inject(NbToastrService);
  private cdRef = inject(ChangeDetectorRef);
  private destroy$ = new Subject<void>();

  /** 无 agent 权限时不渲染 */
  visible = false;
  /** 用户主动关闭过入口（localStorage 记忆，刷新不再出现） */
  dismissed = false;
  private readonly dismissedKey = 'chat-float-dismissed';
  open = false;
  private currentCluster: Cluster | null = null;
  clusterName = '';
  sessions: AgentSession[] = [];
  activeSessionId: number | null = null;
  messages: FloatMsg[] = [];
  input = '';
  sending = false;
  loading = false;
  /** 对话内动作申请（确认卡，与主面板同源）。 */
  pendingActions: ChatActionRequest[] = [];
  private liveReply: FloatMsg | null = null;
  private liveSub: { unsubscribe(): void } | null = null;
  private streamRenderFrame: number | null = null;

  ngOnInit(): void {
    this.visible = this.permissionService.hasPermission('menu:agent');
    this.dismissed = localStorage.getItem(this.dismissedKey) === '1';

    this.clusterContext.activeCluster$
      .pipe(takeUntil(this.destroy$))
      .subscribe((c) => {
        this.currentCluster = c;
        if (!c) {
          this.clusterName = '';
          this.sessions = [];
          this.activeSessionId = null;
          this.messages = [];
          return;
        }
        this.clusterName = c.name;
        this.reloadSessions();
      });
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
    }
  }

  dismiss(): void {
    this.dismissed = true;
    localStorage.setItem(this.dismissedKey, '1');
  }

  toggle(): void {
    this.open = !this.open;
    this.chatService.setUiFront(this.open);
    if (this.open && this.messages.length === 0 && this.activeSessionId) {
      this.loadTranscript();
    }
  }

  onSessionChange(id: number | null): void {
    if (id === null) {
      this.newSession();
    } else {
      this.selectSession(id);
    }
  }

  newSession(): void {
    this.activeSessionId = null;
    this.messages = [];
    this.input = '';
    this.pendingActions = [];
  }

  selectSession(id: number): void {
    this.activeSessionId = id;
    this.messages = [];
    this.pendingActions = [];
    this.loadTranscript();
  }

  private reloadSessions(): void {
    const c = this.currentCluster;
    if (!c) {
      return;
    }
    this.loading = true;
    this.agentService.listSessions(c.id).subscribe({
      next: (items) => {
        this.sessions = items;
        this.loading = false;
        // 保持现有会话
        if (items.length > 0 && !items.some((s) => s.id === this.activeSessionId)) {
          this.activeSessionId = items[0].id;
          // 收起态不预取完整转录：主页面首次打开时会与用户的点击请求重复，
          // 白白竞争网络与主线程。浮窗真正展开后 toggle() 会按需加载。
          if (this.open) {
            this.loadTranscript();
          }
        }
      },
      error: () => {
        this.loading = false;
      },
    });
  }

  private loadTranscript(): void {
    if (!this.activeSessionId) {
      return;
    }
    this.agentService.getSession(this.activeSessionId).subscribe({
      next: (msgs) => {
        this.messages = msgs.map((m) => ({
          id: m.id,
          role: m.role,
          content: m.content,
          feedback: m.feedback ?? null,
        }));
      },
      error: () => {},
    });
  }

  send(): void {
    const text = this.input.trim();
    if (!text || this.sending || !this.currentCluster || this.chatService.isRunning()) {
      return;
    }
    const clusterId = this.currentCluster.id;
    this.sending = true;
    const userMsg: FloatMsg = { role: 'user', content: text };
    const reply: FloatMsg = { role: 'assistant', content: '', liveText: '', streaming: true };
    this.messages.push(userMsg, reply);
    this.liveReply = reply;
    this.pendingActions = [];
    this.input = '';
    this.resetFloatInputHeight();
    this.scrollToBottom();

    // 回合交全局通道：浮窗收起后回合继续并触发铃铛通知；
    // 附带当前页面上下文（sxdevops 页面 Copilot）。
    const pageCtx = {
      page: this.router.url,
      params: {},
    };
    this.chatService.start({
      session_id: this.activeSessionId ?? undefined,
      cluster_id: clusterId,
      message: text,
      context: pageCtx,
    });
    const sub = this.chatService.events().subscribe((ev) => {
      if (ev.type === 'delta' && ev.text) {
        reply.liveText = (reply.liveText ?? '') + ev.text;
      } else if (ev.type === 'phase') {
        reply.phase = ev.phase as 'reasoning' | 'answer';
        if (reply.phase === 'reasoning') {
          reply.liveText = '';
        }
      } else if (ev.type === 'answer') {
        reply.content = ev.final_answer ?? reply.liveText ?? '';
        reply.liveText = reply.content;
      } else if (ev.type === 'action_request' && ev.action) {
        if (!this.pendingActions.some((a) => a.id === ev.action!.id)) {
          this.pendingActions.push(ev.action!);
        }
      } else if (ev.type === 'done') {
        this.activeSessionId = ev.session_id ?? this.activeSessionId;
        reply.content = reply.content || reply.liveText || '';
        reply.liveText = undefined;
        reply.streaming = false;
        this.sending = false;
        this.liveReply = null;
        sub.unsubscribe();
        this.liveSub = null;
        this.reloadSessions();
        // 刷新转录拿到持久化消息 id（点赞状态回显用）。
        this.loadTranscript();
      } else if (ev.type === 'error') {
        reply.content = `⚠️ ${ev.message ?? '诊断失败'}`;
        reply.liveText = undefined;
        reply.streaming = false;
        this.sending = false;
        this.liveReply = null;
        sub.unsubscribe();
        this.liveSub = null;
      }
      this.scheduleStreamRender();
    });
    this.liveSub = sub;
  }

  /** 停止当前回合：保留已到文本。 */
  stopTurn(): void {
    if (!this.sending || !this.liveReply) {
      return;
    }
    this.chatService.stop();
    const reply = this.liveReply;
    reply.content = reply.content || reply.liveText || '';
    reply.liveText = undefined;
    reply.streaming = false;
    reply.stopped = true;
    this.sending = false;
    this.liveReply = null;
    this.liveSub?.unsubscribe();
    this.liveSub = null;
    this.scrollToBottom();
  }

  confirmFloatAction(action: ChatActionRequest): void {
    this.agentService.confirmChatAction(action.id).subscribe({
      next: (r) => {
        this.toastr.success(
          r.action.status === 'executed' ? r.action.result_json ?? '执行成功' : r.action.result_json ?? '执行失败',
          '动作执行',
        );
        this.pendingActions = this.pendingActions.filter((x) => x.id !== action.id);
        this.loadTranscript();
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '确认失败', '动作执行'),
    });
  }

  rejectFloatAction(action: ChatActionRequest): void {
    this.agentService.cancelChatAction(action.id).subscribe({
      next: () => {
        this.toastr.success(this.i18n.instant('已拒绝该动作'), this.i18n.instant('动作执行'));
        this.pendingActions = this.pendingActions.filter((x) => x.id !== action.id);
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '取消失败', '动作执行'),
    });
  }

  /** 回答点赞/点踩（再点一次取消）。 */
  rateFloat(m: FloatMsg, value: 'up' | 'down'): void {
    if (!m.id || m.streaming) {
      return;
    }
    const next = m.feedback === value ? null : value;
    this.agentService.setMessageFeedback(m.id, next).subscribe({
      next: (r) => {
        m.feedback = r.feedback;
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '评价失败', '智能运维助手'),
    });
  }

  copyFloat(m: FloatMsg): void {
    const text = m.content || '';
    if (!text || text.startsWith('⚠️')) {
      return;
    }
    const done = () => {
      m.copied = true;
      setTimeout(() => {
        m.copied = false;
      }, 1500);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(() => done());
    } else {
      done();
    }
  }

  /** 回车发送 / Shift+回车换行（中文输入法组词中不触发）。 */
  onFloatKey(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) {
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      this.send();
    }
  }

  autoGrowFloat(event: Event): void {
    const el = event.target as HTMLTextAreaElement | null;
    if (!el) {
      return;
    }
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 120) + 'px';
  }

  private resetFloatInputHeight(): void {
    const el = document.querySelector('.chat-float-input .float-field') as HTMLTextAreaElement | null;
    if (el) {
      el.style.height = 'auto';
    }
  }

  private scheduleStreamRender(): void {
    if (this.streamRenderFrame !== null) {
      return;
    }
    this.streamRenderFrame = requestAnimationFrame(() => {
      this.streamRenderFrame = null;
      this.cdRef.detectChanges();
      this.scrollToBottom();
    });
  }

  private scrollToBottom(): void {
    setTimeout(() => {
      const el = document.querySelector('.chat-float-messages');
      if (el) {
        el.scrollTop = el.scrollHeight;
      }
    }, 0);
  }
}
