import { NgModule, NO_ERRORS_SCHEMA } from '@angular/core';
import { CommonModule } from '@angular/common';

import { ClusterOpsRoutingModule } from './cluster-ops-routing.module';

/**
 * ClusterOpsModule
 * 集群运维模块主入口
 *
 * 包含：
 * - 权限管控子模块
 */
@NgModule({
  declarations: [],
  imports: [CommonModule, ClusterOpsRoutingModule],
  schemas: [NO_ERRORS_SCHEMA],
})
export class ClusterOpsModule {}
