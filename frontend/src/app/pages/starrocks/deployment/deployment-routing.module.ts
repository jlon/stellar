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
    redirectTo: 'overview',
    pathMatch: 'full',
  },
  {
    path: 'overview',
    component: DeploymentConsoleComponent,
    data: {
      title: '部署总览',
      description: '集中查看物理机部署、受管集群和任务执行状态。',
      icon: 'layers-outline',
    },
  },
  {
    path: 'hosts',
    component: HostManagementComponent,
    data: {
      permission: 'menu:deployment',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'credentials',
    component: CredentialManagementComponent,
    data: {
      title: 'SSH 凭据',
      description: '维护部署服务账号和经确认的 SSH 主机身份信息。',
      icon: 'lock-outline',
    },
  },
  {
    path: 'packages',
    component: PackageManagementComponent,
    data: {
      permission: 'menu:deployment',
    },
    canActivate: [PermissionGuard],
  },
  {
    path: 'clusters',
    component: DeploymentConsoleComponent,
    data: {
      title: '托管集群',
      description: '查看由 Stellar 部署或只读接管的 StarRocks 集群。',
      icon: 'cube-outline',
    },
  },
  {
    path: 'deploy',
    component: DeploymentConsoleComponent,
    data: {
      title: '新建部署',
      description: '按主机、凭据、安装包和拓扑创建受控部署计划。',
      icon: 'plus-circle-outline',
    },
  },
  {
    path: 'adopt',
    component: AdoptionComponent,
    data: {
      title: '集群接管',
      description: '以只读模式纳入现有 StarRocks 集群，后续再完成基础设施绑定。',
      icon: 'link-2-outline',
    },
  },
  {
    path: 'tasks',
    component: DeploymentConsoleComponent,
    data: {
      title: '部署任务',
      description: '追踪部署、预检和接管任务的步骤、日志及最终结果。',
      icon: 'clock-outline',
    },
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class DeploymentRoutingModule {}
