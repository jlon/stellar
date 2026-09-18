import { I18nService } from '../../../@core/i18n/i18n.service';
import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnInit, OnDestroy, ChangeDetectorRef, ElementRef, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { MarkdownModule } from 'ngx-markdown';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { NbCardModule, NbButtonModule, NbIconModule, NbInputModule, NbSpinnerModule, NbListModule, NbTooltipModule, NbToastrService } from '@nebular/theme';

import {
  AgentService,
  AgentSession,
  AgentMessage,
  ChatStreamEvent,
  ChatActionRequest,
} from '../../../@core/data/agent.service';
import { AgentChatService } from '../../../@core/data/agent-chat.service';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { AiIllustrationComponent } from '../../../@theme/components/ai-illustration/ai-illustration.component';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { Subscription } from 'rxjs';

/** 空态引导的预设问题（OLAP 使用者的高频心智：查得慢 / 跑批卡 / 磁盘紧 / 导入堵）。 */
const PRESET_QUESTIONS = [
  '为什么最近查询变慢了？',
  '这条慢查询到底慢在哪？',
  '磁盘快满了吗，还能撑多久？',
  '现在有导入积压或大查询抢资源吗？',
];

interface ViewMessage {
  id?: number;
  role: 'user' | 'assistant';
  content: string;
  steps: AgentMessage['steps'];
  /** 当前 LLM 轮正在抵达的文本，直接以 Markdown 形式实时渲染。 */
  liveText?: string;
  /** 当前轮语义仅用于工具回合完成后清理临时草稿。 */
  phase?: 'reasoning' | 'answer';
  /** 流式中显示光标；done 后切为持久化的最终答案。 */
  streaming?: boolean;
  /** 思考过程折叠展开（默认收起）。 */
  traceOpen?: boolean;
  /** 复制反馈（短暂显示“已复制”）。 */
  copied?: boolean;
  /** 当前用户的评价（up/down）。 */
  feedback?: string | null;
  /** 用户手动停止的回合（保留已到文本 + 停止标记）。 */
  stopped?: boolean;
}

/** 思考过程时间线项：思考与工具调用按发生顺序交错排列。 */
interface TraceItem {
  kind: 'thinking' | 'tool' | 'error';
  title: string;
  detail?: string;
  durationMs?: number;
  /** undefined = 仍在执行中。 */
  ok?: boolean;
  /** 长详情是否展开（默认收起露 4 行）。 */
  open?: boolean;
}

@Component({
  selector: 'ngx-agent-panel',
  templateUrl: './agent.component.html',
  styleUrls: ['./agent.component.scss'],
  standalone: true,
  imports: [
    TranslatePipe,
    CommonModule,
    FormsModule,
    NbCardModule,
    NbButtonModule,
    NbIconModule,
    NbInputModule,
    NbSpinnerModule,
    NbListModule,
      NbTooltipModule,
    MarkdownModule,
    AiIllustrationComponent,
  ],
})
export class AgentComponent implements OnInit, OnDestroy {
  private route = inject(ActivatedRoute)
  private i18n = inject(I18nService);
  private agentService = inject(AgentService);
  private chatService = inject(AgentChatService);
  private confirmDialog = inject(ConfirmDialogService);
  private cdRef = inject(ChangeDetectorRef);
  private clusterContext = inject(ClusterContextService);
  private toastr = inject(NbToastrService);
  private host = inject(ElementRef);

  cluster: Cluster | null = null;

  sessions: AgentSession[] = [];
  activeSessionId: number | null = null;
  messages: ViewMessage[] = [];

  input = '';
  loadingSessions = false;
  sending = false;
  /** 距底部较远时露出的回到底部按钮。 */
  showBackToBottom = false;

  /** 消息滚动容器（host 内查询，不依赖 ViewChild 时序）。 */
  private messagePaneEl(): HTMLElement | null {
    return this.host.nativeElement.querySelector('.agent-messages');
  }
  private transcriptRequest?: Subscription;
  private transcriptLoadId = 0;
  private readonly transcriptCache = new Map<number, ViewMessage[]>();
  private streamRenderFrame: number | null = null;
  private scrollFrame: number | null = null;
  private scrollForce = false;
  private readonly minTranscriptLoadingMs = 180;

  ngOnInit(): void {
    // 通知直达：/pages/cluster-ops/agent?session=<id> 自动打开对应会话
    this.route.queryParamMap.subscribe((q) => {
      const sid = q.get('session');
      if (sid && /^\d+$/.test(sid)) {
        this.openSession(Number(sid));
      }
    });

    // 前台状态上报（切走页面 → 回合完成后触发铃铛通知）
    this.chatService.setUiFront(true);

    // 当前回合广播订阅（回合由全局服务持有，页面销毁不断连）
    this.chatService.events().subscribe((ev) => {
      if (!this.turnMessage) {
        return;
      }
      this.applyStreamEvent(this.turnMessage, ev);
    });

    this.clusterContext.activeCluster$.subscribe((c) => {
      this.cluster = c;
      this.reloadSessions();
    });
  }

  ngOnDestroy(): void {
    this.chatService.setUiFront(false);
    this.transcriptRequest?.unsubscribe();
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
    }
    if (this.scrollFrame !== null) {
      cancelAnimationFrame(this.scrollFrame);
    }
  }

  /**
   * 加载指定会话的完整消息（通知直达 / 会话切换共用）。
   *
   * 首次远端加载保留至少一帧可感知的 loading（最短 180ms），避免服务端响应很快时
   * 浏览器把“写入 loading 状态”和“渲染完整 Markdown”合并到同一帧，用户只能看到卡顿。
   * 已访问会话走本地转录缓存，切换时不再等待网络。
   */
  private loadTranscript(sessionId: number): void {
    const cached = this.transcriptCache.get(sessionId);
    if (cached) {
      this.messages = this.cloneTranscript(cached);
      this.loadingTranscript = false;
      this.cdRef.detectChanges();
      this.scrollToBottom(true);
      return;
    }

    const loadId = ++this.transcriptLoadId;
    const startedAt = performance.now();
    this.transcriptRequest?.unsubscribe();
    this.loadingTranscript = true;
    this.cdRef.detectChanges();
    this.transcriptRequest = this.agentService.getSession(sessionId).subscribe({
      next: (msgs) => {
        const transcript = msgs.map((m) => ({ id: m.id, role: m.role, content: m.content, steps: m.steps, feedback: m.feedback ?? null }));
        const remaining = Math.max(0, this.minTranscriptLoadingMs - (performance.now() - startedAt));
        window.setTimeout(() => {
          if (loadId !== this.transcriptLoadId || sessionId !== this.activeSessionId) {
            return;
          }
          this.messages = transcript;
          this.transcriptCache.set(sessionId, this.cloneTranscript(transcript));
          this.loadingTranscript = false;
          this.cdRef.detectChanges();
          this.scrollToBottom(true);
        }, remaining);
      },
      error: (err) => {
        if (loadId !== this.transcriptLoadId || sessionId !== this.activeSessionId) {
          return;
        }
        this.loadingTranscript = false;
        const msg = err?.error?.message ?? err?.message ?? '加载会话失败';
        this.toastr.danger(msg, '切换会话');
        this.cdRef.detectChanges();
      },
    });
  }

  reloadSessions(): void {
    if (!this.cluster) {
      return;
    }
    this.loadingSessions = true;
    this.agentService.listSessions(this.cluster.id).subscribe({
      next: (s) => {
        this.sessions = s;
        // Re-select the previously open session if it still exists.
        if (this.activeSessionId && !s.some((x) => x.id === this.activeSessionId)) {
          this.activeSessionId = null;
          this.messages = [];
        }
        this.loadingSessions = false;
        // NG0100：订阅回调发生在变更检测之后，需手动触发本轮检测
        this.cdRef.detectChanges();
      },
      error: () => {
        this.loadingSessions = false;
        this.cdRef.detectChanges();
      },
    });
  }

  openSession(sessionId: number): void {
    // 已展示完整内容的当前会话无需重复请求；空转录仍允许点击重试。
    if (this.activeSessionId === sessionId && (this.loadingTranscript || this.messages.length > 0)) {
      return;
    }
    this.saveDraft();
    this.activeSessionId = sessionId;
    this.pendingActions = [];
    this.restoreDraft();
    this.loadTranscript(sessionId);
  }

  newSession(): void {
    this.saveDraft();
    this.activeSessionId = null;
    this.messages = [];
    this.restoreDraft();
  }

  /** 草稿按会话保留（切换会话不丢已打的字，GPT 同款）。 */
  private drafts = new Map<string, string>();

  private draftKey(): string {
    return this.activeSessionId === null ? 'new' : `s${this.activeSessionId}`;
  }

  private saveDraft(): void {
    this.drafts.set(this.draftKey(), this.input);
  }

  private restoreDraft(): void {
    this.input = this.drafts.get(this.draftKey()) ?? '';
    this.resetInputHeight();
  }

  /** 会话搜索（纯前端过滤）。 */
  sessionQuery = '';

  filteredSessions(): AgentSession[] {
    const q = String(this.sessionQuery ?? '').trim().toLowerCase();
    if (!q) {
      return this.sessions;
    }
    return this.sessions.filter((s) => (s.title || '').toLowerCase().includes(q));
  }

  /** 会话重命名（GPT 同款行内编辑）。 */
  renamingId: number | null = null;
  renameDraft = '';

  startRename(s: AgentSession, event: Event): void {
    event.stopPropagation();
    this.renamingId = s.id;
    this.renameDraft = s.title || '';
  }

  cancelRename(): void {
    this.renamingId = null;
  }

  saveRename(s: AgentSession): void {
    const title = this.renameDraft.trim();
    if (!title) {
      this.renamingId = null;
      return;
    }
    this.agentService.renameSession(s.id, title).subscribe({
      next: (r) => {
        s.title = r.title;
        this.renamingId = null;
        this.cdRef.detectChanges();
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '重命名失败', '智能运维助手'),
    });
  }

  /** 换一种思路再答一次（以前文提问重发一轮，历史保留可对照）。 */
  regenerate(m: ViewMessage): void {
    if (this.sending || this.chatService.isRunning()) {
      return;
    }
    const idx = this.messages.indexOf(m);
    for (let i = idx - 1; i >= 0; i--) {
      if (this.messages[i].role === 'user') {
        this.input = this.messages[i].content;
        this.send();
        return;
      }
    }
  }

  /** 导出会话为 Markdown（运维复盘/归档用，纯前端生成）。 */
  exportMarkdown(): void {
    if (!this.messages.length) {
      return;
    }
    const title = this.sessions.find((s) => s.id === this.activeSessionId)?.title || '智能运维会话';
    const lines: string[] = [`# ${title}`, ''];
    for (const m of this.messages) {
      if (m.role === 'user') {
        lines.push('## 用户', '', m.content, '');
      } else if (!m.content.startsWith('⚠️')) {
        lines.push('## 助手', '', m.content, '');
        const tools = this.timeline(m)
          .filter((t) => t.kind === 'tool')
          .map((t) => t.title);
        if (tools.length) {
          lines.push(`> 取证工具：${tools.join('、')}`, '');
        }
      }
    }
    const blob = new Blob([lines.join('\n')], { type: 'text/markdown;charset=utf-8' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `ops-chat-${this.activeSessionId ?? 'new'}.md`;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  deleteSession(sessionId: number, event: Event): void {
    event.stopPropagation();
    const item = this.sessions.find((x) => x.id === sessionId);
    this.confirmDialog
      .confirmDelete(item?.title || '该会话', '会话中的全部消息将被永久删除，不可恢复。')
      .subscribe((yes) => {
        if (!yes) {
          return;
        }
        this.doDeleteSession(sessionId);
      });
  }

  private doDeleteSession(sessionId: number): void {
    this.agentService.deleteSession(sessionId).subscribe({
      next: () => {
        if (this.activeSessionId === sessionId) {
          this.newSession();
        }
        this.reloadSessions();
      },
      error: (err) => {
        // 展示后端具体原因（如"会话不存在"= 陈旧列表项），并强制刷新列表消除陈旧项
        const msg = err?.error?.message ?? err?.message ?? '请求失败';
        this.toastr.danger(`删除会话失败：${msg}`, '智能运维助手');
        this.reloadSessions();
      },
    });
  }

  private turnMessage: ViewMessage | null = null;
  /** 待确认的对话内动作（确认卡列表，渲染在消息流底部）。 */
  pendingActions: ChatActionRequest[] = [];
  /** 切换会话加载中。 */
  loadingTranscript = false;

  /** 把广播回合事件应用到当前回合占位气泡（Flink 同款 token 直达气泡）。 */
  private applyStreamEvent(assistant: ViewMessage, ev: ChatStreamEvent): void {
    if (ev.type === 'delta' && ev.text) {
      assistant.liveText = (assistant.liveText ?? '') + ev.text;
      this.scheduleStreamRender();
    } else if (ev.type === 'phase') {
      assistant.phase = ev.phase as 'reasoning' | 'answer';
      if (assistant.phase === 'reasoning') {
        // 工具回合的草稿已转存到后端 reasoning step；清空临时气泡，等待工具链。
        assistant.liveText = '';
      }
      this.scheduleStreamRender();
    } else if (ev.type === 'step' && ev.step) {
      assistant.steps = [...assistant.steps, ev.step];
      this.scheduleStreamRender();
    } else if (ev.type === 'answer') {
      assistant.content = ev.final_answer ?? assistant.liveText ?? '';
      assistant.liveText = assistant.content;
      this.scheduleStreamRender();
    } else if (ev.type === 'action_request' && ev.action) {
      // 对话内动作申请确认卡：去重后展示
      if (!this.pendingActions.some((a) => a.id === ev.action!.id)) {
        this.pendingActions.push(ev.action!);
      }
      this.scrollToBottom(true);
    } else if (ev.type === 'done') {
      const visibleTurn = this.messages.includes(assistant);
      if (visibleTurn) {
        this.activeSessionId = ev.session_id ?? this.activeSessionId;
      }
      assistant.content = assistant.content || assistant.liveText || '';
      assistant.liveText = undefined;
      assistant.streaming = false;
      this.sending = false;
      if (visibleTurn && this.activeSessionId) {
        this.transcriptCache.set(this.activeSessionId, this.cloneTranscript(this.messages));
      }
      this.turnMessage = null;
      this.reloadSessions();
      this.renderNow();
    } else if (ev.type === 'error') {
      assistant.content = `⚠️ ${ev.message ?? '诊断失败'}`;
      assistant.liveText = undefined;
      assistant.streaming = false;
      this.sending = false;
      this.turnMessage = null;
      this.renderNow();
    }
  }

  send(): void {
    const text = this.input.trim();
    if (!text || this.sending || !this.cluster || this.chatService.isRunning()) {
      return;
    }
    this.sending = true;
    this.messages.push({ role: 'user', content: text, steps: [] });
    this.input = '';
    this.resetInputHeight();
    // 发送瞬间强制到底：先同步渲染新消息再量 scrollHeight，否则读到的是旧高度。
    this.cdRef.detectChanges();
    this.scrollToBottom(true);

    // Streaming turn: placeholder bubble fills in as steps/answer arrive.
    const assistant: ViewMessage = { role: 'assistant', content: '', steps: [], liveText: '', streaming: true };
    this.messages.push(assistant);
    this.turnMessage = assistant;

    // 回合交全局通道：切走页面（组件销毁）不打断，完成后由服务触发铃铛通知。
    // 附带页面上下文（sxdevops 页面 Copilot）：模型感知当前页面/参数。
    this.chatService.start({
      session_id: this.activeSessionId ?? undefined,
      cluster_id: this.cluster.id,
      message: text,
      context: {
        page: 'agent',
        params: { session: this.activeSessionId ? String(this.activeSessionId) : '' },
      },
    });
  }

  /** 工具英文名 → 中文（trace 里不再出现裸英文调用名）。 */
  toolLabel(name: string): string {
    const map: Record<string, string> = {
      query_metrics: '查询集群指标',
      query_nodes: '查询节点状态',
      query_running_queries: '查询运行中查询',
      query_slow_queries: '查询慢查询',
      query_profile_diagnostics: '分析查询 Profile',
      query_variables: '查询集群变量',
      query_capacity_forecast: '预测容量趋势',
      query_explain: '查看执行计划',
      propose_action: '申请运维动作',
      search_web: '联网搜索',
      fetch_doc: '查阅官方文档',
    };
    return map[name] ?? name;
  }

  /**
   * 一轮会话的时间线（带缓存）：思考与工具调用按发生顺序交错排列，
   * 每次工具调用与其结果合并为一行（调用中则标记执行中）。
   *
   * 模板多次调用 + 流式期间高频变更检测，必须复用实例，
   * 否则展开状态（t.open）会随重建丢失（点开即被下一个 token 收起）。
   */
  private readonly timelineCache = new WeakMap<ViewMessage, { len: number; items: TraceItem[] }>();

  timeline(m: ViewMessage): TraceItem[] {
    const len = m.steps?.length ?? 0;
    const cached = this.timelineCache.get(m);
    if (cached && cached.len === len) {
      return cached.items;
    }
    const items = this.buildTimeline(m.steps);
    this.timelineCache.set(m, { len, items });
    return items;
  }

  private buildTimeline(steps: AgentMessage['steps']): TraceItem[] {
    const items: TraceItem[] = [];
    const list = steps ?? [];
    for (let i = 0; i < list.length; i++) {
      const s = list[i];
      if (s.kind === 'end') {
        continue;
      }
      if (s.kind === 'reasoning') {
        const text = (s.detail || '').trim();
        if (text) {
          items.push({ kind: 'thinking', title: this.i18n.instant('思考'), detail: text });
        }
        continue;
      }
      if (s.kind === 'tool') {
        const next = list[i + 1];
        const paired = next && next.kind === 'tool_result' && next.label === s.label ? next : undefined;
        if (paired) {
          i++;
        }
        items.push({
          kind: 'tool',
          title: this.toolLabel(s.label),
          durationMs: paired?.duration_ms,
          ok: paired ? paired.status !== 'error' : undefined,
          detail: paired?.result || undefined,
        });
        continue;
      }
      if (s.kind === 'tool_result') {
        items.push({
          kind: 'tool',
          title: this.toolLabel(s.label),
          durationMs: s.duration_ms,
          ok: s.status !== 'error',
          detail: s.result || undefined,
        });
        continue;
      }
      items.push({
        kind: 'error',
        title: s.label || '错误',
        detail: s.result || s.detail || '',
      });
    }
    return items;
  }

  /** 详情超过约 4 行（≈200 字符）才允许点击展开，短文本点击无意义。 */
  isLong(text?: string): boolean {
    return !!text && text.length > 200;
  }

  /** 时间线项图标（Eva outline，已在 eva-icons/outline-icons.json 校验存在）。 */
  traceIcon(t: TraceItem): string {
    if (t.kind === 'thinking') {
      return 'bulb-outline';
    }
    if (t.kind === 'error' || t.ok === false) {
      return 'alert-triangle-outline';
    }
    return t.ok === undefined ? 'loader-outline' : 'flash-outline';
  }

  /** 复制回答正文（GPT 同款 hover 操作）。 */
  copyMessage(m: ViewMessage): void {
    const text = m.content || m.liveText || '';
    if (!text || text.startsWith('⚠️')) {
      return;
    }
    const done = () => {
      m.copied = true;
      this.cdRef.detectChanges();
      setTimeout(() => {
        m.copied = false;
        this.cdRef.detectChanges();
      }, 1500);
    };
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(() => this.legacyCopy(text, done));
    } else {
      this.legacyCopy(text, done);
    }
  }

  private legacyCopy(text: string, done: () => void): void {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try {
      if (document.execCommand('copy')) {
        done();
      }
    } finally {
      document.body.removeChild(ta);
    }
  }

  trackMessage(_index: number, message: ViewMessage): number | ViewMessage {
    return message.id ?? message;
  }

  private cloneTranscript(messages: ViewMessage[]): ViewMessage[] {
    return messages.map((message) => ({ ...message, steps: [...message.steps] }));
  }

  /** 流式回调可能不在 Angular Zone；每帧最多做一次检测，既稳定又不阻塞主线程。 */
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

  private renderNow(): void {
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
      this.streamRenderFrame = null;
    }
    this.cdRef.detectChanges();
    this.scrollToBottom();
  }

  /** 消息区滚动：远离底部时露出回到底部按钮。 */
  onMessagesScroll(): void {
    const el = this.messagePaneEl();
    if (!el) {
      return;
    }
    const show = el.scrollHeight - el.scrollTop - el.clientHeight > 300;
    if (show !== this.showBackToBottom) {
      this.showBackToBottom = show;
      this.cdRef.detectChanges();
    }
  }

  /** 智能跟随滚动：距底部 <120px 时自动滚到底（用户上翻历史时不打扰）。
   * 同步先跳一次（不依赖 rAF，极端节流下也不卡死），再用双 rAF 等
   * markdown/代码块异步展开后补一次。 */
  private scrollToBottom(force = false): void {
    this.scrollForce ||= force;
    const instant = this.messagePaneEl();
    if (instant && (this.scrollForce || instant.scrollHeight - instant.scrollTop - instant.clientHeight < 120)) {
      instant.scrollTop = instant.scrollHeight;
    }
    if (this.scrollFrame !== null) {
      return;
    }
    this.scrollFrame = requestAnimationFrame(() => {
      // 再等一帧：markdown/代码块在 CD 后异步展开，第一帧量到的是旧高度。
      requestAnimationFrame(() => {
        const el = this.messagePaneEl();
        this.scrollFrame = null;
        this.scrollForce = false;
        if (!el) {
          return;
        }
        const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
        if (force || nearBottom) {
          el.scrollTop = el.scrollHeight;
        }
        if (this.showBackToBottom && el.scrollHeight - el.scrollTop - el.clientHeight < 120) {
          this.showBackToBottom = false;
          this.cdRef.detectChanges();
        }
      });
    });
  }

  get presets(): string[] {
    return PRESET_QUESTIONS.map(q => this.i18n.instant(q));
  }

  confirmActionCard(action: ChatActionRequest): void {
    this.agentService.confirmChatAction(action.id).subscribe({
      next: (r) => {
        const a = r.action;
        this.toastr.success(
          a.status === 'executed' ? a.result_json ?? '执行成功' : a.result_json ?? '执行失败',
          '动作执行',
        );
        this.pendingActions = this.pendingActions.filter((x) => x.id !== action.id);
        // 执行结果已由后端写入会话消息，刷新转录
        if (this.activeSessionId) {
          this.transcriptCache.delete(this.activeSessionId);
          this.loadTranscript(this.activeSessionId);
        }
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '确认失败', '动作执行'),
    });
  }

  rejectActionCard(action: ChatActionRequest): void {
    this.agentService.cancelChatAction(action.id).subscribe({
      next: () => {
        this.toastr.success(this.i18n.instant('已拒绝该动作'), this.i18n.instant('动作执行'));
        this.pendingActions = this.pendingActions.filter((x) => x.id !== action.id);
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '取消失败', '动作执行'),
    });
  }

  askPreset(q: string): void {
    this.input = q;
    this.send();
  }

  /** 回车发送 / Shift+回车换行（中文输入法组词中不触发）。 */
  onInputKey(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) {
      return;
    }
    if (event.key === 'Enter' && (!event.shiftKey || event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      this.send();
    }
  }

  /** 输入框自适应高度（上限约 8 行）。 */
  autoGrow(event: Event): void {
    const el = event.target as HTMLTextAreaElement | null;
    if (!el) {
      return;
    }
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 160) + 'px';
  }

  private resetInputHeight(): void {
    const el = document.querySelector('.chat-input .input-field') as HTMLTextAreaElement | null;
    if (el) {
      el.style.height = 'auto';
    }
  }

  /** 停止当前回合（GPT 同款）：保留已到文本，打停止标记，可立即追问。 */
  stopTurn(): void {
    if (!this.sending || !this.turnMessage) {
      return;
    }
    this.chatService.stop();
    const assistant = this.turnMessage;
    assistant.content = assistant.content || assistant.liveText || '';
    assistant.liveText = undefined;
    assistant.streaming = false;
    assistant.stopped = true;
    this.sending = false;
    this.turnMessage = null;
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
      this.streamRenderFrame = null;
    }
    if (this.activeSessionId) {
      this.transcriptCache.set(this.activeSessionId, this.cloneTranscript(this.messages));
    }
    this.cdRef.detectChanges();
    this.scrollToBottom(true);
  }

  /** 回答点赞/点踩（再点一次取消，切换式）。 */
  rate(m: ViewMessage, value: 'up' | 'down'): void {
    if (!m.id || m.streaming) {
      return;
    }
    const next = m.feedback === value ? null : value;
    this.agentService.setMessageFeedback(m.id, next).subscribe({
      next: (r) => {
        m.feedback = r.feedback;
        if (this.activeSessionId) {
          this.transcriptCache.set(this.activeSessionId, this.cloneTranscript(this.messages));
        }
        this.cdRef.detectChanges();
      },
      error: (e) => this.toastr.danger(e?.error?.message ?? '评价失败', '智能运维助手'),
    });
  }

  /** 重试：把失败消息重新发送一次（用户点击错误气泡的「重试」）。 */
  retryMessage(m: ViewMessage): void {
    const msgText = m.content.replace(/^⚠️\s*/, '');
    // 找到最近的 user 消息作为重试内容
    const userMsg = [...this.messages].reverse().find((x) => x.role === 'user');
    if (!userMsg) {
      return;
    }
    // 移除失败回合（assistant + 其 thinking），重新发送原问题
    const failedIdx = this.messages.indexOf(m);
    if (failedIdx > 0 && this.messages[failedIdx - 1].role === 'user') {
      this.messages.splice(failedIdx, 1);
    }
    this.input = userMsg.content;
    this.send();
  }
}
