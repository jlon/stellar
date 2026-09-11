import { Component } from '@angular/core';
import { NbLayoutModule, NbSidebarModule } from '@nebular/theme';
import { HeaderComponent } from '../../components/header/header.component';
import { TabBarComponent } from '../../components/tab-bar/tab-bar.component';

@Component({
    selector: 'ngx-one-column-layout',
    styleUrls: ['./one-column.layout.scss'],
    template: `
    <nb-layout windowMode>
      <nb-layout-header fixed>
        <ngx-header></ngx-header>
      </nb-layout-header>

      <nb-sidebar class="menu-sidebar" tag="menu-sidebar" responsive>
        <ng-content select="nb-menu"></ng-content>
      </nb-sidebar>

      <nb-layout-column>
        <ngx-tab-bar></ngx-tab-bar>
        <ng-content select="router-outlet"></ng-content>
      </nb-layout-column>
    </nb-layout>
  `,
    imports: [
        NbLayoutModule,
        HeaderComponent,
        NbSidebarModule,
        TabBarComponent,
    ],
})
export class OneColumnLayoutComponent {}
