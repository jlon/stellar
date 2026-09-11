import { Component, Input } from '@angular/core';

import { NbBadgeModule } from '@nebular/theme';

interface Role {
  id: number;
  name: string;
}

@Component({
    selector: 'ngx-users-role-badge-cell',
    template: `
    @if (value?.length) {
      <div class="d-flex flex-wrap align-items-center">
        @for (role of getDisplayRoles(); track role; let i = $index) {
          <nb-badge
            [text]="role.name"
            [status]="i === 0 ? 'primary' : 'basic'"
            class="mr-1 mb-1"
          ></nb-badge>
        }
        @if (value.length > maxDisplay) {
          <span class="text-hint">
            +{{ value.length - maxDisplay }}
          </span>
        }
      </div>
    } @else {
      <span class="text-hint">-</span>
    }
    `,
    imports: [
    NbBadgeModule
],
})
export class UsersRoleBadgeCellComponent {
  @Input() value: Role[] = [];
  maxDisplay = 2;

  getDisplayRoles(): Role[] {
    return this.value.slice(0, this.maxDisplay);
  }
}
