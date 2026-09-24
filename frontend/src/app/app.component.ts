/**
 * @license
 * Copyright John. All Rights Reserved.
 * Licensed under the MIT License. See License.txt in the project root for license information.
 */
import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { NbThemeService } from '@nebular/theme';
import { Subscription } from 'rxjs';
import { SeoService } from './@core/utils/seo.service';

/**
 * 给 angular2-smart-table 表头同步原生 title，全站表格列名被截断时 hover 可见完整内容。
 * 列名可能渲染在两种结构：
 * - <th> > .angular2-smart-title（部分页面）
 * - <th> > a.angular2-smart-sort-link / span.angular2-smart-sort（模块型页面首次渲染）
 * 统一将 title 挂到 th 上，截断判断也以 th 为准。
 */
const SMART_TABLE_TITLE_OBSERVER = (root: Document | HTMLElement): MutationObserver => {
  const sync = (th: HTMLElement) => {
    const text = th.textContent?.trim();
    if (text) {
      // 不用原生 title（黑框无样式）；CSS 悬浮浮层读 data-th-title。
      th.setAttribute('data-th-title', text);
      th.setAttribute('aria-label', text);
    }
  };
  // 浮层用 position:fixed 时，锚点坐标只能由 JS 写入 CSS 变量（滚动/resize 后重算）。
  const syncPos = (th: HTMLElement) => {
    const r = th.getBoundingClientRect();
    th.style.setProperty('--th-bottom', `${Math.round(r.bottom + 4)}px`);
    th.style.setProperty('--th-center', `${Math.round(r.x + r.width / 2)}px`);
  };
  const syncAll = (th: HTMLElement) => {
    sync(th);
    syncPos(th);
  };
  const scan = (node: Node) => {
    if (!(node instanceof HTMLElement)) return;
    if (node.tagName === 'TH' && (node.classList.contains('angular2-smart-th') || node.querySelector(':scope > angular2-st-column-title, :scope > .angular2-smart-title'))) {
      sync(node);
      return;
    }
    if (node.tagName === 'ANGULAR2-ST-COLUMN-TITLE' || node.classList.contains('angular2-smart-title')) {
      const th = node.closest('th');
      if (th) sync(th as HTMLElement);
    }
    node.querySelectorAll?.('th.angular2-smart-th').forEach((th) => sync(th as HTMLElement));
  };
  const observer = new MutationObserver((mutations) => {
    for (const m of mutations) {
      m.addedNodes.forEach(scan);
      if (m.type === 'characterData') {
        const th = (m.target.parentElement as HTMLElement | null)?.closest('th');
        if (th) sync(th as HTMLElement);
      }
    }
  });
  observer.observe(root, { childList: true, subtree: true, characterData: true });
  // 首次全量扫描（路由复用时表头已存在）
  root.querySelectorAll?.('th.angular2-smart-th').forEach((th) => sync(th as HTMLElement));
  // hover 瞬间实时计算锚点：杜绝侧栏开合、数据加载、字体加载等布局变化导致的陈旧坐标。
  root.addEventListener('mouseover', (e) => {
    const th = (e.target as HTMLElement | null)?.closest?.('th.angular2-smart-th[data-th-title]') as HTMLElement | null;
    if (th) syncPos(th);
  }, { capture: true });
  return observer;
};

@Component({
  standalone: true,
  imports: [RouterOutlet],
  selector: 'ngx-app',
  template: '<router-outlet></router-outlet>',
  host: { '[class]': 'themeClass' },
})
export class AppComponent implements OnInit, OnDestroy {
  private seoService = inject(SeoService);
  private themeService = inject(NbThemeService);
  private titleObserver?: MutationObserver;
  private themeSubscription?: Subscription;
  themeClass = `nb-theme-${this.themeService.currentTheme}`;

  ngOnInit(): void {
    this.seoService.trackCanonicalChanges();
    this.themeSubscription = this.themeService.onThemeChange()
      .subscribe(({ name }) => this.themeClass = `nb-theme-${name}`);
    this.titleObserver = SMART_TABLE_TITLE_OBSERVER(document.body);
  }

  ngOnDestroy(): void {
    this.themeSubscription?.unsubscribe();
    this.titleObserver?.disconnect();
  }
}
