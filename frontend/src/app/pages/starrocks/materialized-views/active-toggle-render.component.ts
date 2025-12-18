import { Component, Input, Output, EventEmitter, OnInit } from '@angular/core';
import { ViewCell } from 'ng2-smart-table';

@Component({
  selector: 'ngx-active-toggle-render',
  template: `
    <div class="d-flex align-items-center">
      <span *ngIf="isRollup" class="badge badge-success">Active</span>
      <ng-container *ngIf="!isRollup">
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
      </ng-container>
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
  `]
})
export class ActiveToggleRenderComponent implements ViewCell, OnInit {
  @Input() value: string | number;
  @Input() rowData: any;
  @Output() toggle: EventEmitter<any> = new EventEmitter();
  
  isActive: boolean;
  isRollup: boolean;
  
  ngOnInit() {
    // Convert value to boolean
    this.isActive = this.value === 'true' || this.value === 1 || (this.value as any) === true;
    this.isRollup = this.rowData?.refresh_type === 'ROLLUP';
  }
  
  onToggle() {
    this.toggle.emit(this.rowData);
  }
}

