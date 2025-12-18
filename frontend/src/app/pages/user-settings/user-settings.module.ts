import { NgModule } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { 
  NbCardModule, 
  NbInputModule, 
  NbButtonModule, 
  NbAlertModule, 
  NbSpinnerModule,
  NbIconModule,
  NbTooltipModule,
  NbRadioModule
} from '@nebular/theme';

import { UserSettingsComponent } from './user-settings.component';
import { RouterModule } from '@angular/router';

@NgModule({
  declarations: [
    UserSettingsComponent
  ],
  imports: [
    CommonModule,
    FormsModule,
    NbCardModule,
    NbInputModule,
    NbButtonModule,
    NbAlertModule,
    NbSpinnerModule,
    NbIconModule,
    NbTooltipModule,
    NbRadioModule,
    RouterModule.forChild([
      {
        path: '',
        component: UserSettingsComponent
      }
    ])
  ]
})
export class UserSettingsModule { }

