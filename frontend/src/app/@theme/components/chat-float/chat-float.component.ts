import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
//! 全局助手入口：离开全量助手页后保留右下角图标，点击展开 Nebular 右侧抽屉。
//! 抽屉复用 AgentService 与 AgentChatService，不另建会话或流式通道。

import { ChangeDetectorRef, Component, HostBinding, HostListener, Input, OnDestroy, OnInit, booleanAttribute, inject } from '@angular/core';
import { NavigationEnd, Router } from '@angular/router';

import { FormsModule } from '@angular/forms';
import { Subject } from 'rxjs';
import { filter, takeUntil } from 'rxjs/operators';
import { NbButtonModule, NbIconModule, NbInputModule, NbSidebarService, NbTooltipModule } from '@nebular/theme';
import { NbEvaIconsModule } from '@nebular/eva-icons';
import { NbToastrModule } from '@nebular/theme';
import { NbToastrService } from '@nebular/theme';
import { MarkdownModule } from 'ngx-markdown';
import { AgentService, AgentSession, ChatActionRequest } from '../../../@core/data/agent.service';
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

const ASSISTANT_DRAWER_TAG = 'assistant-drawer';
const ASSISTANT_ROUTE = '/pages/cluster-ops/agent';

@Component({
  selector: 'ngx-chat-float',
  templateUrl: './chat-float.component.html',
  styleUrls: ['./chat-float.component.scss'],
  standalone: true,
  imports: [
    TranslatePipe,
    FormsModule,
    NbIconModule,
    NbButtonModule,
    NbInputModule,
    NbTooltipModule,
    NbEvaIconsModule,
    NbToastrModule,
    MarkdownModule,
    AiIllustrationComponent
],
})
export class ChatFloatComponent implements OnInit, OnDestroy {
  @Input({ transform: booleanAttribute }) drawer = false;
  @HostBinding('class.drawer-mode') get drawerMode(): boolean {
    return this.drawer;
  }
  @HostBinding('class.launcher-mode') get launcherMode(): boolean {
    return !this.drawer;
  }
  @HostBinding('style.left') get launcherLeft(): string | null {
    return this.launcherPosition ? `${this.launcherPosition.left}px` : null;
  }
  @HostBinding('style.top') get launcherTop(): string | null {
    return this.launcherPosition ? `${this.launcherPosition.top}px` : null;
  }
  @HostBinding('style.right') get launcherRight(): string | null {
    return this.launcherPosition ? 'auto' : null;
  }
  @HostBinding('style.bottom') get launcherBottom(): string | null {
    return this.launcherPosition ? 'auto' : null;
  }

  private agentService = inject(AgentService);
  private i18n = inject(I18nService);
  private chatService = inject(AgentChatService);
  private router = inject(Router);
  private sidebarService = inject(NbSidebarService);
  private clusterContext = inject(ClusterContextService);
  private permissionService = inject(PermissionService);
  private toastr = inject(NbToastrService);
  private cdRef = inject(ChangeDetectorRef);
  private destroy$ = new Subject<void>();

  /** 无 agent 权限或仍在全量助手页时，不显示右下角入口。 */
  visible = false;
  open = false;
  /** 轨道默认收起；展开后呈现当前集群的历史会话。 */
  historyExpanded = false;
  private drawerOpen = false;
  private launcherDismissed = false;
  private launcherPosition: { left: number; top: number } | null = null;
  private launcherDrag: { pointerId: number; startX: number; startY: number; left: number; top: number } | null = null;
  private launcherDragMoved = false;
  private suppressLauncherClick = false;
  private readonly launcherDiameter = 60;
  private readonly launcherInset = 20;
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
    if (this.drawer) {
      this.initDrawer();
      return;
    }

    this.initLauncher();
  }

  private initLauncher(): void {
    this.permissionService.permissions$
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.updateLauncherVisibility());
    this.router.events
      .pipe(filter((event) => event instanceof NavigationEnd), takeUntil(this.destroy$))
      .subscribe(() => this.updateLauncherVisibility());
    this.sidebarService.onExpand()
      .pipe(filter(({ tag }) => tag === ASSISTANT_DRAWER_TAG), takeUntil(this.destroy$))
      .subscribe(() => {
        this.drawerOpen = true;
        this.updateLauncherVisibility();
      });
    this.sidebarService.onCollapse()
      .pipe(filter(({ tag }) => tag === ASSISTANT_DRAWER_TAG), takeUntil(this.destroy$))
      .subscribe(() => {
        this.drawerOpen = false;
        this.updateLauncherVisibility();
      });
    this.updateLauncherVisibility();
  }

  private initDrawer(): void {
    this.sidebarService.onExpand()
      .pipe(filter(({ tag }) => tag === ASSISTANT_DRAWER_TAG), takeUntil(this.destroy$))
      .subscribe(() => this.setDrawerOpen(true));
    this.sidebarService.onCollapse()
      .pipe(filter(({ tag }) => tag === ASSISTANT_DRAWER_TAG), takeUntil(this.destroy$))
      .subscribe(() => this.setDrawerOpen(false));

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

    this.chatService.activeSession$
      .pipe(takeUntil(this.destroy$))
      .subscribe((sessionId) => this.syncActiveSession(sessionId));
    this.chatService.events()
      .pipe(takeUntil(this.destroy$))
      .subscribe((event) => {
        // 自己发起的回合由 send() 的订阅维护实时文本；这里只接续全量页离开后的回合。
        if (this.sending || event.type !== 'done') {
          return;
        }
        const sessionId = event.session_id ?? this.chatService.getActiveSession();
        if (sessionId) {
          this.chatService.setActiveSession(sessionId);
          if (this.open) {
            this.loadTranscript();
          }
        }
        this.reloadSessions();
      });
  }

  private updateLauncherVisibility(): void {
    const isAssistantPage = this.router.url.split('?')[0] === ASSISTANT_ROUTE;
    this.visible = this.permissionService.hasPermission('menu:agent')
      && !isAssistantPage
      && !this.drawerOpen
      && !this.launcherDismissed;
    if (isAssistantPage) {
      this.sidebarService.collapse(ASSISTANT_DRAWER_TAG);
    }
  }

  private setDrawerOpen(open: boolean): void {
    this.open = open;
    this.chatService.setUiFront(open);
    if (!open) {
      return;
    }
    const handoff = this.chatService.takeMemoryProfile();
    if (handoff) {
      if (handoff.clusterId !== this.currentCluster?.id) {
        this.toastr.warning('活动集群已切换，请重新打开该 Profile', '智能助手');
      } else if (this.chatService.isRunning()) {
        this.toastr.warning('智能助手正在处理上一条消息，请稍后重试', '智能助手');
      } else {
        this.newSession();
        this.input = handoff.message;
        this.send();
        return;
      }
    }
    if (!this.sessions.length && !this.sending) {
      this.reloadSessions();
    }
    if (this.activeSessionId) {
      this.loadTranscript();
    }
  }

  private syncActiveSession(sessionId: number | null): void {
    if (this.activeSessionId === sessionId) {
      return;
    }
    this.activeSessionId = sessionId;
    this.messages = [];
    this.pendingActions = [];
    if (this.open && sessionId) {
      this.loadTranscript();
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
    }
    this.liveSub?.unsubscribe();
    if (this.drawer) {
      this.chatService.setUiFront(false);
    }
  }

  /** 仅抽屉展开时响应外部点击；overlay 属于当前交互的一部分，不触发收起。 */
  @HostListener('document:pointerdown', ['$event'])
  onDocumentPointerDown(event: PointerEvent): void {
    if (!this.drawer || !this.open || !(event.target instanceof Node)) {
      return;
    }
    const sidebar = document.querySelector<HTMLElement>('nb-sidebar.assistant-drawer');
    if (sidebar?.contains(event.target)) {
      return;
    }
    const target = event.target instanceof Element ? event.target : event.target.parentElement;
    if (target?.closest('.cdk-overlay-container, nb-overlay-container')) {
      return;
    }
    this.closeDrawer();
  }

  @HostListener('document:keydown.escape')
  onEscape(): void {
    if (this.drawer && this.open) {
      this.closeDrawer();
    }
  }

  onLauncherPointerDown(event: PointerEvent): void {
    if (this.drawer || event.button !== 0 || !(event.currentTarget instanceof HTMLElement)) {
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    this.launcherDrag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      left: rect.left,
      top: rect.top,
    };
    this.launcherDragMoved = false;
    this.suppressLauncherClick = false;
  }

  @HostListener('document:pointermove', ['$event'])
  onLauncherPointerMove(event: PointerEvent): void {
    const drag = this.launcherDrag;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    const deltaX = event.clientX - drag.startX;
    const deltaY = event.clientY - drag.startY;
    if (!this.launcherDragMoved && Math.hypot(deltaX, deltaY) < 4) {
      return;
    }
    event.preventDefault();
    this.launcherDragMoved = true;
    this.launcherPosition = this.clampLauncherPosition(drag.left + deltaX, drag.top + deltaY);
  }

  @HostListener('document:pointerup', ['$event'])
  @HostListener('document:pointercancel', ['$event'])
  onLauncherPointerEnd(event: PointerEvent): void {
    const drag = this.launcherDrag;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    this.suppressLauncherClick = this.launcherDragMoved;
    this.launcherDragMoved = false;
    this.launcherDrag = null;
  }

  @HostListener('window:resize')
  onViewportResize(): void {
    if (this.launcherPosition) {
      this.launcherPosition = this.clampLauncherPosition(this.launcherPosition.left, this.launcherPosition.top);
    }
  }

  private clampLauncherPosition(left: number, top: number): { left: number; top: number } {
    const maxLeft = Math.max(this.launcherInset, window.innerWidth - this.launcherDiameter - this.launcherInset);
    const maxTop = Math.max(this.launcherInset, window.innerHeight - this.launcherDiameter - this.launcherInset);
    return {
      left: Math.min(maxLeft, Math.max(this.launcherInset, left)),
      top: Math.min(maxTop, Math.max(this.launcherInset, top)),
    };
  }

  openDrawer(): void {
    if (this.suppressLauncherClick) {
      this.suppressLauncherClick = false;
      return;
    }
    this.sidebarService.expand(ASSISTANT_DRAWER_TAG);
  }

  dismissLauncher(event: Event): void {
    event.preventDefault();
    event.stopPropagation();
    this.launcherDismissed = true;
    this.visible = false;
  }

  closeDrawer(): void {
    this.sidebarService.collapse(ASSISTANT_DRAWER_TAG);
  }

  toggleHistory(): void {
    this.historyExpanded = !this.historyExpanded;
  }

  /** 高风险动作只在完整助手中确认，浮窗只负责提醒和跳转。 */
  openAgentReview(): void {
    const session = this.activeSessionId;
    this.closeDrawer();
    this.router.navigate(['/pages/cluster-ops/agent'], {
      queryParams: session ? { session } : undefined,
    });
  }

  newSession(): void {
    this.activeSessionId = null;
    this.chatService.setActiveSession(null);
    this.messages = [];
    this.input = '';
    this.pendingActions = [];
  }

  selectSession(id: number): void {
    this.activeSessionId = id;
    this.chatService.setActiveSession(id);
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
        const selected = this.chatService.getActiveSession();
        const nextSession = items.some((session) => session.id === selected)
          ? selected
          : items[0]?.id ?? null;
        if (nextSession !== this.activeSessionId) {
          this.activeSessionId = nextSession;
          this.messages = [];
          this.pendingActions = [];
        }
        if (nextSession !== selected) {
          this.chatService.setActiveSession(nextSession);
        }
        // 收起态不预取完整转录，抽屉展开时再加载。
        if (this.open && this.activeSessionId) {
          this.loadTranscript();
        }
      },
      error: () => {
        this.loading = false;
      },
    });
  }

  private loadTranscript(): void {
    const sessionId = this.activeSessionId;
    if (!sessionId) {
      return;
    }
    this.agentService.getSession(sessionId).subscribe({
      next: (msgs) => {
        if (this.activeSessionId !== sessionId) {
          return;
        }
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
    if (!text || this.sending || !this.currentCluster) {
      return;
    }
    if (this.chatService.isRunning()) {
      this.toastr.warning(
        this.i18n.instant('正在处理上一条消息，请等待完成或停止当前诊断'),
        this.i18n.instant('智能助手'),
      );
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
    const route = this.router.url.split(/[?#]/, 1)[0];
    const pageCtx = route.startsWith('/pages/')
      ? { page: 'current_route' as const, params: { route } }
      : undefined;
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
        this.chatService.setActiveSession(this.activeSessionId);
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
        const message = ev.message ?? '诊断失败';
        reply.content = `⚠️ ${message}`;
        reply.liveText = undefined;
        reply.streaming = false;
        this.sending = false;
        this.liveReply = null;
        sub.unsubscribe();
        this.liveSub = null;
        this.toastr.danger(message, this.i18n.instant('智能助手'));
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
      error: (e) => this.toastr.danger(e?.error?.message ?? '评价失败', '智能助手'),
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
