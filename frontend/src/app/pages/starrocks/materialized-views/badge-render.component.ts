import { Component, Input, OnChanges, OnInit, SimpleChanges } from '@angular/core';

export interface BadgeInfo {
  status: string;
  label: string;
  tooltip?: string;
}

/**
 * Generic status badge cell for angular2-smart-table.
 * Uses the global .badge-* classes defined in @theme/styles/_overrides.scss
 * (based on nb-theme status colors), configured per column via
 * componentInitFunction -> getBadge(value, row) => BadgeInfo | null.
 */
@Component({
  selector: 'ngx-badge-render',
  template: `
    @if (badge) {
      <span [class]="'badge badge-' + badge.status" [title]="badge.tooltip">
        {{ badge.label }}
      </span>
    }
    @if (!badge) {
      <span class="text-hint">-</span>
    }
  `,
})
export class BadgeRenderComponent implements OnInit, OnChanges {
  @Input() value: any;
  @Input() rowData: any;

  getBadge: (value: any, row: any) => BadgeInfo | null = () => null;
  badge: BadgeInfo | null = null;

  ngOnInit(): void {
    this.updateBadge();
  }

  // smart-table 动态建 cell 时 ngOnInit 可能先于 rowData 赋值触发，输入就位后补算一次。
  ngOnChanges(changes: SimpleChanges): void {
    if (changes['rowData'] || changes['value']) {
      this.updateBadge();
    }
  }

  private updateBadge(): void {
    try {
      this.badge = this.rowData ? this.getBadge(this.value, this.rowData) : null;
    } catch {
      this.badge = null;
    }
  }
}