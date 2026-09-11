import { Component, EventEmitter, Input, Output } from '@angular/core';

import { UserWithRoles } from '../../../../@core/data/user.service';

import { NbButtonModule, NbTooltipModule, NbIconModule } from '@nebular/theme';

@Component({
    selector: 'ngx-users-actions-cell',
    template: `
    <div class="actions-container">
      @if (value?.canEdit) {
        <button
          nbButton
          ghost
          size="tiny"
          status="primary"
          (click)="editUser.emit(rowData)"
          nbTooltip="编辑用户信息"
          nbTooltipPlacement="top"
          class="action-btn"
          >
          <nb-icon icon="edit-2-outline"></nb-icon>
        </button>
      }
    
      @if (value?.canDelete) {
        <button
          nbButton
          ghost
          size="tiny"
          status="danger"
          (click)="deleteUser.emit(rowData)"
          nbTooltip="删除用户"
          nbTooltipPlacement="top"
          class="action-btn"
          >
          <nb-icon icon="trash-2-outline"></nb-icon>
        </button>
      }
    </div>
    `,
    styles: [
        `
      .actions-container {
        display: flex;
        gap: 0.25rem;
        justify-content: center;
        align-items: center;
      }

      .action-btn {
        transition: all 0.2s ease;
      }

      .action-btn:hover:not(:disabled) {
        transform: translateY(-1px);
      }

      .action-btn:active:not(:disabled) {
        transform: translateY(0);
      }
    `,
    ],
    imports: [
    NbButtonModule,
    NbTooltipModule,
    NbIconModule
],
})
export class UsersActionsCellComponent {
  @Input() value: { canEdit: boolean; canDelete: boolean } | null = null;
  @Input() rowData!: UserWithRoles;
  @Output() editUser = new EventEmitter<UserWithRoles>();
  @Output() deleteUser = new EventEmitter<UserWithRoles>();
}
