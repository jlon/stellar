import { Component, EventEmitter, Input, Output } from '@angular/core';

import { NbButtonModule, NbIconModule, NbSelectModule, NbOptionModule } from '@nebular/theme';
import { TranslatePipe } from '@ngx-translate/core';

/**
 * 服务端分页条（视觉对齐 angular2-smart-table 内置 pager）。
 * smart-table 内置 pager 是纯前端分页，服务端分页页面无法使用，统一用本组件。
 * 全部颜色/字号/圆角走 smart-table-paging-* 主题变量，与内置 pager 观感一致。
 */
@Component({
  selector: 'ngx-table-pagination',
  standalone: true,
  imports: [NbButtonModule, NbIconModule, NbSelectModule, NbOptionModule, TranslatePipe],
  template: `
    <div class="ngx-table-pagination">
      <div class="ngx-table-pagination__info">
        <span class="ngx-table-pagination__summary">
          @if (isEmpty) {
            {{ '暂无数据' | translate }}
          } @else {
            {{ '显示' | translate }} {{ rangeStart }} - {{ rangeEnd }} {{ '条，共' | translate }} {{ total }} {{ '条' | translate }}
          }
        </span>
        @if (pageSizeOptions.length > 1) {
          <nb-select
            class="ngx-table-pagination__page-size"
            [selected]="pageSize"
            size="small"
            (selectedChange)="onPageSizeChange($event)"
            aria-label="每页条数">
            @for (size of pageSizeOptions; track size) {
              <nb-option [value]="size">{{ size }} {{ '条/页' | translate }}</nb-option>
            }
          </nb-select>
        }
      </div>

      <ul class="ngx-table-pagination__pager">
        <li>
          <a href="#" class="page-link" role="button"
             [class.disabled]="page === 1"
             [attr.aria-disabled]="page === 1"
             (click)="go($event, 1)"
             [title]="'首页' | translate">
            <nb-icon icon="chevron-left-outline"></nb-icon><nb-icon icon="chevron-left-outline"></nb-icon>
          </a>
        </li>
        <li>
          <a href="#" class="page-link" role="button"
             [class.disabled]="page === 1"
             [attr.aria-disabled]="page === 1"
             (click)="go($event, page - 1)"
             [title]="'上一页' | translate">
            <nb-icon icon="chevron-left-outline"></nb-icon>
          </a>
        </li>
        <li class="page-item-info"><span>{{ '第' | translate }} {{ page }} / {{ totalPages }} {{ '页' | translate }}</span></li>
        <li>
          <a href="#" class="page-link" role="button"
             [class.disabled]="page === totalPages"
             [attr.aria-disabled]="page === totalPages"
             (click)="go($event, page + 1)"
             [title]="'下一页' | translate">
            <nb-icon icon="chevron-right-outline"></nb-icon>
          </a>
        </li>
        <li>
          <a href="#" class="page-link" role="button"
             [class.disabled]="page === totalPages"
             [attr.aria-disabled]="page === totalPages"
             (click)="go($event, totalPages)"
             [title]="'末页' | translate">
            <nb-icon icon="chevron-right-outline"></nb-icon><nb-icon icon="chevron-right-outline"></nb-icon>
          </a>
        </li>
      </ul>
    </div>
  `,
})
export class TablePaginationComponent {
  /** 当前页（1 起） */
  @Input() page = 1;
  /** 总条数 */
  @Input() total = 0;
  /** 每页条数 */
  @Input() pageSize = 10;
  /** 页大小可选项；单值时不渲染选择器 */
  @Input() pageSizeOptions: number[] = [10];

  @Output() pageChange = new EventEmitter<number>();
  @Output() pageSizeChange = new EventEmitter<number>();

  get totalPages(): number {
    return Math.max(1, Math.ceil(this.total / this.pageSize));
  }

  get rangeStart(): number {
    return this.total === 0 ? 0 : (this.page - 1) * this.pageSize + 1;
  }

  get rangeEnd(): number {
    return Math.min(this.page * this.pageSize, this.total);
  }

  get isEmpty(): boolean {
    return this.total === 0;
  }

  go(event: Event, target: number): void {
    event.preventDefault();
    if (target < 1 || target > this.totalPages || target === this.page) {
      return;
    }
    this.page = target;
    this.pageChange.emit(target);
  }

  onPageSizeChange(size: number): void {
    this.pageSize = size;
    this.pageSizeChange.emit(size);
  }
}
