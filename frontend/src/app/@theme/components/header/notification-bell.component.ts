//! 右上角铃铛通知（纯前端壳 → 完整最小体系）：
//! 轮询未读数（60s），下拉面板展示列表，点击跳链并标记已读。

import { TranslatePipe } from '@ngx-translate/core';
import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { CommonModule } from '@angular/common';
import { Router } from '@angular/router';
import { Subject, interval } from 'rxjs';
import { takeUntil, take } from 'rxjs/operators';
import { NbIconModule, NbPopoverModule } from '@nebular/theme';
import {
  NotificationService,
  NotificationItem,
} from '../../../@core/data/notification.service';

@Component({
  selector: 'ngx-notification-bell',
  templateUrl: './notification-bell.component.html',
  styleUrls: ['./notification-bell.component.scss'],
  standalone: true,
  imports: [
    TranslatePipe,CommonModule, NbIconModule, NbPopoverModule],
})
export class NotificationBellComponent implements OnInit, OnDestroy {
  private notificationService = inject(NotificationService);
  private router = inject(Router);
  private destroy$ = new Subject<void>();

  unreadCount = 0;
  notifications: NotificationItem[] = [];
  private polled = false;

  ngOnInit(): void {
    interval(60_000)
      .pipe(takeUntil(this.destroy$))
      .subscribe(() => this.notificationService.unreadCount().subscribe((c) => (this.unreadCount = c)));
    this.refresh(false);
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /** 打开面板 / 轮询唤醒时刷新列表与未读数。 */
  refresh(userAction: boolean): void {
    this.notificationService.list(false, 30).subscribe({
      next: (v) => {
        this.notifications = v.items;
        this.unreadCount = v.unread_count;
      },
      error: () => {},
    });
    // 首轮（非点击）提前拉一次未读数
    if (!userAction && !this.polled) {
      this.polled = true;
      this.notificationService.unreadCount().subscribe((c) => (this.unreadCount = c));
    }
  }

  openNotification(n: NotificationItem): void {
    if (!n.read) {
      this.notificationService.markRead(n.id).subscribe({
        next: () => {
          n.read = true;
          if (this.unreadCount > 0) {
            this.unreadCount -= 1;
          }
        },
        error: () => {},
      });
    }
    if (n.link) {
      this.router.navigateByUrl(n.link);
    }
  }

  notifIcon(kind: string): string {
    const map: Record<string, string> = {
      agent_chat_done: 'checkmark-circle-2-outline',
      agent_chat_error: 'alert-circle-outline',
      incident_created: 'alert-triangle-outline',
      incident_reopened: 'refresh-outline',
      incident_resolved: 'checkmark-circle-2-outline',
      action_pending: 'flash-outline',
      action_result: 'shield-checkmark-outline',
    };
    return map[kind] ?? 'info-outline';
  }

  notifKind(kind: string): string {
    const map: Record<string, string> = {
      agent_chat_done: '诊断完成',
      agent_chat_error: '诊断失败',
      incident_created: '新 Incident',
      incident_reopened: 'Incident 复开',
      incident_resolved: 'Incident 已恢复',
      action_pending: '动作待确认',
      action_result: '动作结果',
      system: '系统',
    };
    return map[kind] ?? kind;
  }
}
