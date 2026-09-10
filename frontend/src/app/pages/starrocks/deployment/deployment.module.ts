import { NgModule } from '@angular/core';

import { DeploymentRoutingModule } from './deployment-routing.module';
import { HostManagementComponent } from './host-management.component';
import { PackageManagementComponent } from './package-management.component';
import { DeploymentConsoleComponent } from './deployment-console.component';
import { CredentialManagementComponent } from './credential-management.component';
import { AdoptionComponent } from './adoption.component';

@NgModule({
  imports: [
    DeploymentConsoleComponent,
    CredentialManagementComponent,
    AdoptionComponent,
    HostManagementComponent,
    PackageManagementComponent,
    DeploymentRoutingModule,
  ],
})
export class DeploymentModule {}
