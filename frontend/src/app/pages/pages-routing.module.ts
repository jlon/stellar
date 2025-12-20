import { RouterModule, Routes } from '@angular/router';
import { NgModule } from '@angular/core';

import { PagesComponent } from './pages.component';

const routes: Routes = [{
  path: '',
  component: PagesComponent,
  children: [
    {
      path: 'starrocks',
      loadChildren: () => import(/* webpackChunkName: "starrocks" */ './starrocks/starrocks.module')
        .then(m => m.StarRocksModule),
    },
    {
      path: 'cluster-ops',
      loadChildren: () => import(/* webpackChunkName: "cluster-ops" */ './cluster-ops/cluster-ops.module')
        .then(m => m.ClusterOpsModule),
    },
    {
      path: 'user-settings',
      loadChildren: () => import('./user-settings/user-settings.module')
        .then(m => m.UserSettingsModule),
    },
    {
      path: 'system',
      loadChildren: () => import('./system/system.module')
        .then(m => m.SystemModule),
    },
    {
      path: '',
      redirectTo: 'starrocks',
      pathMatch: 'full',
    },
  ],
}];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class PagesRoutingModule {
}
