import { NgModule } from '@angular/core';
import { RouterModule, Routes } from '@angular/router';

const routes: Routes = [
  {
    path: 'permission-management',
    loadChildren: () =>
      import('./permission-management/permission-management.module').then(
        (m) => m.PermissionManagementModule
      ),
  },
  {
    path: 'resource-groups',
    loadChildren: () =>
      import('./resource-groups/resource-groups.module').then(
        (m) => m.ResourceGroupsModule
      ),
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class ClusterOpsRoutingModule {}
