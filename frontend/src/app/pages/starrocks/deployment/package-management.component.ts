import { CommonModule } from '@angular/common';
import { Component, DestroyRef, inject } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import {
  NbAlertModule,
  NbButtonModule,
  NbCardModule,
  NbIconModule,
  NbInputModule,
  NbSelectModule,
  NbSpinnerModule,
  NbToastrService,
} from '@nebular/theme';

import { AuthService } from '../../../@core/data/auth.service';
import { Organization, OrganizationService } from '../../../@core/data/organization.service';
import {
  CreateSrPackageRequest,
  SrPackage,
  SrPackageService,
} from '../../../@core/data/sr-package.service';
import { PermissionService } from '../../../@core/data/permission.service';
import { ErrorHandler } from '../../../@core/utils/error-handler';

const HTTPS_URL_PATTERN = /^https:\/\/\S+$/;
const SHA256_PATTERN = /^[a-fA-F0-9]{64}$/;

@Component({
  selector: 'ngx-package-management',
  imports: [
    CommonModule,
    ReactiveFormsModule,
    NbAlertModule,
    NbButtonModule,
    NbCardModule,
    NbIconModule,
    NbInputModule,
    NbSelectModule,
    NbSpinnerModule,
  ],
  templateUrl: './package-management.component.html',
  styleUrls: ['./package-management.component.scss'],
})
export class PackageManagementComponent {
  private readonly destroyRef = inject(DestroyRef);
  private readonly formBuilder = inject(FormBuilder);
  private readonly authService = inject(AuthService);
  private readonly organizationService = inject(OrganizationService);
  private readonly packageService = inject(SrPackageService);
  private readonly permissionService = inject(PermissionService);
  private readonly toastrService = inject(NbToastrService);

  readonly packageForm = this.formBuilder.group({
    organization_id: [null as number | null],
    version: ['', [Validators.required, Validators.maxLength(64)]],
    package_url: ['', [Validators.required, Validators.pattern(HTTPS_URL_PATTERN)]],
    sha256: ['', [Validators.required, Validators.pattern(SHA256_PATTERN)]],
  });

  packages: SrPackage[] = [];
  organizations: Organization[] = [];
  loading = false;
  submitting = false;
  showCreateForm = false;
  isSuperAdmin = false;
  canListPackages = false;
  canManagePackages = false;

  constructor() {
    this.permissionService.permissions$
      .pipe(takeUntilDestroyed(this.destroyRef))
      .subscribe(() => this.applyPermissionState());
  }

  openCreateForm(): void {
    if (!this.canManagePackages) {
      return;
    }

    this.showCreateForm = true;
    this.packageForm.reset({
      organization_id: this.isSuperAdmin ? null : this.currentOrganizationId,
      version: '',
      package_url: '',
      sha256: '',
    });

    if (this.isSuperAdmin) {
      this.packageForm.controls.organization_id.setValidators([Validators.required]);
      this.packageForm.controls.organization_id.enable();
      this.packageForm.controls.organization_id.updateValueAndValidity();
      this.loadOrganizations();
    } else {
      this.packageForm.controls.organization_id.clearValidators();
      this.packageForm.controls.organization_id.disable();
      this.packageForm.controls.organization_id.updateValueAndValidity();
    }
  }

  closeCreateForm(): void {
    this.showCreateForm = false;
  }

  submitPackage(): void {
    if (this.packageForm.invalid) {
      this.packageForm.markAllAsTouched();
      return;
    }

    const value = this.packageForm.getRawValue();
    const request: CreateSrPackageRequest = {
      organization_id: value.organization_id ?? undefined,
      version: value.version?.trim() || '',
      package_url: value.package_url?.trim() || '',
      sha256: value.sha256?.trim() || '',
    };

    this.submitting = true;
    this.packageService.createPackage(request).subscribe({
      next: () => {
        this.toastrService.success('安装包元数据已登记，尚未下载到控制面缓存。', '登记成功');
        this.submitting = false;
        this.closeCreateForm();
        this.loadPackages();
      },
      error: (error) => {
        this.submitting = false;
        ErrorHandler.handleHttpError(error, this.toastrService);
      },
    });
  }

  statusLabel(status: SrPackage['status']): string {
    return {
      pending: '待缓存',
      cached: '已缓存',
      failed: '缓存失败',
    }[status];
  }

  statusClass(status: SrPackage['status']): string {
    return `package-status package-status--${status}`;
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
    this.canListPackages = this.isSuperAdmin
      || this.permissionService.hasPermission('api:sr-ops:packages:list');
    this.canManagePackages = this.isSuperAdmin
      || this.permissionService.hasPermission('api:sr-ops:packages:manage');

    if (this.isSuperAdmin) {
      this.loadOrganizations();
    }
    if (this.canListPackages) {
      this.loadPackages();
    } else {
      this.packages = [];
    }
  }

  private loadPackages(): void {
    this.loading = true;
    this.packageService.listPackages().subscribe({
      next: (packages) => {
        this.packages = packages;
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
