import { ModuleWithProviders, NgModule } from '@angular/core';
import { CommonModule } from '@angular/common';
import { NbActionsModule, NbLayoutModule, NbMenuModule, NbSidebarModule, NbUserModule, NbContextMenuModule, NbButtonModule, NbSelectModule, NbIconModule, NbThemeModule, NbTooltipModule, NbSpinnerModule } from '@nebular/theme';
import { NbEvaIconsModule } from '@nebular/eva-icons';
import { NbSecurityModule } from '@nebular/security';

import {
  HeaderComponent,
} from './components';
import { ClusterSelectorComponent } from './components/cluster-selector/cluster-selector.component';
import { TabBarComponent } from './components/tab-bar/tab-bar.component';
import { OneColumnLayoutComponent } from './layouts/one-column/one-column.layout';
import { DEFAULT_THEME } from './styles/theme.default';
import { COSMIC_THEME } from './styles/theme.cosmic';
import { CORPORATE_THEME } from './styles/theme.corporate';
import { DARK_THEME } from './styles/theme.dark';
import { resolveInitialTheme } from './styles/theme-preference';

const NB_MODULES = [
  NbLayoutModule,
  NbMenuModule,
  NbUserModule,
  NbActionsModule,
  NbSidebarModule,
  NbContextMenuModule,
  NbSecurityModule,
  NbButtonModule,
  NbSelectModule,
  NbIconModule,
  NbTooltipModule,
  NbSpinnerModule,
  NbEvaIconsModule,
];
const COMPONENTS = [
  HeaderComponent,
  ClusterSelectorComponent,
  TabBarComponent,
  OneColumnLayoutComponent,
];

@NgModule({
    imports: [CommonModule, ...NB_MODULES, ...COMPONENTS],
    exports: [CommonModule, ...COMPONENTS, ...NB_MODULES],
})
export class ThemeModule {
  static forRoot(): ModuleWithProviders<ThemeModule> {
    return {
      ngModule: ThemeModule,
      providers: [
        ...NbThemeModule.forRoot(
          {
            name: resolveInitialTheme(),
          },
          [ DEFAULT_THEME, COSMIC_THEME, CORPORATE_THEME, DARK_THEME ],
        ).providers,
      ],
    };
  }
}
