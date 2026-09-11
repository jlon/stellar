/**
 * @license
 * Copyright John. All Rights Reserved.
 * Licensed under the MIT License. See License.txt in the project root for license information.
 */
import { Component, OnInit, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { SeoService } from './@core/utils/seo.service';

@Component({
  standalone: true,
  imports: [RouterOutlet],
  selector: 'ngx-app',
  template: '<router-outlet></router-outlet>',
})
export class AppComponent implements OnInit {
  private seoService = inject(SeoService);


  ngOnInit(): void {
    this.seoService.trackCanonicalChanges();
  }
}
