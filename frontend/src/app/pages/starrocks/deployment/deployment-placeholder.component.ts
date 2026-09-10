import { Component, inject } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { NbCardModule, NbIconModule } from '@nebular/theme';

interface DeploymentRouteData {
  title: string;
  description: string;
  icon: string;
}

@Component({
  selector: 'ngx-deployment-placeholder',
  imports: [NbCardModule, NbIconModule],
  templateUrl: './deployment-placeholder.component.html',
  styleUrls: ['./deployment-placeholder.component.scss'],
})
export class DeploymentPlaceholderComponent {
  private readonly route = inject(ActivatedRoute);

  readonly page: DeploymentRouteData = this.route.snapshot.data as DeploymentRouteData;
}
