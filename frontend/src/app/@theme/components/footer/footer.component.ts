import { Component } from '@angular/core';

@Component({
  standalone: false,
  selector: 'ngx-footer',
  styleUrls: ['./footer.component.scss'],
  template: `
    <span class="created-by">
      Stellar
    </span>
  `,
})
export class FooterComponent {
}
