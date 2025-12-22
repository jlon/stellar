import { Component, OnInit, OnDestroy, Input, Output, EventEmitter } from '@angular/core';
import { Subject } from 'rxjs';
import { takeUntil } from 'rxjs/operators';
import { LocalDataSource } from 'ng2-smart-table';
import { PermissionRequestService } from '../../../../@core/data/permission-request.service';
import { DbUserPermissionDto } from '../../../../@core/data/permission-request.model';
import { NbToastrService } from '@nebular/theme';

/**
 * Standard Permission Dashboard Component
 * 我的权限 - 展示当前集群数据库用户的权限列表
 *
 * 设计说明：
 * 1. 权限来源于 OLAP 引擎（StarRocks/Doris），通过 SHOW GRANTS 查询
 * 2. 权限分类：
 *    - 角色权限 (ROLE): 用户被授予的角色
 *    - 全局权限 (GLOBAL/SYSTEM): 系统级别权限，如 BLACKLIST, NODE 等
 *    - 数据库权限 (DATABASE): 数据库级别权限
 *    - 表级权限 (TABLE): 表级别权限
 * 3. 权限只能通过"权限申请"流程撤销，不能直接删除
 */

interface PermissionRecord extends DbUserPermissionDto {
  risk_level?: 'low' | 'medium' | 'high';
}

@Component({
  selector: 'ngx-permission-dashboard-standard',
  templateUrl: './permission-dashboard-standard.component.html',
  styleUrls: ['./permission-dashboard-standard.component.scss'],
})
export class PermissionDashboardStandardComponent implements OnInit, OnDestroy {
  @Input() refresh$: Subject<void>;
  @Input() clusterId: number;
  @Output() revokePermission = new EventEmitter<PermissionRecord>();
  @Output() switchToRequest = new EventEmitter<{type: string, permission?: PermissionRecord}>();

  // 状态
  loading = false;
  permissions: PermissionRecord[] = [];
  filteredPermissions: PermissionRecord[] = [];
  source: LocalDataSource = new LocalDataSource();

  // ng2-smart-table 配置 - 仿照节点管理，使用标准编辑按钮查看详情
  settings = {
    mode: 'external',
    hideSubHeader: false,
    noDataMessage: '暂无权限数据',
    actions: {
      columnTitle: '操作',
      add: false,
      edit: true,  // 使用编辑按钮作为查看详情
      delete: false,
      position: 'right',
    },
    edit: {
      editButtonContent: '<i class="nb-search"></i>',
    },
    pager: {
      display: true,
      perPage: 15,
    },
    columns: {
      privilege_type: {
        title: '权限类型',
        type: 'string',
      },
      resource_type: {
        title: '资源类型',
        type: 'string',
      },
      resource_path: {
        title: '资源路径',
        type: 'string',
      },
      granted_role: {
        title: '授权角色',
        type: 'string',
        valuePrepareFunction: (cell: string) => {
          return cell || '直接授权';
        },
      },
      risk_level_display: {
        title: '风险等级',
        type: 'string',
      },
    },
  };

  // 统计
  stats = {
    totalRoles: 0,
    globalPermissions: 0,
    dbPermissions: 0,
    tablePermissions: 0,
  };

  // 选中的权限（用于详情展示）
  selectedPermission: PermissionRecord | null = null;
  showDetailDialog = false;

  // 角色权限详情
  rolePermissions: DbUserPermissionDto[] = [];
  loadingRolePermissions = false;

  private destroy$ = new Subject<void>();

  constructor(
    private permissionService: PermissionRequestService,
    private toastr: NbToastrService,
  ) {}

  ngOnInit(): void {
    if (this.refresh$) {
      this.refresh$.pipe(takeUntil(this.destroy$)).subscribe(() => {
        this.loadPermissions();
      });
    }
    this.loadPermissions();
  }

  ngOnDestroy(): void {
    this.destroy$.next();
    this.destroy$.complete();
  }

  /**
   * 加载权限列表
   */
  loadPermissions(): void {
    this.loading = true;

    this.permissionService.listMyDbPermissions().subscribe({
      next: (permissions: DbUserPermissionDto[]) => {
        this.permissions = this.enhancePermissions(permissions);
        this.updatePermissionsDisplay();
        this.calculateStats();
        this.loading = false;
      },
      error: (err) => {
        console.error('Failed to load permissions:', err);
        this.toastr.danger('加载权限列表失败', '错误');
        this.loading = false;
      },
    });
  }

  /**
   * 增强权限数据
   */
  private enhancePermissions(permissions: DbUserPermissionDto[]): PermissionRecord[] {
    return permissions.map(p => {
      const riskLevel = this.calculateRiskLevel(p.privilege_type, p.resource_type);
      return {
        ...p,
        risk_level: riskLevel,
        risk_level_display: this.getRiskLabel(riskLevel),
      };
    });
  }

  /**
   * 计算风险等级
   * 基于权限类型和资源范围判断风险
   */
  private calculateRiskLevel(privilege: string, resourceType: string): 'low' | 'medium' | 'high' {
    const highRiskPrivileges = ['DELETE', 'DROP', 'GRANT', 'ADMIN', 'NODE', 'BLACKLIST', 'ALL'];
    const mediumRiskPrivileges = ['INSERT', 'UPDATE', 'CREATE', 'ALTER', 'LOAD'];

    // 全局权限风险更高
    if (resourceType === 'GLOBAL' || resourceType === 'SYSTEM') {
      return 'high';
    }

    if (highRiskPrivileges.some(p => privilege.toUpperCase().includes(p))) {
      return 'high';
    }
    if (mediumRiskPrivileges.some(p => privilege.toUpperCase().includes(p))) {
      return 'medium';
    }
    return 'low';
  }

  /**
   * 计算统计数据
   *
   * 权限分类说明：
   * - 角色数：用户被授予的角色数量（resource_type === 'ROLE'）
   * - 全局权限：系统级别权限（resource_type === 'GLOBAL' 或 'SYSTEM' 或 'CATALOG'）
   * - 数据库权限：数据库级别权限（resource_type === 'DATABASE'）
   * - 表级权限：表级别权限（resource_type === 'TABLE'）
   */
  private calculateStats(): void {
    // 角色数量
    this.stats.totalRoles = this.permissions.filter(p =>
      p.resource_type === 'ROLE' || p.privilege_type === 'ROLE'
    ).length;

    // 全局权限（GLOBAL, SYSTEM, CATALOG 级别）
    this.stats.globalPermissions = this.permissions.filter(p =>
      p.resource_type === 'GLOBAL' ||
      p.resource_type === 'SYSTEM' ||
      p.resource_type === 'CATALOG'
    ).length;

    // 数据库权限
    this.stats.dbPermissions = this.permissions.filter(p =>
      p.resource_type === 'DATABASE'
    ).length;

    // 表级权限
    this.stats.tablePermissions = this.permissions.filter(p =>
      p.resource_type === 'TABLE'
    ).length;
  }

  /**
   * 更新权限列表显示
   */
  private updatePermissionsDisplay(): void {
    this.filteredPermissions = [...this.permissions];
    this.source.load(this.filteredPermissions);
  }

  /**
   * 编辑行事件 - 用于查看详情
   */
  onEditRow(event: any): void {
    const permission = event.data as PermissionRecord;
    this.viewPermissionDetail(permission);
  }

  /**
   * 查看权限详情
   */
  viewPermissionDetail(permission: PermissionRecord): void {
    this.selectedPermission = permission;
    this.showDetailDialog = true;
    this.rolePermissions = [];

    // 如果是角色类型，加载角色的具体权限
    if (this.isRolePermission) {
      this.loadRolePermissions(permission.resource_path);
    }
  }

  /**
   * 判断当前选中的权限是否是角色类型
   */
  get isRolePermission(): boolean {
    return this.selectedPermission?.resource_type === 'ROLE' ||
           this.selectedPermission?.privilege_type === 'ROLE';
  }

  /**
   * 加载角色的具体权限
   */
  private loadRolePermissions(roleName: string): void {
    this.loadingRolePermissions = true;
    this.permissionService.listRolePermissions(roleName).subscribe({
      next: (permissions) => {
        this.rolePermissions = permissions;
        this.loadingRolePermissions = false;
      },
      error: (err) => {
        console.error('Failed to load role permissions:', err);
        this.loadingRolePermissions = false;
      },
    });
  }

  /**
   * 关闭详情对话框
   */
  closeDetailDialog(): void {
    this.showDetailDialog = false;
    this.selectedPermission = null;
  }

  /**
   * 申请撤销权限
   * 跳转到权限申请页面，预填撤销信息
   */
  requestRevoke(permission: PermissionRecord): void {
    // 发送事件给父组件，切换到权限申请Tab并预填信息
    this.switchToRequest.emit({
      type: 'revoke_permission',
      permission: permission,
    });

    this.toastr.info(
      '请在权限申请页面填写撤销原因并提交',
      '跳转到权限申请'
    );
  }

  // 辅助方法
  getRiskLabel(risk: string): string {
    switch (risk) {
      case 'high': return '高风险';
      case 'medium': return '中风险';
      case 'low': return '低风险';
      default: return '低风险';
    }
  }

  getRiskColor(risk: string): string {
    switch (risk) {
      case 'high': return 'danger';
      case 'medium': return 'warning';
      case 'low': return 'success';
      default: return 'basic';
    }
  }

  /**
   * 获取权限类型说明
   */
  getPrivilegeTypeDescription(type: string): string {
    const descriptions: {[key: string]: string} = {
      'SELECT': '查询数据',
      'INSERT': '插入数据',
      'UPDATE': '更新数据',
      'DELETE': '删除数据',
      'CREATE': '创建对象',
      'DROP': '删除对象',
      'ALTER': '修改对象',
      'GRANT': '授权权限',
      'USAGE': '使用权限',
      'ROLE': '角色授权',
      'BLACKLIST': '黑名单管理',
      'NODE': '节点管理',
      'ADMIN': '管理员权限',
      'ALL': '所有权限',
    };
    return descriptions[type.toUpperCase()] || type;
  }

  /**
   * 获取权限详细描述（用于详情对话框）
   */
  getPrivilegeDescription(type: string): string {
    const descriptions: {[key: string]: string} = {
      'SELECT': '允许对指定资源执行 SELECT 查询操作，读取表中的数据。这是最基本的数据访问权限。',
      'INSERT': '允许向指定表中插入新数据。通常用于数据导入、ETL 作业等场景。',
      'UPDATE': '允许修改指定表中已存在的数据记录。需要谨慎授予，可能影响数据完整性。',
      'DELETE': '允许删除指定表中的数据记录。高风险操作，建议仅授予必要人员。',
      'CREATE TABLE': '允许在指定数据库中创建新表。',
      'CREATE VIEW': '允许在指定数据库中创建视图。',
      'CREATE DATABASE': '允许创建新的数据库。',
      'CREATE FUNCTION': '允许创建用户自定义函数（UDF）。',
      'CREATE MATERIALIZED VIEW': '允许创建物化视图，用于加速查询。',
      'DROP': '允许删除指定的数据库对象（表、视图等）。高风险操作，删除后数据不可恢复。',
      'ALTER': '允许修改指定对象的结构，如添加列、修改表属性等。',
      'GRANT': '允许将自己拥有的权限授予其他用户或角色。具有此权限的用户可以进行权限分发。',
      'USAGE': '允许使用指定的 Catalog 或资源。这是访问外部数据源的基础权限。',
      'ROLE': '表示用户被授予了一个角色，该角色包含一组预定义的权限。',
      'BLACKLIST': '允许管理 SQL 黑名单，可以添加或删除被禁止执行的 SQL 模式。这是系统管理权限，用于阻止特定的危险查询。',
      'NODE': '允许管理集群节点，包括添加、删除 BE/CN 节点等操作。这是集群管理权限。',
      'ADMIN': '管理员权限，拥有系统的完全控制能力。',
      'ALL': '拥有指定资源上的所有权限，等同于该资源的完全控制权。',
      'EXPORT': '允许导出表数据到外部存储。',
      'LOAD': '允许执行数据导入操作，如 Stream Load、Broker Load 等。',
      'IMPERSONATE': '允许模拟其他用户执行操作。高风险权限，需谨慎授予。',
      'OPERATE': '允许执行运维操作，如 KILL 查询、设置系统变量等。',
      'CREATE RESOURCE': '允许创建外部资源，如 Spark 资源、Hive 资源等。',
      'CREATE RESOURCE GROUP': '允许创建资源组，用于资源隔离和管理。',
      'CREATE GLOBAL FUNCTION': '允许创建全局函数。',
      'CREATE STORAGE VOLUME': '允许创建存储卷（存算分离模式）。',
      'FILE': '允许执行文件相关操作。',
      'PLUGIN': '允许管理插件。',
      'REPOSITORY': '允许管理备份仓库。',
    };
    return descriptions[type.toUpperCase()] || `${type} 权限允许执行相关操作。`;
  }

  /**
   * 获取权限使用场景
   */
  getPrivilegeUsage(type: string): string {
    const usages: {[key: string]: string} = {
      'SELECT': '数据分析、报表查询、BI 工具连接',
      'INSERT': '数据导入、ETL 作业、实时写入',
      'UPDATE': '数据修正、状态更新',
      'DELETE': '数据清理、过期数据删除',
      'DROP': '表结构重建、清理测试数据',
      'ALTER': '表结构变更、添加索引',
      'GRANT': '权限管理、团队权限分发',
      'USAGE': '访问外部 Catalog（如 Hive、Iceberg）',
      'BLACKLIST': '阻止危险 SQL、防止资源滥用',
      'NODE': '集群扩缩容、节点维护',
      'ADMIN': '系统管理、紧急运维',
      'ALL': '开发测试环境、管理员账户',
      'EXPORT': '数据备份、数据迁移',
      'LOAD': '批量数据导入、数据同步',
      'OPERATE': '查询管理、系统调优',
    };
    return usages[type.toUpperCase()] || '';
  }

  /**
   * 获取资源范围描述
   */
  getResourceScopeDescription(resourceType: string, resourcePath: string): string {
    switch (resourceType?.toUpperCase()) {
      case 'GLOBAL':
      case 'SYSTEM':
        return '整个集群范围，影响所有数据库和表';
      case 'CATALOG':
        return resourcePath === '*' ? '所有 Catalog' : `Catalog: ${resourcePath}`;
      case 'DATABASE':
        return resourcePath?.endsWith('.*') 
          ? `数据库 ${resourcePath.replace('.*', '')} 下的所有表`
          : `数据库: ${resourcePath}`;
      case 'TABLE':
        return `表: ${resourcePath}`;
      case 'ROLE':
        return `角色 ${resourcePath} 包含的所有权限`;
      default:
        return resourcePath || '未知范围';
    }
  }

  /**
   * 获取资源类型说明
   */
  getResourceTypeDescription(type: string): string {
    const descriptions: {[key: string]: string} = {
      'GLOBAL': '全局级别 - 影响整个集群',
      'SYSTEM': '系统级别 - 系统管理权限',
      'CATALOG': '目录级别 - 数据目录权限',
      'DATABASE': '数据库级别 - 特定数据库权限',
      'TABLE': '表级别 - 特定表权限',
      'ROLE': '角色 - 通过角色获得的权限',
    };
    return descriptions[type.toUpperCase()] || type;
  }
}
