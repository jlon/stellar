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
  ChatActionView,
} from '../../../@core/data/agent.service';
import { AgentChatService } from '../../../@core/data/agent-chat.service';
import { ConfirmDialogService } from '../../../@core/services/confirm-dialog.service';
import { AiIllustrationComponent } from '../../../@theme/components/ai-illustration/ai-illustration.component';
import { ClusterContextService } from '../../../@core/data/cluster-context.service';
import { Cluster } from '../../../@core/data/cluster.service';
import { Subscription } from 'rxjs';

/** 空态引导的预设问题（OLAP 使用者的高频心智：查得慢 / 跑批卡 / 存储问题 / 导入堵）。 */
const PRESET_QUESTIONS = [
  '为什么最近查询变慢了？',
  '这条慢查询到底慢在哪？',
  '磁盘快满了吗，还能撑多久？',
  '现在有导入积压或大查询抢资源吗？',
];

const SHARED_DATA_PRESET_QUESTIONS = [
  '为什么最近查询变慢了？',
  '这条慢查询到底慢在哪？',
  '对象存储或缓存是否影响查询性能？',
  '现在有导入积压或大查询抢资源吗？',
];

interface ViewMessage {
  id?: number;
  role: 'user' | 'assistant';
  content: string;
  steps: AgentMessage['steps'];
  /** 当前 LLM 轮正在抵达的文本，直接以 Markdown 形式实时渲染。 */
  liveText?: string;
  /** 网络流中的未分类文本；确认是最终回答前不渲染，避免暴露模型草稿。 */
  bufferedText?: string;
  /** 已确认的最终回答，用于安全的前端渐进呈现。 */
  answerTarget?: string;
  answerRevealDone?: boolean;
  doneReceived?: boolean;
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
  /** 这条助手消息在流式期间收到的动作申请。 */
  actionIds?: number[];
  /** 证据引用的展开状态。 */
  evidenceOpen?: boolean;
}

/** 诊断活动项：只包含可审计的工具、结果、错误和确定性状态。 */
interface TraceItem {
  kind: 'status' | 'tool' | 'error';
  title: string;
  detail?: string;
  args?: Record<string, unknown>;
  durationMs?: number;
  /** undefined = 仍在执行中。 */
  ok?: boolean;
  /** 参数与长结果独立展开，避免单次点击同时撑开全部信息。 */
  argsOpen?: boolean;
  resultOpen?: boolean;
}

interface TurnStage {
  label: string;
  status: 'done' | 'active' | 'pending';
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
  private host = inject<ElementRef<HTMLElement>>(ElementRef);

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
  private turnEventsSub?: Subscription;
  private transcriptLoadId = 0;
  private readonly transcriptCache = new Map<number, ViewMessage[]>();
  private streamRenderFrame: number | null = null;
  private answerRevealFrame: number | null = null;
  private scrollFrame: number | null = null;
  private scrollForce = false;
  private readonly minTranscriptLoadingMs = 180;
  private readonly actionsById = new Map<number, ChatActionView>();
  private actionsRevision = 0;
  private actionLoadId = 0;
  private actionClock: number | null = null;

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
    this.turnEventsSub = this.chatService.events().subscribe((ev) => {
      if (!this.turnMessage) {
        return;
      }
      this.applyStreamEvent(this.turnMessage, ev);
    });

    // 动作 TTL 只需低频刷新；不以高频计时器干扰流式渲染。
    this.actionClock = window.setInterval(() => {
      if ([...this.actionsById.values()].some((action) => action.status === 'pending')) {
        this.cdRef.detectChanges();
      }
    }, 30_000);

    this.clusterContext.activeCluster$.subscribe((c) => {
      this.cluster = c;
      this.reloadSessions();
    });
  }

  ngOnDestroy(): void {
    this.chatService.setUiFront(false);
    this.transcriptRequest?.unsubscribe();
    this.turnEventsSub?.unsubscribe();
    if (this.streamRenderFrame !== null) {
      cancelAnimationFrame(this.streamRenderFrame);
    }
    if (this.answerRevealFrame !== null) {
      cancelAnimationFrame(this.answerRevealFrame);
    }
    if (this.scrollFrame !== null) {
      cancelAnimationFrame(this.scrollFrame);
    }
    if (this.actionClock !== null) {
      window.clearInterval(this.actionClock);
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
    this.loadActions(sessionId);
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
    this.clearActions();
    this.restoreDraft();
    this.loadTranscript(sessionId);
  }

  newSession(): void {
    this.saveDraft();
    this.activeSessionId = null;
    this.messages = [];
    this.clearActions();
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
  /** 切换会话加载中。 */
  loadingTranscript = false;

  /** 读取持久动作，确保刷新和重新打开会话后仍能审阅每次决定。 */
  private loadActions(sessionId: number): void {
    const loadId = ++this.actionLoadId;
    this.agentService.listChatActions(sessionId).subscribe({
      next: (actions) => {
        if (loadId !== this.actionLoadId || sessionId !== this.activeSessionId) {
          return;
        }
        this.actionsById.clear();
        for (const action of actions) {
          this.actionsById.set(action.id, action);
        }
        this.actionsRevision++;
        this.cdRef.detectChanges();
      },
      // 对话内容仍可正常显示；动作审计接口失败不应遮挡整段转录。
      error: () => undefined,
    });
  }

  private clearActions(): void {
    this.actionLoadId++;
    this.actionsById.clear();
    this.actionsRevision++;
  }

  private upsertAction(action: ChatActionView): void {
    this.actionsById.set(action.id, action);
    this.actionsRevision++;
  }

  private toActionView(action: ChatActionRequest): ChatActionView {
    return {
      ...action,
      session_id: this.activeSessionId ?? 0,
      status: 'pending',
      action_uuid: '',
      created_by: '',
      created_at: '',
    };
  }

  /** 从已持久化的工具结果恢复动作卡，兼容旧会话而无需新增表关联。 */
  private actionRequestFromResult(result?: string): ChatActionRequest | null {
    const prefix = 'ACTION_PENDING:';
    if (!result?.startsWith(prefix)) {
      return null;
    }
    try {
      const action = JSON.parse(result.slice(prefix.length)) as ChatActionRequest;
      return typeof action.id === 'number' && !!action.title ? action : null;
    } catch {
      return null;
    }
  }

  /** 把广播回合事件应用到当前回合占位气泡（Flink 同款 token 直达气泡）。 */
  private applyStreamEvent(assistant: ViewMessage, ev: ChatStreamEvent): void {
    if (ev.type === 'delta' && ev.text) {
      // 在本轮是否调用工具尚不确定前，缓冲模型文本而不直接展示。工具回合的
      // 草稿不属于用户可见的诊断记录；最终回答会在 answer 事件后安全渐进显示。
      assistant.bufferedText = (assistant.bufferedText ?? '') + ev.text;
    } else if (ev.type === 'phase') {
      assistant.phase = ev.phase as 'reasoning' | 'answer';
      if (assistant.phase === 'reasoning') {
        assistant.bufferedText = '';
      }
      this.scheduleStreamRender();
    } else if (ev.type === 'step' && ev.step) {
      assistant.steps = [...assistant.steps, ev.step];
      this.scheduleStreamRender();
    } else if (ev.type === 'answer') {
      this.revealAnswer(assistant, ev.final_answer ?? assistant.bufferedText ?? '');
    } else if (ev.type === 'action_request' && ev.action) {
      const action = this.toActionView(ev.action);
      this.upsertAction(action);
      assistant.actionIds = assistant.actionIds?.includes(action.id)
        ? assistant.actionIds
        : [...(assistant.actionIds ?? []), action.id];
      this.scrollToBottom(true);
    } else if (ev.type === 'done') {
      const visibleTurn = this.messages.includes(assistant);
      if (visibleTurn) {
        this.activeSessionId = ev.session_id ?? this.activeSessionId;
      }
      assistant.doneReceived = true;
      if (!assistant.answerTarget) {
        this.revealAnswer(assistant, assistant.bufferedText ?? '');
      }
      this.finishAnswerIfReady(assistant, visibleTurn);
    } else if (ev.type === 'error') {
      this.cancelAnswerReveal();
      const message = ev.message ?? '诊断失败';
      assistant.content = `⚠️ ${message}`;
      assistant.liveText = undefined;
      assistant.bufferedText = undefined;
      assistant.streaming = false;
      this.sending = false;
      this.turnMessage = null;
      this.toastr.danger(message, this.i18n.instant('智能运维助手'));
      this.renderNow();
    }
  }

  /** 安全的前端渐进呈现：只动画已确认的最终回答，不显示模型工具草稿。 */
  private revealAnswer(assistant: ViewMessage, answer: string): void {
    this.cancelAnswerReveal();
    assistant.answerTarget = answer;
    assistant.answerRevealDone = false;
    assistant.liveText = '';
    assistant.bufferedText = undefined;
    const reduceMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const step = () => {
      const visibleLength = assistant.liveText?.length ?? 0;
      const nextLength = reduceMotion
        ? answer.length
        : Math.min(answer.length, visibleLength + Math.max(4, Math.ceil(answer.length / 150)));
      assistant.liveText = answer.slice(0, nextLength);
      this.scheduleStreamRender();
      if (nextLength < answer.length) {
        this.answerRevealFrame = requestAnimationFrame(step);
        return;
      }
      this.answerRevealFrame = null;
      assistant.answerRevealDone = true;
      this.finishAnswerIfReady(assistant, this.messages.includes(assistant));
    };
    step();
  }

  private cancelAnswerReveal(): void {
    if (this.answerRevealFrame !== null) {
      cancelAnimationFrame(this.answerRevealFrame);
      this.answerRevealFrame = null;
    }
  }

  private finishAnswerIfReady(assistant: ViewMessage, visibleTurn: boolean): void {
    if (!assistant.doneReceived || !assistant.answerRevealDone) {
      return;
    }
    assistant.content = assistant.answerTarget ?? '';
    assistant.liveText = undefined;
    assistant.streaming = false;
    this.sending = false;
    if (visibleTurn && this.activeSessionId) {
      this.transcriptCache.set(this.activeSessionId, this.cloneTranscript(this.messages));
      this.loadActions(this.activeSessionId);
    }
    this.turnMessage = null;
    this.reloadSessions();
    this.renderNow();
  }

  send(): void {
    const text = this.input.trim();
    if (!text || this.sending || !this.cluster) {
      return;
    }
    if (this.chatService.isRunning()) {
      this.toastr.warning(
        this.i18n.instant('正在处理上一条消息，请等待完成或停止当前诊断'),
        this.i18n.instant('智能运维助手'),
      );
      return;
    }
    this.sending = true;
    this.messages.push({ role: 'user', content: text, steps: [] });
    this.input = '';
    this.resetInputHeight();
    // 发送瞬间强制到底：先同步渲染新消息再量 scrollHeight，否则读到的是旧高度。
    this.cdRef.detectChanges();
    this.scrollToBottom(true);

    // 工具回合先显示结构化活动；只有确认是最终回答后才显示文本。
    const assistant: ViewMessage = { role: 'assistant', content: '', steps: [], bufferedText: '', streaming: true };
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
   * 一轮会话的可审计活动（带缓存）。原始 reasoning 文本永不进入用户视图；
   * 工具调用和结果合并成一行，参数和结果分别渐进展开。
   */
  private readonly timelineCache = new WeakMap<ViewMessage, { len: number; items: TraceItem[] }>();
  private readonly actionMessageCache = new WeakMap<ViewMessage, { len: number; revision: number; items: ChatActionView[] }>();

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
        // 兼容历史会话中保留的系统状态；模型自由文本不展示。
        if ((s.detail || '').startsWith('工具执行完成')) {
          items.push({ kind: 'status', title: this.i18n.instant('正在综合已收集的证据') });
        }
        continue;
      }
      if (s.kind === 'tool') {
        const next = list[i + 1];
        const paired = next && next.kind === 'tool_result' && next.label === s.label ? next : undefined;
        if (paired) {
          i++;
        }
        const action = this.actionRequestFromResult(paired?.result);
        items.push({
          kind: 'tool',
          title: this.toolLabel(s.label),
          durationMs: paired?.duration_ms,
          ok: paired ? paired.status !== 'error' : undefined,
          args: s.args,
          detail: action ? undefined : paired?.result || undefined,
        });
        continue;
      }
      if (s.kind === 'tool_result') {
        const action = this.actionRequestFromResult(s.result);
        items.push({
          kind: 'tool',
          title: this.toolLabel(s.label),
          durationMs: s.duration_ms,
          ok: s.status !== 'error',
          detail: action ? undefined : s.result || undefined,
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

  hasArgs(t: TraceItem): boolean {
    return !!t.args && Object.keys(t.args).length > 0;
  }

  formatArgs(args?: Record<string, unknown>): string {
    return JSON.stringify(args ?? {}, null, 2);
  }

  isCodeOutput(text?: string): boolean {
    return !!text && (/^\s*[\[{]/.test(text) || /\b(SELECT|SHOW|EXPLAIN|KILL|SET)\b/i.test(text));
  }

  durationLabel(durationMs?: number): string {
    if (!durationMs || durationMs < 1_000) {
      return durationMs ? `${durationMs}ms` : '';
    }
    const seconds = durationMs / 1_000;
    return `${seconds < 10 ? seconds.toFixed(1) : Math.round(seconds)}s`;
  }

  activityLabel(m: ViewMessage): string {
    const running = this.timeline(m).find((item) => item.kind === 'tool' && item.ok === undefined);
    if (running) {
      return `${this.i18n.instant('正在执行')}：${running.title}`;
    }
    if (m.phase === 'answer') {
      return this.i18n.instant('正在形成诊断结论');
    }
    return this.i18n.instant('正在建立诊断上下文');
  }

  activitySummary(m: ViewMessage): string {
    const items = this.timeline(m);
    const tools = items.filter((item) => item.kind === 'tool');
    const failed = tools.filter((item) => item.ok === false).length;
    const totalMs = tools.reduce((total, item) => total + (item.durationMs ?? 0), 0);
    if (!tools.length) {
      return this.i18n.instant('已完成本轮诊断');
    }
    const duration = this.durationLabel(totalMs);
    const base = `${this.i18n.instant('已完成')} ${tools.length} ${this.i18n.instant('次取证')}`;
    const failedText = failed ? ` · ${failed} ${this.i18n.instant('项失败')}` : '';
    return `${base}${duration ? ` · ${duration}` : ''}${failedText}`;
  }

  turnStages(m: ViewMessage): TurnStage[] {
    const tools = this.timeline(m).filter((item) => item.kind === 'tool');
    const toolsDone = tools.length > 0 && tools.every((item) => item.ok !== undefined);
    const answering = m.phase === 'answer' || !!m.answerTarget;
    return [
      {
        label: this.i18n.instant('收集证据'),
        status: answering || toolsDone ? 'done' : 'active',
      },
      {
        label: this.i18n.instant('分析证据'),
        status: answering ? 'done' : tools.length ? 'active' : 'pending',
      },
      {
        label: this.i18n.instant('形成结论'),
        status: answering ? 'active' : 'pending',
      },
    ];
  }

  evidence(m: ViewMessage): TraceItem[] {
    return this.timeline(m).filter((item) => item.kind === 'tool' && item.ok === true && !!item.detail);
  }

  showEvidence(m: ViewMessage, item: TraceItem): void {
    m.evidenceOpen = true;
    m.traceOpen = true;
    item.resultOpen = true;
    this.cdRef.detectChanges();
    const index = this.timeline(m).indexOf(item);
    if (index < 0) {
      return;
    }
    requestAnimationFrame(() => {
      const target = this.host.nativeElement.querySelector(`#${this.traceAnchorId(m, index)}`) as HTMLElement | null;
      if (!target) {
        return;
      }
      const behavior = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth';
      target.scrollIntoView({ block: 'nearest', behavior });
      target.focus({ preventScroll: true });
    });
  }

  /** 证据列表与诊断活动之间的稳定阅读器锚点。 */
  traceAnchorId(m: ViewMessage, index: number): string {
    return `agent-trace-${m.id ?? this.messages.indexOf(m)}-${index}`;
  }

  evidenceListId(m: ViewMessage): string {
    return `agent-evidence-${m.id ?? this.messages.indexOf(m)}`;
  }

  /** 会话转录与实时 SSE 共用同一套动作卡数据。 */
  messageActions(m: ViewMessage): ChatActionView[] {
    const cached = this.actionMessageCache.get(m);
    const len = m.steps?.length ?? 0;
    if (cached && cached.len === len && cached.revision === this.actionsRevision) {
      return cached.items;
    }
    const ids = new Set<number>(m.actionIds ?? []);
    for (const step of m.steps ?? []) {
      const action = this.actionRequestFromResult(step.result);
      if (action) {
        ids.add(action.id);
        if (!this.actionsById.has(action.id)) {
          this.actionsById.set(action.id, this.toActionView(action));
        }
      }
    }
    const items = [...ids]
      .map((id) => this.actionsById.get(id))
      .filter((action): action is ChatActionView => !!action);
    this.actionMessageCache.set(m, { len, revision: this.actionsRevision, items });
    return items;
  }

  trackAction(_index: number, action: ChatActionView): number {
    return action.id;
  }

  /** 时间线项图标（Eva outline，已在 eva-icons/outline-icons.json 校验存在）。 */
  traceIcon(t: TraceItem): string {
    if (t.kind === 'status') {
      return 'bulb-outline';
    }
    if (t.kind === 'error' || t.ok === false) {
      return 'alert-triangle-outline';
    }
    return t.ok === undefined ? 'loader-outline' : 'flash-outline';
  }

  /** 会话列表遵循 listbox 键盘语义，避免只能用鼠标切换历史记录。 */
  onSessionListKeydown(event: KeyboardEvent): void {
    const sessions = this.filteredSessions();
    if (!sessions.length || !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      return;
    }
    event.preventDefault();
    const activeIndex = Math.max(0, sessions.findIndex((session) => session.id === this.activeSessionId));
    const nextIndex = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? sessions.length - 1
        : (activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + sessions.length) % sessions.length;
    const session = sessions[nextIndex];
    this.openSession(session.id);
    requestAnimationFrame(() => {
      const active = this.host.nativeElement.querySelector('.session-item.active') as HTMLElement | null;
      active?.focus();
    });
  }

  copyTraceResult(text?: string): void {
    if (!text) {
      return;
    }
    const done = () => this.toastr.success(this.i18n.instant('已复制'), this.i18n.instant('诊断证据'));
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(() => this.legacyCopy(text, done));
    } else {
      this.legacyCopy(text, done);
    }
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
    return messages.map((message) => ({
      ...message,
      steps: [...message.steps],
      actionIds: message.actionIds ? [...message.actionIds] : undefined,
    }));
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
    const questions = this.cluster?.deployment_mode === 'shared_data'
      ? SHARED_DATA_PRESET_QUESTIONS
      : PRESET_QUESTIONS;
    return questions.map(q => this.i18n.instant(q));
  }

  get emptyStateHint(): string {
    return this.cluster?.deployment_mode === 'shared_data'
      ? '集群查询变慢 · 存储与缓存性能 · 节点异常 · 导入积压 —— 我先取证，再给有数据依据的结论'
      : '集群查询变慢 · 磁盘告急 · 节点异常 · 导入积压 —— 我先取证，再给有数据依据的结论';
  }

  actionStatus(action: ChatActionView): ChatActionView['status'] {
    return action.status === 'pending' && this.isActionExpired(action) ? 'expired' : action.status;
  }

  actionStatusLabel(action: ChatActionView): string {
    const labels: Record<ChatActionView['status'], string> = {
      pending: this.i18n.instant('等待确认'),
      executing: this.i18n.instant('正在执行'),
      executed: this.i18n.instant('已执行'),
      failed: this.i18n.instant('执行失败'),
      cancelled: this.i18n.instant('已拒绝'),
      expired: this.i18n.instant('已过期'),
    };
    return labels[this.actionStatus(action)];
  }

  actionIcon(action: ChatActionView): string {
    const status = this.actionStatus(action);
    if (status === 'executed') {
      return 'checkmark-circle-2-outline';
    }
    if (status === 'failed' || status === 'cancelled' || status === 'expired') {
      return status === 'cancelled' ? 'close-circle-outline' : 'alert-triangle-outline';
    }
    return status === 'executing' ? 'loader-outline' : 'shield-outline';
  }

  actionCanRespond(action: ChatActionView): boolean {
    return this.actionStatus(action) === 'pending';
  }

  actionPreview(action: ChatActionView): string {
    if (action.kind === 'kill_query') {
      return `KILL QUERY ${String(action.params.query_id ?? '')}`;
    }
    if (action.kind === 'update_variable') {
      const scope = String(action.params.scope ?? 'global').toUpperCase();
      return `SET ${scope} ${String(action.params.key ?? '')} = ${String(action.params.value ?? '')}`;
    }
    return action.title;
  }

  actionScope(action: ChatActionView): string {
    if (action.kind === 'kill_query') {
      return `${this.i18n.instant('目标查询')}：${String(action.params.query_id ?? '-')}`;
    }
    if (action.kind === 'update_variable') {
      return `${this.i18n.instant('集群变量')}：${String(action.params.scope ?? 'global').toUpperCase()}.${String(action.params.key ?? '-')}`;
    }
    return this.i18n.instant('当前集群');
  }

  actionParameters(action: ChatActionView): Array<{ label: string; value: string }> {
    if (action.kind === 'kill_query') {
      return [{ label: 'Query ID', value: String(action.params.query_id ?? '-') }];
    }
    if (action.kind === 'update_variable') {
      return [
        { label: this.i18n.instant('作用域'), value: String(action.params.scope ?? 'global').toUpperCase() },
        { label: this.i18n.instant('变量'), value: String(action.params.key ?? '-') },
        { label: this.i18n.instant('新值'), value: String(action.params.value ?? '-') },
      ];
    }
    return Object.entries(action.params).map(([label, value]) => ({
      label,
      value: typeof value === 'string' ? value : JSON.stringify(value),
    }));
  }

  actionExpiryLabel(action: ChatActionView): string {
    if (this.isActionExpired(action)) {
      return this.i18n.instant('确认窗口已关闭');
    }
    const deadline = this.actionDeadline(action);
    const minutes = Math.max(1, Math.ceil((deadline.getTime() - Date.now()) / 60_000));
    return `${this.i18n.instant('剩余')} ${minutes} ${this.i18n.instant('分钟可确认')}`;
  }

  actionAuditLabel(action: ChatActionView): string {
    const status = this.actionStatus(action);
    if (status === 'executed' || status === 'failed') {
      return action.confirmed_by
        ? this.i18n.instant('确认人：{user}', { user: action.confirmed_by })
        : this.i18n.instant('已记录执行结果');
    }
    if (status === 'cancelled') {
      return this.i18n.instant('该动作未执行');
    }
    return this.actionExpiryLabel(action);
  }

  private actionDeadline(action: ChatActionView): Date {
    const value = action.expires_at.includes('T') ? action.expires_at : action.expires_at.replace(' ', 'T');
    return new Date(value.endsWith('Z') ? value : `${value}Z`);
  }

  private isActionExpired(action: ChatActionView): boolean {
    const deadline = this.actionDeadline(action).getTime();
    return Number.isFinite(deadline) && deadline <= Date.now();
  }

  confirmActionCard(action: ChatActionView): void {
    if (!this.actionCanRespond(action)) {
      return;
    }
    this.upsertAction({ ...action, status: 'executing' });
    this.cdRef.detectChanges();
    this.agentService.confirmChatAction(action.id).subscribe({
      next: (r) => {
        const updated = r.action;
        this.upsertAction(updated);
        this.toastr.success(
          updated.status === 'executed' ? updated.result_json ?? '执行成功' : updated.result_json ?? '执行失败',
          '动作执行',
        );
        // 执行结果已由后端写入会话消息，刷新转录。
        if (this.activeSessionId) {
          this.transcriptCache.delete(this.activeSessionId);
          this.loadTranscript(this.activeSessionId);
        }
      },
      error: (e) => {
        this.upsertAction(action);
        this.toastr.danger(e?.error?.message ?? '确认失败', '动作执行');
      },
    });
  }

  rejectActionCard(action: ChatActionView): void {
    if (!this.actionCanRespond(action)) {
      return;
    }
    this.agentService.cancelChatAction(action.id).subscribe({
      next: (r) => {
        this.upsertAction(r.action);
        this.toastr.success(this.i18n.instant('已拒绝该动作'), this.i18n.instant('动作执行'));
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
    if (event.key === 'Escape' && this.sending) {
      event.preventDefault();
      this.stopTurn();
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
    this.cancelAnswerReveal();
    assistant.content = assistant.content || assistant.liveText || '';
    assistant.liveText = undefined;
    assistant.bufferedText = undefined;
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
