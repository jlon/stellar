import { CommonModule } from '@angular/common';
import { Component, OnInit, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { NbAlertModule, NbButtonModule, NbCardModule, NbInputModule, NbSelectModule, NbToastrService } from '@nebular/theme';

import { DeploymentCredential, SrDeploymentService } from '../../../@core/data/sr-deployment.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

@Component({
  selector: 'ngx-read-only-adoption',
  imports: [CommonModule, ReactiveFormsModule, NbAlertModule, NbButtonModule, NbCardModule, NbInputModule, NbSelectModule],
  templateUrl: './adoption.component.html',
  styleUrls: ['./adoption.component.scss'],
})
export class AdoptionComponent implements OnInit {
  private readonly formBuilder = inject(FormBuilder);
  private readonly deploymentService = inject(SrDeploymentService);
  private readonly toastr = inject(NbToastrService);

  credentials: DeploymentCredential[] = [];
  submitting = false;
  readonly adoptionForm = this.formBuilder.group({
    name: ['', [Validators.required, Validators.pattern(/^[A-Za-z0-9_.-]+$/)]],
    fe_host: ['', [Validators.required, Validators.maxLength(255)]],
    fe_http_port: [8030, [Validators.required, Validators.min(1), Validators.max(65535)]],
    fe_query_port: [9030, [Validators.required, Validators.min(1), Validators.max(65535)]],
    operator_credential_id: [null as number | null, Validators.required],
  });

  ngOnInit(): void {
    this.deploymentService.listDatabaseCredentials().subscribe({
      next: (credentials) => this.credentials = credentials,
      error: (error) => ErrorHandler.handleHttpError(error, this.toastr),
    });
  }

  submit(): void {
    if (this.adoptionForm.invalid) { this.adoptionForm.markAllAsTouched(); return; }
    const value = this.adoptionForm.getRawValue();
    this.submitting = true;
    this.deploymentService.createReadOnlyAdoption({
      name: value.name || '', fe_host: value.fe_host || '',
      fe_http_port: value.fe_http_port || 8030, fe_query_port: value.fe_query_port || 9030,
      operator_credential_id: value.operator_credential_id || 0,
    }).subscribe({
      next: (task) => { this.submitting = false; this.toastr.success(`任务 #${task.id} 正在导入观测节点。`, '只读接管已提交'); },
      error: (error) => { this.submitting = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }
}
