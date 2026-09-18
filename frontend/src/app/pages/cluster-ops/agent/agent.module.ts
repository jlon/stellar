import { NgModule } from '@angular/core';
import { CommonModule } from '@angular/common';
import { RouterModule, Routes } from '@angular/router';

import { AgentComponent } from './agent.component';

const routes: Routes = [
  {
    path: '',
    component: AgentComponent,
  },
];

@NgModule({
  imports: [RouterModule.forChild(routes)],
  exports: [RouterModule],
})
export class AgentRoutingModule {}

@NgModule({
  imports: [CommonModule, AgentRoutingModule, AgentComponent],
})
export class AgentModule {}