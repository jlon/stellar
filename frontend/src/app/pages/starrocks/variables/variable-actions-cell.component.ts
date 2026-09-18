import { Component, EventEmitter, Output } from '@angular/core';
import { NbButtonModule, NbIconModule, NbTooltipModule } from '@nebular/theme';
import { Variable } from '../../../@core/data/node.service';

/** 变量表操作列：避免 angular2-smart-table 的内联编辑状态。 */
@Component({
  selector: 'ngx-variable-actions-cell',
  template: `
    <button
      nbButton
      ghost
      size="tiny"
      status="primary"
      nbTooltip="修改变量"
      nbTooltipPlacement="top"
      [attr.aria-label]="'修改变量 ' + variable?.name"
      (click)="editVariable($event)"
    >
      <nb-icon icon="edit-2-outline"></nb-icon>
    </button>
  `,
  imports: [NbButtonModule, NbIconModule, NbTooltipModule],
})
export class VariableActionsCellComponent {
  // angular2-smart-table v3 通过 componentInitFunction 注入，不会自动绑定 @Input。
  variable: Variable | null = null;
  @Output() edit = new EventEmitter<Variable>();

  editVariable(event: Event): void {
    event.stopPropagation();
    if (this.variable) {
      this.edit.emit(this.variable);
    }
  }
}
