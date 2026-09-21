/**
 * @license
 * Copyright John. All Rights Reserved.
 * Licensed under the MIT License. See License.txt in the project root for license information.
 */
import { BrowserModule } from '@angular/platform-browser';
import { BrowserAnimationsModule } from '@angular/platform-browser/animations';
import { NgModule } from '@angular/core';
import { HttpClientModule, HTTP_INTERCEPTORS } from '@angular/common/http';
import { provideTranslateService, TranslateLoader } from '@ngx-translate/core';
import { MarkdownModule } from 'ngx-markdown';
import { RouteReuseStrategy } from '@angular/router';
import { CoreModule } from './@core/core.module';
import { ThemeModule } from './@theme/theme.module';
import { AppRoutingModule } from './app-routing.module';
import { I18nService } from './@core/i18n/i18n.service';
import { MergedTranslateLoader } from './@core/i18n/merged-translate.loader';
import {
  NbDatepickerModule,
  NbDialogModule,
  NbMenuModule,
  NbSidebarModule,
  NbTimepickerModule,
  NbToastrModule,
  NbWindowModule,
} from '@nebular/theme';
import { JwtInterceptor } from './@core/interceptors/jwt.interceptor';
import { AuthModule } from './auth/auth.module';
import { TabRouteReuseStrategy } from './@core/routing/tab-route-reuse.strategy';

@NgModule({
  imports: [
    BrowserModule,
    BrowserAnimationsModule,
    HttpClientModule,
    MarkdownModule.forRoot(),
    AuthModule,
    AppRoutingModule,
    NbSidebarModule.forRoot(),
    NbMenuModule.forRoot(),
    NbDatepickerModule.forRoot(),
    NbTimepickerModule.forRoot(),
    NbDialogModule.forRoot(),
    NbWindowModule.forRoot(),
    NbToastrModule.forRoot(),
    CoreModule.forRoot(),
    ThemeModule.forRoot(),
  ],
  providers: [
    I18nService,
    { provide: HTTP_INTERCEPTORS, useClass: JwtInterceptor, multi: true },
    { provide: RouteReuseStrategy, useClass: TabRouteReuseStrategy },
    ...provideTranslateService({ fallbackLang: 'zh' }),
    { provide: TranslateLoader, useFactory: () => new MergedTranslateLoader() },
  ],
})
export class AppModule {
  constructor(private i18n: I18nService) {
    this.i18n.init();
  }
}
