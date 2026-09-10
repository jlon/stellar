import { CommonModule } from '@angular/common';
import { Component, OnDestroy, OnInit, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { Subscription, forkJoin, timer } from 'rxjs';
import {
  NbAlertModule, NbButtonModule, NbCardModule, NbCheckboxModule, NbIconModule,
  NbInputModule, NbListModule, NbSelectModule, NbSpinnerModule, NbToastrService,
} from '@nebular/theme';

import { PhysicalHost, PhysicalHostService } from '../../../@core/data/physical-host.service';
import { SrPackage, SrPackageService } from '../../../@core/data/sr-package.service';
import {
  DeploymentCredential, DeploymentTask, DeploymentTaskDetail, ManagedCluster, SrDeploymentService,
} from '../../../@core/data/sr-deployment.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

type ConsoleMode = 'overview' | 'deploy' | 'tasks' | 'clusters';

@Component({
  selector: 'ngx-deployment-console',
  imports: [
    CommonModule, ReactiveFormsModule, NbAlertModule, NbButtonModule, NbCardModule, NbCheckboxModule,
    NbIconModule, NbInputModule, NbListModule, NbSelectModule, NbSpinnerModule,
  ],
  templateUrl: './deployment-console.component.html',
  styleUrls: ['./deployment-console.component.scss'],
})
export class DeploymentConsoleComponent implements OnInit, OnDestroy {
  private readonly formBuilder = inject(FormBuilder);
  private readonly deploymentService = inject(SrDeploymentService);
  private readonly hostService = inject(PhysicalHostService);
  private readonly packageService = inject(SrPackageService);
  private readonly toastr = inject(NbToastrService);
  private taskRefresh?: Subscription;

  mode: ConsoleMode = 'overview';
  loading = false;
  submitting = false;
  hosts: PhysicalHost[] = [];
  packages: SrPackage[] = [];
  sshCredentials: DeploymentCredential[] = [];
  databaseCredentials: DeploymentCredential[] = [];
  tasks: DeploymentTask[] = [];
  clusters: ManagedCluster[] = [];
  selectedTask?: DeploymentTaskDetail;
  error = '';

  readonly deployForm = this.formBuilder.group({
    name: ['', [Validators.required, Validators.pattern(/^[A-Za-z0-9_.-]+$/)]],
    package_id: [null as number | null, Validators.required],
    ssh_credential_id: [null as number | null, Validators.required],
    operator_credential_id: [null as number | null, Validators.required],
    install_dir: ['/opt/starrocks', [Validators.required, Validators.pattern(/^\/[A-Za-z0-9/_.-]+$/)]],
    leader_host_id: [null as number | null, Validators.required],
    leader_advertise_host: ['', [Validators.required, Validators.pattern(/^\d{1,3}(\.\d{1,3}){3}$/)]],
    backend_host_id: [null as number | null, Validators.required],
    backend_advertise_host: ['', [Validators.required, Validators.pattern(/^\d{1,3}(\.\d{1,3}){3}$/)]],
    confirm_non_ha: [false, Validators.requiredTrue],
  });

  ngOnInit(): void {
    this.mode = (location.pathname.split('/').pop() as ConsoleMode) || 'overview';
    this.load();
    if (this.mode === 'overview' || this.mode === 'tasks') {
      this.taskRefresh = timer(5_000, 5_000).subscribe(() => this.loadTasks());
    }
  }

  ngOnDestroy(): void {
    this.taskRefresh?.unsubscribe();
  }

  submitDeployment(): void {
    if (this.deployForm.invalid) {
      this.deployForm.markAllAsTouched();
      return;
    }
    const value = this.deployForm.getRawValue();
    this.submitting = true;
    this.error = '';
    this.deploymentService.createDeployment({
      name: value.name || '', package_id: value.package_id || 0,
      ssh_credential_id: value.ssh_credential_id || 0,
      operator_credential_id: value.operator_credential_id || 0,
      install_dir: value.install_dir || '', confirm_non_ha: true,
      frontends: [{ host_id: value.leader_host_id || 0, advertise_host: value.leader_advertise_host || '' }],
      backends: [{ host_id: value.backend_host_id || 0, advertise_host: value.backend_advertise_host || '' }],
    }).subscribe({
      next: (task) => {
        this.submitting = false;
        this.toastr.success(`任务 #${task.id} 已开始执行。`, '部署已提交');
        this.mode = 'tasks';
        this.loadTasks();
      },
      error: (error) => { this.submitting = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }

  selectTask(task: DeploymentTask): void {
    this.deploymentService.getTask(task.id).subscribe({
      next: (detail) => this.selectedTask = detail,
      error: (error) => ErrorHandler.handleHttpError(error, this.toastr),
    });
  }

  hostLabel(host: PhysicalHost): string {
    return `${host.hostname} (${host.ssh_target})`;
  }

  get runningTaskCount(): number {
    return this.tasks.filter((task) => task.status === 'running').length;
  }

  private load(): void {
    if (this.mode === 'deploy') {
      this.loading = true;
      forkJoin({
        hosts: this.hostService.listHosts(), packages: this.packageService.listPackages(),
        ssh: this.deploymentService.listSshCredentials(), database: this.deploymentService.listDatabaseCredentials(),
      }).subscribe({
        next: (data) => { this.hosts = data.hosts; this.packages = data.packages; this.sshCredentials = data.ssh; this.databaseCredentials = data.database; this.loading = false; },
        error: (error) => { this.loading = false; ErrorHandler.handleHttpError(error, this.toastr); },
      });
    } else if (this.mode === 'clusters') {
      this.loading = true;
      this.deploymentService.listClusters().subscribe({ next: (clusters) => { this.clusters = clusters; this.loading = false; }, error: (error) => { this.loading = false; ErrorHandler.handleHttpError(error, this.toastr); } });
    } else {
      this.loadTasks();
      if (this.mode === 'overview') this.deploymentService.listClusters().subscribe({ next: (clusters) => this.clusters = clusters });
    }
  }

  private loadTasks(): void {
    this.loading = this.tasks.length === 0;
    this.deploymentService.listTasks().subscribe({
      next: (tasks) => { this.tasks = tasks; this.loading = false; },
      error: (error) => { this.loading = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }
}
