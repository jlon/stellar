import { NgModule } from '@angular/core';
import { RouterModule, Routes } from '@angular/router';
import { PermissionGuard } from '../../../@core/guards/permission.guard';

import { HostManagementComponent } from './host-management.component';
import { PackageManagementComponent } from './package-management.component';
import { DeploymentConsoleComponent } from './deployment-console.component';
import { CredentialManagementComponent } from './credential-management.component';
import { AdoptionComponent } from './adoption.component';

const routes: Routes = [
  {
    path: '',
    redirectTo: 'clusters',
    pathMatch: 'full',
  },
  // The standalone overview was merged into the managed-clusters landing
  // page; keep the old URL working.
  {
    path: 'overview',
    redirectTo: 'clusters',
    pathMatch: 'full',
  },
  {
    path: 'hosts',
    component: HostManagementComponent,
    data: {
      permission: 'menu:deployment:hosts',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'credentials',
    component: CredentialManagementComponent,
    data: {
      title: 'SSH 凭据',
      permission: 'menu:deployment:credentials',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'packages',
    component: PackageManagementComponent,
    data: {
      permission: 'menu:deployment:packages',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'clusters',
    component: DeploymentConsoleComponent,
    data: {
      title: '托管集群',
      permission: 'menu:deployment:clusters',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'deploy',
    component: DeploymentConsoleComponent,
    data: {
      title: '新建部署',
      permission: 'menu:deployment:deploy',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'adopt',
    component: AdoptionComponent,
    data: {
      title: '集群接管',
      permission: 'menu:deployment:adopt',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'tasks',
    component: DeploymentConsoleComponent,
    data: {
      title: '部署任务',
      permission: 'menu:deployment:tasks',
    },
    canActivate: [PermissionGuard],
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class DeploymentRoutingModule {}
