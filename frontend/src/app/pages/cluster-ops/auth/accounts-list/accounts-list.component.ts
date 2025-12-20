import { Component, OnInit, OnDestroy, Input } from '@angular/core';
import { Observable, Subject } from 'rxjs';
import { takeUntil, startWith } from 'rxjs/operators';
import { NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { DbAccountDto } from '../../../../@core/data/permission-request.model';
import { ErrorHandler } from '../../../../@core/utils/error-handler';

/**
 * AccountsListComponent
 * 数据库账户列表组件（只读）
 *
 * 功能：
 * - 显示当前集群的数据库账户列表
 * - 实时查询，无本地缓存
 */
@Component({
  selector: 'ngx-accounts-list',
  templateUrl: './accounts-list.component.html',
  styleUrls: ['./accounts-list.component.scss'],
})
export class AccountsListComponent implements OnInit, OnDestroy {
  @Input() refresh$: Observable<void>;

  source: LocalDataSource = new LocalDataSource();
  loading = true;
  error: string | null = null;
  isEmpty = false;
  isNoCluster = false;  // Flag when no cluster is selected

  private destroy$ = new Subject<void>();

  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: '暂无账户数据',
    actions: {
      columnTitle: '操作',
      add: false,
      edit: false,
      delete: false,
      position: 'right',
    },
    pager: {
      display: false,
    },
    columns: {
      account_name: {
        title: '账户名称',
        type: 'string',
      },
      host: {
        title: '主机',
        type: 'string',
      },
    },
  };

  constructor(
    private permissionRequestService: PermissionRequestService,
    private toastrService: NbToastrService,
  ) {}

  ngOnInit(): void {
    this.loadAccounts();

    // 监听刷新事件
    if (this.refresh$) {
      this.refresh$
        .pipe(startWith(undefined), takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadAccounts();
        });
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * 加载账户列表
   */
  loadAccounts(): void {
    this.loading = true;
    this.error = null;
    this.isEmpty = false;
    this.isNoCluster = false;

    // Backend will determine active cluster from session context
    this.permissionRequestService.listDbAccounts()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (accounts: DbAccountDto[]) => {
          // Check if data is empty
          if (!accounts || accounts.length === 0) {
            this.isEmpty = true;
            this.source.load([]);
          } else {
            this.isEmpty = false;
            this.source.load(accounts);
          }

          this.loading = false;
        },
        error: (error) => {
          this.loading = false;

          // Extract user-friendly error message
          if (error?.error?.message) {
            this.error = error.error.message;
          } else if (error?.message) {
            this.error = error.message;
          } else if (error?.status === 0) {
            this.error = '网络连接失败，请检查服务器是否正常运行';
          } else if (error?.status === 401) {
            this.error = '认证失败，请重新登录';
          } else if (error?.status === 403) {
            this.error = '您没有权限查看账户信息';
          } else if (error?.status === 500) {
            this.error = '服务器错误，请稍后重试';
          } else {
            this.error = `加载失败: ${error?.statusText || '未知错误'}`;
          }

          // Also show error toast
          ErrorHandler.handleHttpError(error, this.toastrService);
        },
      });
  }

  /**
   * 重试加载
   */
  retry(): void {
    this.error = null;
    this.isEmpty = false;
    this.loadAccounts();
  }
}
