import { CommonModule } from '@angular/common';
import { Component, DestroyRef, inject } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import {
  NbAlertModule,
  NbButtonModule,
  NbCardModule,
  NbCheckboxModule,
  NbIconModule,
  NbInputModule,
  NbSelectModule,
  NbSpinnerModule,
  NbToastrService,
} from '@nebular/theme';

import { AuthService } from '../../../@core/data/auth.service';
import { Organization, OrganizationService } from '../../../@core/data/organization.service';
import {
  CreatePhysicalHostRequest,
  PhysicalHost,
  PhysicalHostService,
} from '../../../@core/data/physical-host.service';
import { PermissionService } from '../../../@core/data/permission.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

const HOST_KEY_PATTERN = /^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(?:256|384|521)) [A-Za-z0-9+/=]+$/;
const FINGERPRINT_PATTERN = /^SHA256:[A-Za-z0-9+/]{43}$/;
const SSH_TARGET_PATTERN = /^[A-Za-z0-9.:-]+$/;

@Component({
  selector: 'ngx-host-management',
  imports: [
    CommonModule,
    ReactiveFormsModule,
    NbAlertModule,
    NbButtonModule,
    NbCardModule,
    NbCheckboxModule,
    NbIconModule,
    NbInputModule,
    NbSelectModule,
    NbSpinnerModule,
  ],
  templateUrl: './host-management.component.html',
  styleUrls: ['./host-management.component.scss'],
})
export class HostManagementComponent {
  private readonly destroyRef = inject(DestroyRef);
  private readonly formBuilder = inject(FormBuilder);
  private readonly authService = inject(AuthService);
  private readonly organizationService = inject(OrganizationService);
  private readonly physicalHostService = inject(PhysicalHostService);
  private readonly permissionService = inject(PermissionService);
  private readonly toastrService = inject(NbToastrService);

  readonly hostForm = this.formBuilder.group({
    organization_id: [null as number | null],
    hostname: ['', [Validators.required, Validators.maxLength(255)]],
    ssh_target: ['', [Validators.required, Validators.maxLength(255), Validators.pattern(SSH_TARGET_PATTERN)]],
    ssh_port: [22, [Validators.required, Validators.min(1), Validators.max(65535)]],
    host_key: ['', [Validators.required, Validators.pattern(HOST_KEY_PATTERN)]],
    host_key_fingerprint: ['', [Validators.required, Validators.pattern(FINGERPRINT_PATTERN)]],
    host_key_confirmed: [false, Validators.requiredTrue],
  });

  hosts: PhysicalHost[] = [];
  organizations: Organization[] = [];
  loading = false;
  submitting = false;
  showCreateForm = false;
  isSuperAdmin = false;
  canListHosts = false;
  canManageHosts = false;

  constructor() {
    this.permissionService.permissions$
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.applyPermissionState());
  }

  openCreateForm(): void {
    if (!this.canManageHosts) {
      return;
    }

    this.showCreateForm = true;
    this.hostForm.reset({
      organization_id: this.isSuperAdmin ? null : this.currentOrganizationId,
      hostname: '',
      ssh_target: '',
      ssh_port: 22,
      host_key: '',
      host_key_fingerprint: '',
      host_key_confirmed: false,
    });

    if (this.isSuperAdmin) {
      this.hostForm.controls.organization_id.setValidators([Validators.required]);
      this.hostForm.controls.organization_id.enable();
      this.hostForm.controls.organization_id.updateValueAndValidity();
      this.loadOrganizations();
    } else {
      this.hostForm.controls.organization_id.clearValidators();
      this.hostForm.controls.organization_id.disable();
      this.hostForm.controls.organization_id.updateValueAndValidity();
    }
  }

  closeCreateForm(): void {
    this.showCreateForm = false;
  }

  submitHost(): void {
    if (this.hostForm.invalid) {
      this.hostForm.markAllAsTouched();
      return;
    }

    const value = this.hostForm.getRawValue();
    const request: CreatePhysicalHostRequest = {
      organization_id: value.organization_id ?? undefined,
      hostname: value.hostname?.trim() || '',
      ssh_target: value.ssh_target?.trim() || '',
      ssh_port: value.ssh_port || 22,
      host_key: value.host_key?.trim() || '',
      host_key_fingerprint: value.host_key_fingerprint?.trim() || '',
    };

    this.submitting = true;
    this.physicalHostService.createHost(request).subscribe({
      next: () => {
        this.toastrService.success('主机已登记，部署前仍需执行远程预检。', '登记成功');
        this.submitting = false;
        this.closeCreateForm();
        this.loadHosts();
      },
      error: (error) => {
        this.submitting = false;
        ErrorHandler.handleHttpError(error, this.toastrService);
      },
    });
  }

  statusLabel(status: PhysicalHost['status']): string {
    return {
      online: '在线',
      offline: '离线',
      unknown: '未检测',
    }[status];
  }

  statusClass(status: PhysicalHost['status']): string {
    return `host-status host-status--${status}`;
  }

  organizationName(organizationId: number): string {
    return this.organizations.find((organization) => organization.id === organizationId)?.name
      || `组织 #${organizationId}`;
  }

  private get currentOrganizationId(): number | null {
    return this.authService.currentUserValue?.organization_id ?? null;
  }

  private applyPermissionState(): void {
    this.isSuperAdmin = this.authService.isSuperAdmin();
    this.canListHosts = this.isSuperAdmin
      || this.permissionService.hasPermission('api:sr-ops:hosts:list');
    this.canManageHosts = this.isSuperAdmin
      || this.permissionService.hasPermission('api:sr-ops:hosts:manage');

    if (this.isSuperAdmin) {
      this.loadOrganizations();
    }
    if (this.canListHosts) {
      this.loadHosts();
    } else {
      this.hosts = [];
    }
  }

  private loadHosts(): void {
    this.loading = true;
    this.physicalHostService.listHosts().subscribe({
      next: (hosts) => {
        this.hosts = hosts;
        this.loading = false;
      },
      error: (error) => {
        this.loading = false;
        ErrorHandler.handleHttpError(error, this.toastrService);
      },
    });
  }

  private loadOrganizations(): void {
    if (!this.isSuperAdmin || this.organizations.length > 0) {
      return;
    }

    this.organizationService.listOrganizations().subscribe({
      next: (organizations) => {
        this.organizations = organizations;
      },
      error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
    });
  }
}
