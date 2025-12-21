import {
  Component,
  OnInit,
  OnDestroy,
  Input,
} from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbToastrService } from '@nebular/theme';
import { RoleService, RoleSummary, PermissionDto } from '../../../../../@core/data/role.service';

/**
 * Extended role info with members list
 */
export interface SystemRoleInfo extends RoleSummary {
  members?: string[];
  permissions?: PermissionDto[];
}

/**
 * SystemRolesComponent
 * Permission Config Tab 2: 系统角色管理 (System Roles Management)
 *
 * Purpose:
 * - Display list of system roles with their members and permissions
 * - Show role details, permissions, and member list
 * - Support role editing (future backend integration)
 *
 * Features:
 * - Role list table with search
 * - Role detail modal with permissions tree and members list
 * - Read-only display of built-in roles
 */
@Component({
  selector: 'ngx-system-roles',
  templateUrl: './system-roles.component.html',
  styleUrls: ['./system-roles.component.scss'],
})
export class SystemRolesComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;

  // State
  systemRoles: RoleSummary[] = [];
  filteredRoles: RoleSummary[] = [];
  rolesLoading = false;
  searchTerm = '';

  // Modal state
  selectedRole: SystemRoleInfo | null = null;
  roleDetailLoading = false;

  private destroy$ = new Subject<void>();

  constructor(
    private toastr: NbToastrService,
    private roleService: RoleService,
  ) {}

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$
        .pipe(takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadSystemRoles();
        });
    }
    this.loadSystemRoles();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load system roles from backend API
   */
  private loadSystemRoles(): void {
    this.rolesLoading = true;

    this.roleService
      .listRoles()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (roles) => {
          this.systemRoles = roles;
          this.applyFilters();
          this.rolesLoading = false;
        },
        error: (err) => {
          this.rolesLoading = false;
          this.toastr.danger(
            err?.error?.message || '加载系统角色失败',
            '错误',
          );
        },
      });
  }

  /**
   * Handle search term change
   */
  onSearchChange(): void {
    this.applyFilters();
  }

  /**
   * Apply search filters to roles list
   */
  private applyFilters(): void {
    if (!this.searchTerm.trim()) {
      this.filteredRoles = [...this.systemRoles];
      return;
    }

    const term = this.searchTerm.toLowerCase();
    this.filteredRoles = this.systemRoles.filter(
      (role) =>
        role.name.toLowerCase().includes(term) ||
        role.description.toLowerCase().includes(term),
    );
  }

  /**
   * View role details - fetch permissions
   */
  onViewDetail(role: RoleSummary): void {
    this.selectedRole = { ...role };
    this.roleDetailLoading = true;

    // Fetch role permissions
    this.roleService
      .getRolePermissions(role.id)
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (permissions) => {
          if (this.selectedRole) {
            this.selectedRole.permissions = permissions;
          }
          this.roleDetailLoading = false;
        },
        error: (err) => {
          this.roleDetailLoading = false;
          this.toastr.danger(
            err?.error?.message || '加载角色权限失败',
            '错误',
          );
        },
      });
  }

  /**
   * Close detail modal
   */
  onCloseDetail(): void {
    this.selectedRole = null;
  }

  /**
   * Get role type label
   */
  getRoleTypeLabel(isBuiltin: boolean): string {
    return isBuiltin ? '内置' : '自定义';
  }
}
