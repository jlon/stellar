import { Component, Input, Output, EventEmitter, OnChanges, OnInit, SimpleChanges } from '@angular/core';
import { TranslatePipe } from '@ngx-translate/core';

import { NbButtonModule, NbIconModule } from '@nebular/theme';

@Component({
    selector: 'ngx-active-toggle-render',
    template: `
    <div class="mv-state">
      <span [class]="isActive ? 'badge badge-success' : 'text-hint'">
        {{ (isActive ? '已激活' : '已停用') | translate }}
      </span>
      @if (!isRollup) {
        <button nbButton ghost status="basic" size="tiny" type="button"
          (click)="onToggle($event)" [title]="(isActive ? '停用' : '激活') | translate"
          [attr.aria-label]="(isActive ? '停用' : '激活') | translate">
          <nb-icon icon="power-outline"></nb-icon>
        </button>
      }
    </div>
    `,
    styles: [`
      .mv-state { display: flex; align-items: center; gap: 0.35rem; white-space: nowrap; }
      .mv-state button { color: var(--text-hint-color); }
      .mv-state button:hover, .mv-state button:focus-visible { color: var(--text-basic-color); }
    `],
    imports: [NbButtonModule, NbIconModule, TranslatePipe]
})
export class ActiveToggleRenderComponent implements OnInit, OnChanges {
  @Input() value: unknown;
  @Input() rowData: unknown;
  @Output() toggleActive: EventEmitter<any> = new EventEmitter();

  isActive = false;
  isRollup = false;

  ngOnInit(): void {
    this.updateState();
  }

  ngOnChanges(changes: SimpleChanges): void {
    if (changes['rowData'] || changes['value']) {
      this.updateState();
    }
  }

  setCell(value: unknown, rowData: unknown): void {
    this.value = value;
    this.rowData = rowData;
    this.updateState();
  }

  onToggle(event: MouseEvent) {
    event.stopPropagation();
    if (this.rowData) {
      this.toggleActive.emit(this.rowData);
    }
  }

  private updateState(): void {
    const state = typeof this.value === 'string' ? this.value.trim().toLowerCase() : this.value;
    const row = this.rowData as { kind?: unknown } | null;
    this.isRollup = row?.kind === 'rollup';
    this.isActive = this.isRollup || state === true || state === 1 || state === 'true' || state === '1';
  }
}
