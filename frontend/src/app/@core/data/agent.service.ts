import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { map } from 'rxjs/operators';
import { ApiService } from './api.service';
import { AuthService } from './auth.service';

// ---- Agent chat types (mirror backend handlers/agent_chat.rs + ops_agent) ----

/** One recorded step of an agent turn (audit chain). */
export interface AgentStep {
  kind: 'reasoning' | 'tool' | 'tool_result' | 'end' | 'error';
  label: string;
  detail: string;
  args?: Record<string, unknown>;
  result?: string;
  duration_ms: number;
  status: 'ok' | 'error' | 'pending';
}

/** Result of one chat turn. */
export interface ChatOutcome {
  session_id: number;
  cluster_id: number;
  steps: AgentStep[];
  final_answer: string;
}

/** Session metadata. */
export interface AgentSession {
  id: number;
  channel: string;
  cluster_id: number;
  title: string;
  created_at: string;
  last_active_at: string;
}

/** One persisted message with its trace. */
export interface AgentMessage {
  id: number;
  role: 'user' | 'assistant';
  content: string;
  steps: AgentStep[];
  /** 当前用户的评价（up/down），转录回显点赞状态用。 */
  feedback?: string | null;
}

/** Incident 的只读闭环视图；详情由后端按组织边界授权。 */
export interface AgentIncident {
  id: number;
  cluster_id: number;
  title: string;
  status: 'open' | 'investigating' | 'resolved' | 'closed';
  created_at: string;
  resolved_at?: string | null;
}

export interface AgentIncidentDetail {
  incident: AgentIncident;
  events: unknown[];
  evidences: unknown[];
  decisions: unknown[];
}

export interface ChatRequest {
  session_id?: number;
  cluster_id?: number;
  message: string;
  /** 受限页面上下文：仅用于定位当前界面，不能承载任意用户内容。 */
  context?: AgentPageContext;
}

export type AgentPageContext =
  | { page: 'agent'; params?: { session?: number } }
  | { page: 'current_route'; params: { route: string } };

/** 对话内动作申请（确认卡）。 */
export interface ChatActionRequest {
  id: number;
  kind: string;
  title: string;
  params: Record<string, unknown>;
  reason?: string;
  expires_at: string;
}

/** One SSE event of `/api/agent/chat/stream`. */
export interface ChatStreamEvent {
  type: 'step' | 'delta' | 'phase' | 'action_request' | 'answer' | 'done' | 'error';
  step?: AgentStep;
  /** Typewriter text chunk; the frontend appends it to the pending thinking bubble. */
  text?: string;
  /** Round-boundary semantics: 'reasoning' | 'answer' (repositions the thinking buffer). */
  phase?: string;
  /** 对话内动作申请确认卡。 */
  action?: ChatActionRequest;
  final_answer?: string;
  session_id?: number;
  usage_tokens?: number;
  message?: string;
}

@Injectable({ providedIn: 'root' })
export class AgentService {
  private api = inject(ApiService);
  private auth = inject(AuthService);

  private readonly basePath = '/agent';

  /** Run one chat turn (creates or continues a session). */
  chat(req: ChatRequest): Observable<ChatOutcome> {
    return this.api.post<ChatOutcome>(`${this.basePath}/chat`, req);
  }

  /**
   * Stream one chat turn over SSE (`step` -> `answer` -> `done`).
   * Uses fetch + ReadableStream with the Bearer header — the token never
   * travels in the URL. Unsubscribing aborts the connection, which the
   * backend cancels the active model/tool request when the connection closes.
   */
  chatStream(req: ChatRequest): Observable<ChatStreamEvent> {
    return new Observable<ChatStreamEvent>((subscriber) => {
      const controller = new AbortController();
      const token = this.auth.token;
      fetch(`${this.api.apiBaseUrl}${this.basePath}/chat/stream`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
        },
        body: JSON.stringify(req),
        signal: controller.signal,
      })
        .then(async (resp) => {
          if (!resp.ok) {
            const body = await resp.json().catch(() => null);
            throw new Error(body?.message ?? `HTTP ${resp.status}`);
          }
          if (!resp.body) {
            throw new Error('响应无内容');
          }
          const reader = resp.body.getReader();
          const decoder = new TextDecoder();
          let buf = '';
          for (;;) {
            const { done, value } = await reader.read();
            if (done) {
              break;
            }
            buf += decoder.decode(value, { stream: true });
            let idx: number;
            while ((idx = buf.indexOf('\n\n')) >= 0) {
              const raw = buf.slice(0, idx);
              buf = buf.slice(idx + 2);
              const ev = parseSseEvent(raw);
              if (ev) {
                subscriber.next(ev);
              }
            }
          }
          subscriber.complete();
        })
        .catch((e) => subscriber.error(e));
      return () => controller.abort();
    });
  }

  /** List sessions of a cluster, newest first. */
  listSessions(clusterId: number): Observable<AgentSession[]> {
    return this.api
      .get<{ items: AgentSession[] }>(`${this.basePath}/sessions?cluster_id=${clusterId}`)
      .pipe(map((v) => v.items));
  }

  /** Full transcript of a session, oldest first. */
  getSession(sessionId: number): Observable<AgentMessage[]> {
    return this.api
      .get<{ items: AgentMessage[] }>(`${this.basePath}/sessions/${sessionId}`)
      .pipe(map((v) => v.items));
  }

  /** 通知深链的 Incident 证据与状态。 */
  getIncident(incidentId: number): Observable<AgentIncidentDetail> {
    return this.api.get<AgentIncidentDetail>(`${this.basePath}/incidents/${incidentId}`);
  }

  /** 按 Incident 归属集群重新执行既有只读取证与规则诊断。 */
  investigateIncident(incidentId: number): Observable<unknown> {
    return this.api.post(`${this.basePath}/incidents/${incidentId}/investigate`, {});
  }

  /** 仅当后台已确认关联事件恢复后，允许人工关闭。 */
  closeIncident(incidentId: number): Observable<{ message: string }> {
    return this.api.post<{ message: string }>(`${this.basePath}/incidents/${incidentId}/close`, {});
  }

  /** 确认对话内动作（单次执行；结果回填会话）。 */
  confirmChatAction(id: number): Observable<{ action: ChatActionView }> {
    return this.api.post<{ action: ChatActionView }>(`${this.basePath}/chat-actions/${id}/confirm`, {});
  }

  /** 拒绝对话内动作。 */
  cancelChatAction(id: number): Observable<{ action: ChatActionView }> {
    return this.api.post<{ action: ChatActionView }>(`${this.basePath}/chat-actions/${id}/cancel`, {});
  }

  /** 会话动作申请列表（审计回放）。 */
  listChatActions(sessionId: number): Observable<ChatActionView[]> {
    return this.api
      .get<{ items: ChatActionView[] }>(`${this.basePath}/chat-actions?session_id=${sessionId}`)
      .pipe(map((v) => v.items));
  }

  /** 回答点赞/点踩（rating null = 取消评价，切换式）。 */
  setMessageFeedback(messageId: number, rating: 'up' | 'down' | null): Observable<{ message_id: number; feedback: string | null }> {
    return this.api.post<{ message_id: number; feedback: string | null }>(`${this.basePath}/messages/${messageId}/feedback`, { rating });
  }

  /** 会话重命名（GPT 同款）。 */
  renameSession(sessionId: number, title: string): Observable<{ id: number; title: string }> {
    return this.api.patch<{ id: number; title: string }>(`${this.basePath}/sessions/${sessionId}`, { title });
  }

  /** Delete a session (and its messages, cascaded). */
  deleteSession(sessionId: number): Observable<void> {
    return this.api.delete<void>(`${this.basePath}/sessions/${sessionId}`);
  }
}

/**
 * Parse one SSE block while preserving the `data:` payload byte-for-byte.
 *
 * SSE represents a payload containing line breaks as multiple `data:` lines;
 * joining those lines without `\n` flattens Markdown tables, headings and code
 * indentation. That was the reason a live response looked like one long line
 * but became correct after loading the persisted transcript.
 */
export function parseSseEvent(rawBlock: string): ChatStreamEvent | null {
  let eventName = 'message';
  const dataLines: string[] = [];
  for (const line of rawBlock.split('\n')) {
    if (line.startsWith(':')) {
      continue; // heartbeat comment
    }
    if (line.startsWith('event:')) {
      eventName = line.slice(6).trim();
    } else if (line.startsWith('data:')) {
      // Per SSE, remove only the optional separator space after `data:`.
      // Do not trim: a model token may itself be a space or code indentation.
      const payload = line.slice(5);
      dataLines.push(payload.startsWith(' ') ? payload.slice(1) : payload);
    }
  }
  const data = dataLines.join('\n');
  if (eventName === 'step' && data) {
    try {
      return { type: 'step', step: JSON.parse(data) as AgentStep };
    } catch {
      return null;
    }
  }
  if (eventName === 'delta' && data) {
    return { type: 'delta', text: data };
  }
  if (eventName === 'action_request' && data) {
    try {
      return { type: 'action_request', action: JSON.parse(data) as ChatActionRequest };
    } catch {
      return null;
    }
  }
  if (eventName === 'phase' && data) {
    try {
      return { type: 'phase', phase: (JSON.parse(data) as { phase: string }).phase };
    } catch {
      return null;
    }
  }
  if (eventName === 'answer' && data) {
    return { type: 'answer', final_answer: data };
  }
  if (eventName === 'done' && data) {
    try {
      return { type: 'done', ...(JSON.parse(data) as object) } as ChatStreamEvent;
    } catch {
      return { type: 'done' };
    }
  }
  if (eventName === 'error' && data) {
    return { type: 'error', message: data };
  }
  return null;
}

/** 对话内动作执行视图（审计回放）。 */
export interface ChatActionView {
  id: number;
  session_id: number;
  kind: string;
  title: string;
  params: Record<string, unknown>;
  reason?: string;
  status: 'pending' | 'executing' | 'executed' | 'failed' | 'cancelled' | 'expired';
  action_uuid: string;
  created_by: string;
  created_at: string;
  expires_at: string;
  confirmed_by?: string;
  confirmed_at?: string;
  executed_at?: string;
  result_json?: string;
}
