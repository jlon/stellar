import { NgModule } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule, ReactiveFormsModule } from '@angular/forms';
import {
  NbActionsModule,
  NbButtonModule,
  NbCardModule,
  NbCheckboxModule,
  NbDialogModule,
  NbFormFieldModule,
  NbIconModule,
  NbInputModule,
  NbListModule,
  NbSelectModule,
  NbSpinnerModule,
  NbTabsetModule,
  NbTagModule,
  NbTooltipModule,
  NbAlertModule,
  NbProgressBarModule,
} from '@nebular/theme';
import { Ng2SmartTableModule } from 'ng2-smart-table';

import { ResourceGroupsRoutingModule } from './resource-groups-routing.module';
import { ResourceGroupsListComponent } from './resource-groups-list/resource-groups-list.component';
import { ResourceGroupFormComponent } from './resource-group-form/resource-group-form.component';
import { ResourceGroupUsageComponent } from './resource-group-usage/resource-group-usage.component';
import { ResourceGroupAnalysisComponent } from './resource-group-analysis/resource-group-analysis.component';

@NgModule({
  declarations: [
    ResourceGroupsListComponent,
    ResourceGroupFormComponent,
    ResourceGroupUsageComponent,
    ResourceGroupAnalysisComponent,
  ],
  imports: [
    CommonModule,
    FormsModule,
    ReactiveFormsModule,
    ResourceGroupsRoutingModule,
    NbActionsModule,
    NbButtonModule,
    NbCardModule,
    NbCheckboxModule,
    NbDialogModule,
    NbFormFieldModule,
    NbIconModule,
    NbInputModule,
    NbListModule,
    NbSelectModule,
    NbSpinnerModule,
    NbTabsetModule,
    NbTagModule,
    NbTooltipModule,
    NbAlertModule,
    NbProgressBarModule,
    Ng2SmartTableModule,
  ],
})
export class ResourceGroupsModule {}