import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { map } from 'rxjs/operators';
import { ApiService } from './api.service';

export interface NotificationItem {
  id: number;
  kind: string;
  title: string;
  body?: string;
  link?: string;
  severity: 'info' | 'warning' | 'critical';
  meta?: Record<string, unknown>;
  read: boolean;
  created_at: string;
}

export interface NotificationList {
  items: NotificationItem[];
  unread_count: number;
}

/** 右上角铃铛通知（聊天完成 / 事件触达等）。 */
@Injectable({ providedIn: 'root' })
export class NotificationService {
  private api = inject(ApiService);

  private readonly basePath = '/notifications';

  list(unreadOnly = false, limit = 50): Observable<NotificationList> {
    return this.api.get<NotificationList>(
      `${this.basePath}?unread_only=${unreadOnly}&limit=${limit}`,
    );
  }

  unreadCount(): Observable<number> {
    return this.list(true, 1).pipe(map((v) => v.unread_count));
  }

  /** 异步任务完成时创建通知（如用户离开会话页后 LLM 诊断完成）。 */
  create(
    kind: string,
    title: string,
    body?: string,
    link?: string,
    severity: 'info' | 'warning' | 'critical' = 'info',
  ): Observable<{ id: number }> {
    return this.api.post<{ id: number }>(this.basePath, { kind, title, body, link, severity });
  }

  markRead(id: number): Observable<{ ok: boolean }> {
    return this.api.post<{ ok: boolean }>(`${this.basePath}/${id}/read`, {});
  }
}