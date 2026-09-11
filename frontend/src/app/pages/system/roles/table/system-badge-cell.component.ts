import { Component, Input } from '@angular/core';
import { NbBadgeModule } from '@nebular/theme';

@Component({
    selector: 'ngx-roles-system-badge-cell',
    template: `
    <nb-badge
      [text]="value ? '是' : '否'"
      [status]="value ? 'warning' : 'basic'"
      size="small"
    ></nb-badge>
  `,
    imports: [NbBadgeModule],
})
export class RolesSystemBadgeCellComponent {
  @Input() value = false;
}


