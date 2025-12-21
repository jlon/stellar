import { Component, Input, Output, EventEmitter } from '@angular/core';
import { Subject } from 'rxjs';

/**
 * PermissionDashboardComponent
 * 权限仪表板主组件 - 使用标准版本的权限撤销组件
 *
 * 通过标准组件实现：
 * - 权限列表展示
 * - 批量撤销功能
 * - 权限风险评估
 * - 使用情况统计
 * - 完全遵循 ngx-admin 原生样式
 */
@Component({
  selector: 'ngx-permission-dashboard',
  templateUrl: './permission-dashboard.component.html',
  styleUrls: ['./permission-dashboard.component.scss'],
})
export class PermissionDashboardComponent {
  @Input() refresh$: Subject<void>;
  @Output() revokePermission = new EventEmitter<any>();

  onRevoke(permission: any): void {
    this.revokePermission.emit(permission);
  }
}
