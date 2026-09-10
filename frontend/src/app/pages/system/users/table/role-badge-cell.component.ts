import { Component, Input } from '@angular/core';

interface Role {
  id: number;
  name: string;
}

@Component({
  selector: 'ngx-users-role-badge-cell',
  template: `
    <div class="d-flex flex-wrap align-items-center" *ngIf="value?.length; else empty">
      <nb-badge
        *ngFor="let role of getDisplayRoles(); let i = index"
        [text]="role.name"
        [status]="i === 0 ? 'primary' : 'basic'"
        class="mr-1 mb-1"
      ></nb-badge>
      <span class="text-hint" *ngIf="value.length > maxDisplay">
        +{{ value.length - maxDisplay }}
      </span>
    </div>
    <ng-template #empty>
      <span class="text-hint">-</span>
    </ng-template>
  `,
})
export class UsersRoleBadgeCellComponent {
  @Input() value: Role[] = [];
  maxDisplay = 2;

  getDisplayRoles(): Role[] {
    return this.value.slice(0, this.maxDisplay);
  }
}
