import { Component, Input, Output, EventEmitter, OnInit } from '@angular/core';

import { NbButtonModule, NbIconModule } from '@nebular/theme';

@Component({
    selector: 'ngx-active-toggle-render',
    template: `
    <div class="d-flex align-items-center">
      @if (isRollup) {
        <span class="badge badge-success">Active</span>
      }
      @if (!isRollup) {
        <span [class]="isActive ? 'badge badge-success' : 'badge badge-warning'">
          {{ isActive ? 'Active' : 'Inactive' }}
        </span>
        <button
          nbButton
          ghost
          size="tiny"
          [status]="isActive ? 'warning' : 'success'"
          (click)="onToggle($event)"
          [title]="isActive ? '停用' : '激活'">
          <nb-icon icon="power-outline"></nb-icon>
        </button>
      }
    </div>
    `,
    imports: [NbButtonModule, NbIconModule]
})
export class ActiveToggleRenderComponent implements OnInit {
  @Input() value: string | number;
  @Input() rowData: any;
  @Output() toggleActive: EventEmitter<any> = new EventEmitter();

  isActive: boolean;
  isRollup: boolean;

  ngOnInit() {
    // Convert value to boolean
    this.isActive = this.value === 'true' || this.value === 1 || (this.value as any) === true;
    this.isRollup = this.rowData?.refresh_type === 'ROLLUP';
  }

  onToggle(event: MouseEvent) {
    event.stopPropagation();
    this.toggleActive.emit(this.rowData);
  }
}
