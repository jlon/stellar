import { NgModule, NO_ERRORS_SCHEMA } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule, ReactiveFormsModule } from '@angular/forms';
import {
  NbCardModule,
  NbTabsetModule,
  NbButtonModule,
  NbIconModule,
  NbInputModule,
  NbSpinnerModule,
  NbDialogModule,
  NbSelectModule,
  NbCheckboxModule,
  NbAlertModule,
} from '@nebular/theme';
import { Ng2SmartTableModule } from 'ng2-smart-table';

import { AuthComponent } from './auth.component';
import { AuthRoutingModule } from './auth-routing.module';
import { MyRequestsComponent } from './my-requests/my-requests.component';
import { PendingApprovalsComponent } from './pending-approvals/pending-approvals.component';
import { AccountsListComponent } from './accounts-list/accounts-list.component';
import { RolesListComponent } from './roles-list/roles-list.component';

/**
 * AuthModule
 * 权限管控模块
 *
 * 功能：
 * - 权限申请提交和管理
 * - 权限申请审批
 * - 数据库账户和角色查看
 */
@NgModule({
  declarations: [
    AuthComponent,
    MyRequestsComponent,
    PendingApprovalsComponent,
    AccountsListComponent,
    RolesListComponent,
  ],
  imports: [
    CommonModule,
    FormsModule,
    ReactiveFormsModule,
    AuthRoutingModule,
    NbCardModule,
    NbTabsetModule,
    NbButtonModule,
    NbIconModule,
    NbInputModule,
    NbSpinnerModule,
    NbDialogModule,
    NbSelectModule,
    NbCheckboxModule,
    NbAlertModule,
    Ng2SmartTableModule,
  ],
  schemas: [NO_ERRORS_SCHEMA],
})
export class AuthModule {}
