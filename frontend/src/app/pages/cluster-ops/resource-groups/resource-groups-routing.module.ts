import { NgModule } from '@angular/core';
import { RouterModule, Routes } from '@angular/router';

import { ResourceGroupsListComponent } from './resource-groups-list/resource-groups-list.component';
import { ResourceGroupFormComponent } from './resource-group-form/resource-group-form.component';
import { ResourceGroupUsageComponent } from './resource-group-usage/resource-group-usage.component';
import { ResourceGroupAnalysisComponent } from './resource-group-analysis/resource-group-analysis.component';

const routes: Routes = [
  {
    path: '',
    component: ResourceGroupsListComponent,
  },
  {
    path: 'create',
    component: ResourceGroupFormComponent,
  },
  {
    path: 'edit/:name',
    component: ResourceGroupFormComponent,
  },
  {
    path: 'usage',
    component: ResourceGroupUsageComponent,
  },
  {
    path: 'analysis',
    component: ResourceGroupAnalysisComponent,
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class ResourceGroupsRoutingModule {}