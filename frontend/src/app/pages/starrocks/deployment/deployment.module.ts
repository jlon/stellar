import { NgModule } from '@angular/core';

import { DeploymentRoutingModule } from './deployment-routing.module';
import { HostManagementComponent } from './host-management.component';
import { PackageManagementComponent } from './package-management.component';
import { DeploymentPlaceholderComponent } from './deployment-placeholder.component';

@NgModule({
  imports: [
    DeploymentPlaceholderComponent,
    HostManagementComponent,
    PackageManagementComponent,
    DeploymentRoutingModule,
  ],
})
export class DeploymentModule {}
