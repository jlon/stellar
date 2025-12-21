import {
  Component,
  OnInit,
  OnDestroy,
  Input,
} from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbToastrService } from '@nebular/theme';
import { PermissionRequestService } from '../../../../../@core/data/permission-request.service';
import { DbRoleDto } from '../../../../../@core/data/permission-request.model';

/**
 * DbRolesComponent
 * Permission Config Tab 4: 数据库角色管理 (Database Roles Management)
 *
 * Purpose:
 * - Display database roles from different OLAP engines
 * - Show role information and associated permissions
 * - Read-only view of built-in roles
 *
 * Features:
 * - Role list with cluster selection
 * - Detailed view of role permissions
 */
@Component({
  selector: 'ngx-db-roles',
  templateUrl: './db-roles.component.html',
  styleUrls: ['./db-roles.component.scss'],
})
export class DbRolesComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;

  // State
  dbRoles: DbRoleDto[] = [];
  filteredRoles: DbRoleDto[] = [];
  rolesLoading = false;

  // Modal state
  selectedRole: DbRoleDto | null = null;

  private destroy$ = new Subject<void>();

  constructor(
    private toastr: NbToastrService,
    private permissionRequestService: PermissionRequestService,
  ) {}

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$
        .pipe(takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadDbRoles();
        });
    }
    this.loadDbRoles();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load database roles from backend API
   */
  private loadDbRoles(): void {
    this.rolesLoading = true;

    this.permissionRequestService
      .listDbRoles()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (roles) => {
          this.dbRoles = roles;
          this.filteredRoles = [...this.dbRoles];
          this.rolesLoading = false;
        },
        error: (err) => {
          this.rolesLoading = false;
          this.toastr.danger(
            err?.error?.message || '加载数据库角色失败',
            '错误',
          );
        },
      });
  }

  /**
   * View role details
   */
  onViewDetail(role: DbRoleDto): void {
    this.selectedRole = role;
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
  getRoleTypeLabel(roleType: string): string {
    return roleType === 'built-in' ? '内置' : '自定义';
  }
}
