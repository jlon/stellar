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
  // Legacy alias for backward compatibility
  {
    path: 'auth',
    redirectTo: 'permission-management',
    pathMatch: 'full',
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class ClusterOpsRoutingModule {}
