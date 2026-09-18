import { Component } from '@angular/core';
import { NbLayoutModule, NbSidebarModule } from '@nebular/theme';
import { HeaderComponent } from '../../components/header/header.component';
import { TabBarComponent } from '../../components/tab-bar/tab-bar.component';
import { ChatFloatComponent } from '../../components/chat-float/chat-float.component';

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

      <nb-layout-column class="main-column">
        <ngx-tab-bar></ngx-tab-bar>
        <div class="main-column__body">
          <ng-content select="router-outlet"></ng-content>
        </div>
      </nb-layout-column>

      <!-- 全局浮动智能运维聊天入口（任意页面右下角） -->
      <ngx-chat-float></ngx-chat-float>
    </nb-layout>
  `,
    imports: [
        NbLayoutModule,
        HeaderComponent,
        ChatFloatComponent,
        NbSidebarModule,
        TabBarComponent,
    ],
})
export class OneColumnLayoutComponent {}
