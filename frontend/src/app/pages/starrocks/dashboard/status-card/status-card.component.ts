import { Component, Input } from '@angular/core';
import { NbCardModule } from '@nebular/theme';

@Component({
    selector: 'ngx-status-card',
    styleUrls: ['./status-card.component.scss'],
    template: `
    <nb-card>
      <div class="icon-container">
        <div class="icon status-{{ type }}">
          <ng-content></ng-content>
        </div>
      </div>
      <div class="details">
        <div class="title h5">{{ value }}</div>
        <div class="status paragraph-2">{{ title }}</div>
      </div>
    </nb-card>
  `,
    imports: [NbCardModule],
})
export class StatusCardComponent {
  @Input() title: string;
  @Input() value: string;
  @Input() type: string;
}
