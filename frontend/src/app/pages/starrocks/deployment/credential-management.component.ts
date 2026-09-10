import { CommonModule } from '@angular/common';
import { Component, OnInit, inject } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { forkJoin } from 'rxjs';
import { NbAlertModule, NbButtonModule, NbCardModule, NbInputModule, NbListModule, NbToastrService } from '@nebular/theme';

import { DeploymentCredential, SrDeploymentService } from '../../../@core/data/sr-deployment.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

@Component({
  selector: 'ngx-credential-management',
  imports: [CommonModule, ReactiveFormsModule, NbAlertModule, NbButtonModule, NbCardModule, NbInputModule, NbListModule],
  templateUrl: './credential-management.component.html',
  styleUrls: ['./credential-management.component.scss'],
})
export class CredentialManagementComponent implements OnInit {
  private readonly formBuilder = inject(FormBuilder);
  private readonly deploymentService = inject(SrDeploymentService);
  private readonly toastr = inject(NbToastrService);

  sshCredentials: DeploymentCredential[] = [];
  databaseCredentials: DeploymentCredential[] = [];
  loading = false;
  submitting = false;

  readonly sshForm = this.formBuilder.group({
    name: ['', [Validators.required, Validators.maxLength(100)]],
    username: ['', [Validators.required, Validators.pattern(/^[A-Za-z0-9_.-]+$/)]],
    private_key: ['', Validators.required],
  });
  readonly databaseForm = this.formBuilder.group({
    name: ['', [Validators.required, Validators.maxLength(100)]],
    username: ['', [Validators.required, Validators.pattern(/^[A-Za-z0-9_.-]+$/)]],
    password: ['', [Validators.required, Validators.minLength(8), Validators.maxLength(64)]],
  });

  ngOnInit(): void { this.load(); }

  saveSsh(): void {
    if (this.sshForm.invalid) { this.sshForm.markAllAsTouched(); return; }
    this.submitting = true;
    const value = this.sshForm.getRawValue();
    this.deploymentService.createSshCredential({ name: value.name || '', username: value.username || '', private_key: value.private_key || '' }).subscribe({
      next: () => { this.submitting = false; this.sshForm.reset(); this.toastr.success('私钥已加密保存。', 'SSH 凭据已创建'); this.load(); },
      error: (error) => { this.submitting = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }

  saveDatabase(): void {
    if (this.databaseForm.invalid) { this.databaseForm.markAllAsTouched(); return; }
    this.submitting = true;
    const value = this.databaseForm.getRawValue();
    this.deploymentService.createDatabaseCredential({ name: value.name || '', username: value.username || '', password: value.password || '' }).subscribe({
      next: () => { this.submitting = false; this.databaseForm.reset(); this.toastr.success('密码已加密保存。', '运维账号已创建'); this.load(); },
      error: (error) => { this.submitting = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }

  private load(): void {
    this.loading = true;
    forkJoin({ ssh: this.deploymentService.listSshCredentials(), database: this.deploymentService.listDatabaseCredentials() }).subscribe({
      next: (data) => { this.sshCredentials = data.ssh; this.databaseCredentials = data.database; this.loading = false; },
      error: (error) => { this.loading = false; ErrorHandler.handleHttpError(error, this.toastr); },
    });
  }
}
