import {
  Component,
  OnInit,
  OnDestroy,
  Input,
} from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { NbToastrService } from '@nebular/theme';
import { UserService, UserWithRoles, RoleSummary } from '../../../../../@core/data/user.service';

/**
 * SystemUsersComponent
 * Permission Config Tab 1: 系统用户管理 (System Users Management)
 *
 * Purpose:
 * - Display list of system users with their roles
 * - Show user details and role assignments
 * - Support role editing (future backend integration)
 *
 * Features:
 * - User list table with search and filtering
 * - User detail modal with role checkboxes
 * - Permission tree display (read-only)
 * - Role management interface
 */
@Component({
  selector: 'ngx-system-users',
  templateUrl: './system-users.component.html',
  styleUrls: ['./system-users.component.scss'],
})
export class SystemUsersComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;

  // State
  systemUsers: UserWithRoles[] = [];
  filteredUsers: UserWithRoles[] = [];
  usersLoading = false;
  searchTerm = '';

  // Modal state
  selectedUser: UserWithRoles | null = null;
  selectedUserEditCopy: UserWithRoles | null = null;

  // Role options
  availableRoles: RoleSummary[] = [];

  private destroy$ = new Subject<void>();

  constructor(
    private toastr: NbToastrService,
    private userService: UserService,
  ) {}

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$
        .pipe(takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadSystemUsers();
        });
    }
    this.loadAvailableRoles();
    this.loadSystemUsers();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load available roles for assignment
   */
  private loadAvailableRoles(): void {
    this.userService
      .listRoles()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (roles) => {
          this.availableRoles = roles;
        },
        error: (err) => {
          this.toastr.danger(
            err?.error?.message || '加载角色列表失败',
            '错误',
          );
        },
      });
  }

  /**
   * Load system users from backend API
   */
  private loadSystemUsers(): void {
    this.usersLoading = true;

    this.userService
      .listUsers()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (users) => {
          this.systemUsers = users;
          this.applyFilters();
          this.usersLoading = false;
        },
        error: (err) => {
          this.usersLoading = false;
          this.toastr.danger(
            err?.error?.message || '加载系统用户失败',
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
   * Apply search filters to users list
   */
  private applyFilters(): void {
    if (!this.searchTerm.trim()) {
      this.filteredUsers = [...this.systemUsers];
      return;
    }

    const term = this.searchTerm.toLowerCase();
    this.filteredUsers = this.systemUsers.filter(
      (user) =>
        user.username.toLowerCase().includes(term) ||
        user.email.toLowerCase().includes(term),
    );
  }

  /**
   * View user details
   */
  onViewDetail(user: UserWithRoles): void {
    this.selectedUser = user;
    this.selectedUserEditCopy = JSON.parse(JSON.stringify(user)); // Deep copy for editing
  }

  /**
   * Close detail modal
   */
  onCloseDetail(): void {
    this.selectedUser = null;
    this.selectedUserEditCopy = null;
  }

  /**
   * Toggle role for selected user
   */
  onToggleRole(roleId: number): void {
    if (!this.selectedUserEditCopy) {
      return;
    }

    const index = this.selectedUserEditCopy.roles.findIndex(r => r.id === roleId);
    if (index > -1) {
      this.selectedUserEditCopy.roles.splice(index, 1);
    } else {
      const role = this.availableRoles.find(r => r.id === roleId);
      if (role) {
        this.selectedUserEditCopy.roles.push(role);
      }
    }
  }

  /**
   * Save role changes
   */
  onSaveRoles(): void {
    if (!this.selectedUser || !this.selectedUserEditCopy) {
      return;
    }

    const roleIds = this.selectedUserEditCopy.roles.map(r => r.id);

    this.userService
      .updateUser(this.selectedUser.id, { role_ids: roleIds })
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: () => {
          this.toastr.success(
            `用户 ${this.selectedUser?.username} 的角色已更新`,
            '保存成功',
          );
          this.onCloseDetail();
          this.loadSystemUsers();
        },
        error: (err) => {
          this.toastr.danger(
            err?.error?.message || '更新用户角色失败',
            '错误',
          );
        },
      });
  }

  /**
   * Check if role is assigned to user
   */
  hasRole(roleId: number): boolean {
    return this.selectedUserEditCopy?.roles.some(r => r.id === roleId) || false;
  }
}
