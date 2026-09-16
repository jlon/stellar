import { Component, Input, OnInit, inject } from '@angular/core';
import { FormBuilder, FormGroup, Validators, FormsModule, ReactiveFormsModule } from '@angular/forms';
import { NbDialogRef, NbToastrService, NbCardModule, NbFormFieldModule, NbSelectModule, NbOptionModule, NbIconModule, NbInputModule, NbCheckboxModule, NbButtonModule, NbSpinnerModule } from '@nebular/theme';
import { timeout } from 'rxjs';
import { ClusterService, Cluster } from '../../../../@core/data/cluster.service';
import { OrganizationService, Organization } from '../../../../@core/data/organization.service';
import { AuthService } from '../../../../@core/data/auth.service';
import { ErrorHandler } from '../../../../@core/utils/error-handler';

/** 集群创建/编辑弹窗（llm-provider-form-dialog 同款范式）。 */
@Component({
    selector: 'ngx-cluster-form',
    templateUrl: './cluster-form.component.html',
    styleUrls: ['./cluster-form.component.scss'],
    imports: [
    NbCardModule,
    FormsModule,
    ReactiveFormsModule,
    NbFormFieldModule,
    NbSelectModule,
    NbOptionModule,
    NbIconModule,
    NbInputModule,
    NbCheckboxModule,
    NbButtonModule,
    NbSpinnerModule
],
})
export class ClusterFormComponent implements OnInit {
  private dialogRef = inject<NbDialogRef<ClusterFormComponent>>(NbDialogRef);
  private fb = inject(FormBuilder);
  private clusterService = inject(ClusterService);
  private organizationService = inject(OrganizationService);
  private authService = inject(AuthService);
  private toastrService = inject(NbToastrService);

  /** 编辑时传入集群 id；新增时为空。 */
  @Input() clusterId: number | null = null;
  clusterForm: FormGroup;
  loading = false;
  /** 保存中（与测试连接互不阻塞：测试超时/挂起不再锁死保存按钮） */
  saving = false;
  /** 测试连接中 */
  testing = false;
  connectionTested = false; // Track if connection has been tested
  connectionValid = false;  // Track if connection is valid
  
  get isEditMode(): boolean {
    return this.clusterId !== null;
  }

  // Organization support
  organizations: Organization[] = [];
  currentOrganization?: Organization;
  isSuperAdmin = false;
  isOrgAdmin = false;  // Check if user is organization admin
  organizationsLoading = false;

  constructor() {
    this.clusterForm = this.fb.group({
      organization_id: [null],
      name: ['', [Validators.required, Validators.maxLength(100)]],
      description: [''],
      cluster_type: ['starrocks', [Validators.required]],
      fe_host: ['', [Validators.required]],
      fe_http_port: [8030, [Validators.required, Validators.min(1), Validators.max(65535)]],
      fe_query_port: [9030, [Validators.required, Validators.min(1), Validators.max(65535)]],
      username: ['root', [Validators.required]],
      password: ['', [Validators.required]],
      enable_ssl: [false],
      connection_timeout: [10, [Validators.min(1), Validators.max(300)]],
      catalog: ['default_catalog'],
      deployment_mode: ['shared_nothing', [Validators.required]],
      tags: [''],
      admin_user: [''],  // Admin user for permission execution (optional)
      admin_password: [''],  // Admin password (optional)
    });
  }

  ngOnInit(): void {
    // Determine if current user is super admin
    this.isSuperAdmin = this.authService.isSuperAdmin();
    
    // Check if user is organization admin
    const currentUser = this.authService.currentUserValue;
    this.isOrgAdmin = currentUser?.is_org_admin === true || false;

    // Load organizations and current organization
    this.loadOrganizationData();

    if (this.isEditMode) {
      this.loadCluster();
    } else {
      // In create mode, password is optional (allow empty password)
      this.clusterForm.get('password')?.clearValidators();
      this.clusterForm.get('password')?.updateValueAndValidity();
    }
  }
  
  private loadOrganizationData(): void {
    // Load organizations for super admin
    if (this.isSuperAdmin) {
      this.organizationsLoading = true;
      this.organizationService.listOrganizations().subscribe({
        next: (orgs) => {
          this.organizations = orgs;
          this.organizationsLoading = false;
          // Set organization_id as required for super admin
          this.clusterForm.get('organization_id')?.setValidators([Validators.required]);
          this.clusterForm.get('organization_id')?.updateValueAndValidity();
        },
        error: (error) => {
          ErrorHandler.handleHttpError(error, this.toastrService);
          this.organizationsLoading = false;
        },
      });
    } else {
      // Load current organization for org admin
      const currentUser = this.authService.currentUserValue;
      if (currentUser?.organization_id) {
        this.organizationService.getOrganization(currentUser.organization_id).subscribe({
          next: (org) => {
            this.currentOrganization = org;
            // Auto-set organization for org admin
            this.clusterForm.get('organization_id')?.setValue(org.id);
            this.clusterForm.get('organization_id')?.disable();
          },
          error: (error) => ErrorHandler.handleHttpError(error, this.toastrService),
        });
      }
    }
  }

  loadCluster(): void {
    if (!this.clusterId) return;

    this.loading = true;
    this.clusterService.getCluster(this.clusterId).subscribe({
      next: (cluster) => {
        this.clusterForm.patchValue({
          organization_id: cluster.organization_id,
          name: cluster.name,
          description: cluster.description,
          cluster_type: cluster.cluster_type || 'starrocks',
          fe_host: cluster.fe_host,
          fe_http_port: cluster.fe_http_port,
          fe_query_port: cluster.fe_query_port,
          username: cluster.username,
          enable_ssl: cluster.enable_ssl,
          connection_timeout: cluster.connection_timeout,
          catalog: cluster.catalog,
          deployment_mode: cluster.deployment_mode,
          tags: cluster.tags.join(', '),
          admin_user: cluster.admin_user || '',
        });
        // Password is not loaded for security
        this.clusterForm.get('password')?.clearValidators();
        this.clusterForm.get('password')?.updateValueAndValidity();
        this.loading = false;
      },
      error: (error) => {
        this.toastrService.danger(
          ErrorHandler.extractErrorMessage(error),
          '错误',
        );
        this.loading = false;
      },
    });
  }

  onSubmit(): void {
    if (this.clusterForm.invalid) {
      Object.keys(this.clusterForm.controls).forEach(key => {
        this.clusterForm.get(key)?.markAsTouched();
      });
      return;
    }

    this.saving = true;
    const formValue = this.clusterForm.value;
    
    // Parse tags
    const tags = formValue.tags
      ? formValue.tags.split(',').map((t: string) => t.trim()).filter((t: string) => t)
      : [];

    const clusterData: any = {
      ...formValue,
      tags,
      organization_id: this.clusterForm.get('organization_id')?.value,
    };

    // Remove password if in edit mode and password is empty
    if (this.isEditMode && !formValue.password) {
      delete clusterData.password;
    }

    // Only include admin_user fields if user is org admin or super admin
    if (!this.isSuperAdmin && !this.isOrgAdmin) {
      delete clusterData.admin_user;
      delete clusterData.admin_password;
    } else {
      // If admin_user is provided, admin_password is required
      if (clusterData.admin_user && !clusterData.admin_password) {
        this.toastrService.danger('管理用户密码不能为空', '错误');
        this.saving = false;
        return;
      }
      // If admin_user is empty, clear admin_password
      if (!clusterData.admin_user) {
        delete clusterData.admin_user;
        delete clusterData.admin_password;
      }
      // In edit mode, if admin_password is empty, don't send it (keep existing)
      if (this.isEditMode && !clusterData.admin_password) {
        delete clusterData.admin_password;
      }
    }

    const request$ = this.isEditMode && this.clusterId
      ? this.clusterService.updateCluster(this.clusterId, clusterData)
      : this.clusterService.createCluster(clusterData);

    request$.subscribe({
      next: (cluster) => {
        // For new cluster, test connection after creation
        if (!this.isEditMode && cluster.id) {
          this.testConnectionAfterCreate(cluster.id);
        } else {
          this.toastrService.success('集群更新成功', '成功');
          this.dialogRef.close(true);
        }
      },
      error: (error) => {
        this.toastrService.danger(
          ErrorHandler.extractErrorMessage(error),
          '错误',
        );
        this.saving = false;
      },
    });
  }

  private testConnectionAfterCreate(clusterId: number): void {
    this.clusterService.getHealth(clusterId).subscribe({
      next: (health) => {
        if (health.status === 'healthy') {
          this.toastrService.success('集群创建成功，健康检查通过', '成功');
        } else if (health.status === 'warning') {
          this.toastrService.warning('集群已创建，但健康检查发现问题。请检查配置', '警告');
        } else {
          this.toastrService.warning('集群已创建，但健康检查失败。请检查配置', '警告');
        }
        this.dialogRef.close(true);
      },
      error: () => {
        this.toastrService.warning('集群已创建，但健康检查失败。请检查配置', '警告');
        this.dialogRef.close(true);
      },
    });
  }

  onCancel(): void {
    this.dialogRef.close();
  }

  testConnection(): void {
    // Check required fields for new cluster
    if (!this.isEditMode) {
      const requiredFields = ['fe_host', 'fe_http_port', 'fe_query_port', 'username', 'password'];
      const missingFields = requiredFields.filter(field => !this.clusterForm.get(field)?.value);
      
      if (missingFields.length > 0) {
        this.toastrService.warning('请先填写完整的连接信息（FE地址、端口、用户名、密码）', '提示');
        return;
      }
    }

    this.testing = true;
    const formValue = this.clusterForm.value;
    
    if (!this.isEditMode) {
      // New cluster mode: test connection with connection details
      const testData = {
        fe_host: formValue.fe_host,
        fe_http_port: formValue.fe_http_port,
        fe_query_port: formValue.fe_query_port,
        username: formValue.username,
        password: formValue.password,
        enable_ssl: formValue.enable_ssl || false,
        catalog: formValue.catalog || 'default_catalog',
      };

      this.clusterService.testConnection(testData).pipe(timeout(30000)).subscribe({
        next: (health) => this.handleHealthCheckResult(health),
        error: (error) => this.handleHealthCheckError(error),
      });
    } else {
      // Edit mode: check health of existing cluster
      this.clusterService.getHealth(this.clusterId!).pipe(timeout(30000)).subscribe({
        next: (health) => this.handleHealthCheckResult(health),
        error: (error) => this.handleHealthCheckError(error),
      });
    }
  }

  private handleHealthCheckResult(health: any): void {
    if (health.status === 'healthy') {
      // For healthy status, show detailed checks
      const details = health.checks.map((c: any) => `✓ ${c.name}: ${c.message}`).join('\n');
      this.toastrService.success(`健康检查通过\n\n${details}`, '连接成功');
    } else if (health.status === 'warning') {
      // For warning status, highlight problematic checks
      const warnings = health.checks
        .filter((c: any) => c.status !== 'ok')
        .map((c: any) => `⚠ ${c.name}: ${c.message}`)
        .join('\n');
      this.toastrService.warning(`健康检查发现问题\n\n${warnings || '请检查集群配置'}`, '警告');
    } else {
      // For critical status, show error details
      const errors = health.checks
        .filter((c: any) => c.status === 'critical' || c.status === 'warning')
        .map((c: any) => `✗ ${c.name}: ${c.message}`)
        .join('\n');
      this.toastrService.danger(`健康检查失败\n\n${errors || '请检查集群配置'}`, '连接失败');
    }
    this.testing = false;
  }

  private handleHealthCheckError(error: any): void {
    const message = error?.name === 'TimeoutError'
      ? '连接超时（30秒无响应），请检查 FE 地址与端口是否可达'
      : ErrorHandler.extractErrorMessage(error);
    this.toastrService.danger(message, '错误');
    this.testing = false;
  }
}

