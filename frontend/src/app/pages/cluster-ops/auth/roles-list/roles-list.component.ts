import { Component, OnInit, OnDestroy, Input } from '@angular/core';
import { Observable, Subject } from 'rxjs';
import { takeUntil, startWith } from 'rxjs/operators';
import { NbToastrService } from '@nebular/theme';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { DbRoleDto } from '../../../../@core/data/permission-request.model';
import { ErrorHandler } from '../../../../@core/utils/error-handler';

/**
 * RolesListComponent
 * 数据库角色列表组件（只读）
 *
 * 功能：
 * - 显示当前集群的数据库角色列表
 * - 区分内置角色和自定义角色
 * - 实时查询，无本地缓存
 */
@Component({
  selector: 'ngx-roles-list',
  templateUrl: './roles-list.component.html',
  styleUrls: ['./roles-list.component.scss'],
})
export class RolesListComponent implements OnInit, OnDestroy {
  @Input() refresh$: Observable<void>;

  source: LocalDataSource = new LocalDataSource();
  loading = true;
  error: string | null = null;
  isEmpty = false;
  isNoCluster = false;

  private destroy$ = new Subject<void>();

  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: '暂无角色数据',
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
      role_name: {
        title: '角色名称',
        type: 'string',
      },
      role_type: {
        title: '角色类型',
        type: 'string',
        valuePrepareFunction: (value: string) => {
          return value === 'built-in' ? '内置角色' : '自定义角色';
        },
      },
    },
  };

  constructor(
    private permissionRequestService: PermissionRequestService,
    private toastrService: NbToastrService,
  ) {}

  ngOnInit(): void {
    this.loadRoles();

    if (this.refresh$) {
      this.refresh$
        .pipe(startWith(undefined), takeUntil(this.destroy$))
        .subscribe(() => {
          this.loadRoles();
        });
    }
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  loadRoles(): void {
    this.loading = true;
    this.error = null;
    this.isEmpty = false;
    this.isNoCluster = false;

    // Backend will determine active cluster from session context
    this.permissionRequestService.listDbRoles()
      .pipe(takeUntil(this.destroy$))
      .subscribe({
        next: (roles: DbRoleDto[]) => {
          if (!roles || roles.length === 0) {
            this.isEmpty = true;
            this.source.load([]);
          } else {
            this.isEmpty = false;
            this.source.load(roles);
          }

          this.loading = false;
        },
        error: (error) => {
          this.loading = false;

          if (error?.error?.message) {
            this.error = error.error.message;
          } else if (error?.message) {
            this.error = error.message;
          } else if (error?.status === 0) {
            this.error = '网络连接失败，请检查服务器是否正常运行';
          } else if (error?.status === 401) {
            this.error = '认证失败，请重新登录';
          } else if (error?.status === 403) {
            this.error = '您没有权限查看角色信息';
          } else if (error?.status === 500) {
            this.error = '服务器错误，请稍后重试';
          } else {
            this.error = `加载失败: ${error?.statusText || '未知错误'}`;
          }

          ErrorHandler.handleHttpError(error, this.toastrService);
        },
      });
  }

  retry(): void {
    this.error = null;
    this.isEmpty = false;
    this.loadRoles();
  }
}
