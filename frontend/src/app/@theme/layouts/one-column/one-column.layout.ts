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

      <nb-sidebar class="assistant-drawer" end fixed state="collapsed" tag="assistant-drawer">
        <ngx-chat-float [drawer]="true"></ngx-chat-float>
      </nb-sidebar>

      <nb-layout-column class="main-column">
        <ngx-tab-bar></ngx-tab-bar>
        <div class="main-column__body">
          <ng-content select="router-outlet"></ng-content>
        </div>

        <!-- 离开全量助手页后的全局会话入口。 -->
        <ngx-chat-float></ngx-chat-float>
      </nb-layout-column>
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
