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
          size="tiny"
          [status]="isActive ? 'warning' : 'success'"
          [outline]="true"
          class="ms-2"
          (click)="onToggle()"
          [title]="isActive ? '停用' : '激活'">
          <nb-icon icon="power-outline"></nb-icon>
        </button>
      }
    </div>
    `,
    styles: [`
    .d-flex {
      display: flex;
      align-items: center;
    }
    .ms-2 {
      margin-left: 0.5rem;
    }
    button {
      padding: 0.25rem 0.5rem;
    }
  `],
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
  
  onToggle() {
    this.toggleActive.emit(this.rowData);
  }
}

