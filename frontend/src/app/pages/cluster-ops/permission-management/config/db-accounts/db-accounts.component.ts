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
import { DbAccountDto } from '../../../../../@core/data/permission-request.model';

/**
 * DbAccountsComponent
 * Permission Config Tab 3: 数据库账户管理 (Database Accounts Management)
 *
 * Purpose:
 * - Display database accounts from different OLAP engines (StarRocks, Doris)
 * - Show account details and assigned roles
 * - Support permission request creation for accounts
 *
 * Features:
 * - Account list with cluster selection
 * - Detailed view of account information
 * - Request permission button for quick access to permission request form
 */
@Component({
  selector: 'ngx-db-accounts',
  templateUrl: './db-accounts.component.html',
  styleUrls: ['./db-accounts.component.scss'],
})
export class DbAccountsComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;

  // State
  dbAccounts: DbAccountDto[] = [];
  filteredAccounts: DbAccountDto[] = [];
  accountsLoading = false;

  // Modal state
  selectedAccount: DbAccountDto | null = null;

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
          this.loadDbAccounts();
        });
    }
    this.loadDbAccounts();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * Load database accounts from backend API
   */
  private loadDbAccounts(): void {
    this.accountsLoading = true;

    this.permissionRequestService
      .listDbAccounts()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (accounts) => {
          this.dbAccounts = accounts;
          this.filteredAccounts = [...this.dbAccounts];
          this.accountsLoading = false;
        },
        error: (err) => {
          this.accountsLoading = false;
          this.toastr.danger(
            err?.error?.message || '加载数据库账户失败',
            '错误',
          );
        },
      });
  }

  /**
   * View account details
   */
  onViewDetail(account: DbAccountDto): void {
    this.selectedAccount = account;
  }

  /**
   * Close detail modal
   */
  onCloseDetail(): void {
    this.selectedAccount = null;
  }

  /**
   * Request permission for account (would navigate to permission-request tab)
   */
  onRequestPermission(account: DbAccountDto): void {
    this.toastr.info(
      `请跳转到"权限申请"标签页，为账户 ${account.account_name} 申请权限`,
      '提示',
    );
    // TODO: Navigate to permission-request tab with account pre-filled
  }
}
