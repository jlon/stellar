import { NgModule, NO_ERRORS_SCHEMA } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule, ReactiveFormsModule } from '@angular/forms';
import {
  NbCardModule,
  NbTabsetModule,
  NbButtonModule,
  NbButtonGroupModule,
  NbIconModule,
  NbInputModule,
  NbSpinnerModule,
  NbDialogModule,
  NbSelectModule,
  NbCheckboxModule,
  NbAlertModule,
  NbLayoutModule,
  NbActionsModule,
  NbBadgeModule,
  NbTagModule,
  NbTooltipModule,
  NbProgressBarModule,
  NbAccordionModule,
  NbListModule,
} from '@nebular/theme';
import { Ng2SmartTableModule } from 'ng2-smart-table';

import { PermissionManagementComponent } from './permission-management.component';
import { PermissionManagementRoutingModule } from './permission-management-routing.module';
import { PermissionDashboardStandardComponent } from './dashboard/permission-dashboard-standard.component';
import { PermissionRequestComponent } from './request/permission-request.component';
import { PermissionApprovalComponent } from './approval/permission-approval.component';
import { PermissionApprovalDetailDialogComponent } from './approval/permission-approval-detail-dialog.component';
import { ConfirmationDialogComponent } from './shared/confirmation-dialog.component';
import { CascadeSelectorComponent } from './shared/cascade-selector.component';

/**
 * PermissionManagementModule
 * Complete permission management module for Doris/StarRocks RBAC
 *
 * Features:
 * - Permission Dashboard (view current user permissions)
 * - Permission Request (submit new permission requests: grant_role, grant_permission, revoke_permission)
 * - Permission Approval (review and approve/reject pending requests)
 * - Backend integration with OLAP engine SQL generation
 */
@NgModule({
  declarations: [
    PermissionManagementComponent,
    PermissionDashboardStandardComponent,
    PermissionRequestComponent,
    PermissionApprovalComponent,
    PermissionApprovalDetailDialogComponent,
    ConfirmationDialogComponent,
    CascadeSelectorComponent,
  ],
  imports: [
    CommonModule,
    FormsModule,
    ReactiveFormsModule,
    PermissionManagementRoutingModule,
    NbCardModule,
    NbTabsetModule,
    NbButtonModule,
    NbButtonGroupModule,
    NbIconModule,
    NbInputModule,
    NbSpinnerModule,
    NbDialogModule,
    NbSelectModule,
    NbCheckboxModule,
    NbAlertModule,
    NbLayoutModule,
    NbActionsModule,
    NbBadgeModule,
    NbTagModule,
    NbTooltipModule,
    NbProgressBarModule,
    NbAccordionModule,
    NbListModule,
    Ng2SmartTableModule,
  ],
  schemas: [NO_ERRORS_SCHEMA],
})
export class PermissionManagementModule {}
