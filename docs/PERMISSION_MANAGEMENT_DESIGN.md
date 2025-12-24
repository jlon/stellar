# Stellar 权限管理系统设计文档

> **设计版本**: v1.1
> **最后更新**: 2025-12-23
> **设计者**: Claude + User
> **目标**: 支持 Doris + StarRocks 权限管理的统一界面
> **更新**: 添加管理用户配置，解决"自己给自己授权"的安全问题

---

## 📖 目录

1. [背景与目标](#背景与目标)
2. [权限模型概述](#权限模型概述)
3. [系统架构设计](#系统架构设计)
4. [数据模型设计](#数据模型设计)
5. [前端界面结构](#前端界面结构)
6. [权限工单流程](#权限工单流程)
7. [权限配置管理](#权限配置管理)
8. [管理用户配置](#管理用户配置)
9. [交互设计](#交互设计)

---

## 背景与目标

### 项目背景

Stellar 是一个多集群 OLAP 引擎管理平台，需要支持管理员对 **StarRocks** 和 **Apache Doris** 两种数据库的权限进行统一管理。

### 核心需求

- ✅ 支持权限工单（申请→审批→执行）全流程
- ✅ 管理用户、角色、权限的 RBAC 体系
- ✅ 查询数据库账户、角色的实时信息
- ✅ 支持多种权限粒度（全局、数据库、表、列）
- ✅ 支持集群隔离（不同集群的权限独立管理）

### 设计原则

1. **统一抽象** - 将 Doris 和 StarRocks 权限差异统一成通用模型
2. **最小化** - 只实现 MVP 所需的功能，避免过度设计
3. **可扩展** - 便于后续扩展支持其他 OLAP 引擎
4. **职责分离** - 工单管理和权限配置明确分界

---

## 权限模型概述

### Doris 权限模型

#### 权限层级

```
┌─────────────────────────────────────────────┐
│ 全局级别 (Global)                           │
│ ├─ Admin_priv  (超级管理员权限)            │
│ ├─ Node_priv   (节点管理权限)              │
│ ├─ Grant_priv  (权限授予权限)              │
│ └─ ...                                      │
├─────────────────────────────────────────────┤
│ 数据目录级别 (Catalog)                     │
│ ├─ Select_priv, Load_priv, Alter_priv     │
│ └─ ...                                      │
├─────────────────────────────────────────────┤
│ 数据库级别 (Database)                      │
│ ├─ 同 Catalog 权限                         │
│ └─ ...                                      │
├─────────────────────────────────────────────┤
│ 表级别 (Table)                              │
│ ├─ Select_priv, Load_priv, Alter_priv     │
│ └─ ...                                      │
├─────────────────────────────────────────────┤
│ 列级别 (Column)                             │
│ └─ Select_priv only                         │
├─────────────────────────────────────────────┤
│ 资源级别 (Resource)                        │
│ └─ Usage_priv                               │
└─────────────────────────────────────────────┘
```

#### 权限类型（9 种）

| 权限 | 说明 | 层级 |
|------|------|------|
| `Admin_priv` | 超级管理员权限 | 全局 |
| `Node_priv` | 节点管理权限（add/drop 节点）| 全局 |
| `Grant_priv` | 权限授予权限 | 全局、数据目录、数据库、表 |
| `Select_priv` | 查询权限 | Catalog、DB、Table、Column |
| `Load_priv` | 数据导入权限 | Catalog、DB、Table |
| `Alter_priv` | 修改权限 | Catalog、DB、Table |
| `Create_priv` | 创建权限 | Catalog、DB |
| `Drop_priv` | 删除权限 | Catalog、DB、Table |
| `Usage_priv` | 资源使用权限 | Resource |

#### 角色

- **内置角色**: `operator`（拥有 Admin_priv + Node_priv）、`admin`（拥有 Admin_priv）
- **自定义角色**: 任意命名，可以独立授权
- **模型**: RBAC，用户通过角色获得权限

---

### StarRocks 权限模型

#### 内置角色（5 种，不可变）

| 角色 | 职责 | 权限范围 |
|------|------|---------|
| `db_admin` | 数据库管理员 | 所有数据相关权限 + 基本运维权限 |
| `cluster_admin` | 集群管理员 | 节点管理权限 |
| `user_admin` | 用户管理员 | 用户、角色、权限管理 |
| `security_admin` | 安全管理员 | 安全配置和策略管理 |
| `public` | 公共角色 | 自动授予所有用户，默认无权限 |

#### 权限层级

```
系统级别 (SYSTEM)
  ├─ NODE              (节点管理)
  ├─ GRANT             (权限授予)
  ├─ CREATE RESOURCE GROUP
  ├─ CREATE EXTERNAL CATALOG
  ├─ PLUGIN
  └─ REPOSITORY

Catalog 级别
  ├─ USAGE             (访问)
  └─ CREATE DATABASE   (仅内部 Catalog)

数据库级别 (DATABASE)
  ├─ ALTER
  ├─ DROP
  ├─ CREATE TABLE
  ├─ CREATE VIEW
  └─ CREATE FUNCTION

表级别 (TABLE)
  ├─ SELECT
  ├─ INSERT
  ├─ UPDATE
  ├─ DELETE
  ├─ ALTER
  ├─ EXPORT
  └─ ALL

视图/物化视图级别
  ├─ SELECT
  ├─ ALTER            (仅视图)
  ├─ REFRESH          (仅物化视图)
  └─ DROP
```

#### 用户-角色-权限关系

```
User (最多 64 个角色)
  ├─ Role1 (default)  → Permissions
  ├─ Role2            → Permissions
  └─ RoleN

Role (可继承 16 级)
  ├─ Parent Role1
  ├─ Parent Role2
  └─ Direct Permissions
```

#### 特殊机制

- **WITH GRANT OPTION**: 允许用户将权限再授予给他人
- **默认角色**: 自动激活；可通过 SET ROLE 手动切换
- **权限继承**: 用户从所有角色继承权限（并集）

---

## 系统架构设计

### 整体架构图

```
┌──────────────────────────────────────────────────────────────┐
│ Frontend (Angular SPA)                                       │
├──────────────────────────────────────────────────────────────┤
│
│ 权限管理 (cluster-ops/permission-management)
│   │
│   ├─ Permission Workbench (权限工单)
│   │   ├─ My Requests      (我的申请)
│   │   └─ Approvals        (待审批)
│   │
│   └─ Permission Config (权限配置)
│       ├─ System Users     (系统用户)
│       ├─ System Roles     (系统角色)
│       ├─ DB Accounts      (数据库账户)
│       └─ DB Roles         (数据库角色)
│
└──────────────────────────────────────────────────────────────┘
         ↓↑ (HTTP/REST API)
┌──────────────────────────────────────────────────────────────┐
│ Backend (Rust / Actix-web)                                   │
├──────────────────────────────────────────────────────────────┤
│
│ API Layer
│   ├─ POST   /api/permission-requests           (提交申请)
│   ├─ GET    /api/permission-requests/my        (查询我的申请)
│   ├─ GET    /api/permission-requests/pending   (查询待审批)
│   ├─ POST   /api/permission-requests/:id/approve (批准)
│   ├─ POST   /api/permission-requests/:id/reject  (拒绝)
│   │
│   ├─ GET    /api/clusters/db-auth/accounts     (查询账户)
│   ├─ GET    /api/clusters/db-auth/roles        (查询角色)
│   └─ POST   /api/db-auth/preview-sql           (预览 SQL)
│
│ Business Logic Layer
│   ├─ PermissionRequestService    (工单管理)
│   ├─ DbAuthQueryService          (账户/角色查询)
│   ├─ PermissionSqlExecutor       (SQL 执行)
│   └─ ClusterService              (集群管理)
│
└──────────────────────────────────────────────────────────────┘
         ↓↑ (JDBC/MySQL Protocol)
┌──────────────────────────────────────────────────────────────┐
│ OLAP Engines                                                  │
├──────────────────────────────────────────────────────────────┤
│
│ StarRocks / Doris
│   ├─ SQL Execution    (CREATE USER, GRANT, etc.)
│   └─ Metadata Query   (SELECT * FROM information_schema.*)
│
└──────────────────────────────────────────────────────────────┘
```

### 菜单结构

```
集群运维 (cluster-ops)
│
├─ 概览
├─ 节点管理
├─ 查询管理
│
└─ 权限管理 ★ (权限统一菜单)
   │
   ├─ [我的权限]  (permission-dashboard)
   │  └─ 权限概览 + 权限清单 + 删除权限
   │
   ├─ [权限申请]  (permission-request)
   │  └─ 新建申请表单 + 我的申请列表
   │
   └─ [权限审批]  (permission-approval)
      └─ 待审批列表 + 批准/拒绝操作
```

**核心原则：** 权限的增删改全部通过申请工单流程，无直接修改接口。

---

## 管理用户配置

### 问题背景

在权限授权场景中，如果集群的连接用户（如 `starrocks`）与权限申请的目标用户相同，会出现"自己给自己授权"的逻辑问题。例如：
- 集群注册时使用 `starrocks` 用户连接
- 用户申请给 `starrocks` 用户授权
- 执行授权时使用 `starrocks` 用户执行 `GRANT ... TO starrocks`，等于自己给自己授权

### 解决方案

引入**管理用户**（Admin User）字段，实现权限执行用户与连接用户的分离：

- **连接用户**：用于集群连接、查询、监控等常规操作
- **管理用户**：专门用于执行权限授权操作（GRANT/REVOKE）

### 配置方式

1. **权限要求**：只有组织管理员或超级管理员可以配置管理用户字段
2. **字段位置**：集群创建/编辑表单的"高级选项"区域
3. **字段说明**：
   - `admin_user`：管理用户名（可选）
   - `admin_password`：管理用户密码（可选）
   - 管理用户和管理密码必须同时提供或同时为空

### 执行逻辑

权限授权操作（GRANT/REVOKE）的执行逻辑：

```
1. 如果配置了管理用户且密码不为空
   → 使用管理用户执行 SQL
2. 否则
   → 使用连接用户执行 SQL（向后兼容）
```

### 安全建议

1. **最佳实践**：为每个集群配置专门的管理用户（如 `stellar_admin`），与连接用户分离
2. **权限最小化**：管理用户只需要 GRANT 权限，不需要其他权限
3. **密码安全**：管理用户密码加密存储，与连接密码使用相同的加密方式

---

## 数据模型设计

### 1. 权限工单模型（已存在，需扩展）

#### PermissionRequest (数据库存储模型)

```typescript
interface PermissionRequest {
  // 基本信息
  id: number;
  cluster_id: number;
  applicant_id: number;
  applicant_org_id: number;

  // 申请内容
  request_type: 'create_account' | 'grant_role' | 'grant_permission';
  request_details: RequestDetails;  // JSON
  reason: string;
  valid_until?: DateTime;

  // 状态
  status: 'pending' | 'approved' | 'rejected' | 'executing' | 'completed' | 'failed';

  // 审批信息
  approver_id?: number;
  approval_comment?: string;
  approved_at?: DateTime;

  // 执行结果
  executed_sql?: string;
  execution_result?: string;
  executed_at?: DateTime;

  // 时间戳
  created_at: DateTime;
  updated_at: DateTime;
}
```

#### RequestDetails (申请详情 JSON)

```typescript
interface RequestDetails {
  action: 'create_account' | 'grant_role' | 'grant_permission';

  // 针对 create_account
  target_account?: string;  // e.g., "user@'%'" 或 "user@'192.168.%'"

  // 针对 grant_role / grant_permission
  target_user?: string;
  target_role?: string;

  // 权限范围
  scope: 'global' | 'database' | 'table' | 'column';
  database?: string;
  table?: string;
  column?: string;

  // 权限列表（根据数据库类型填充）
  permissions: string[];  // ['SELECT', 'INSERT', ...] 或 ['Select_priv', 'Load_priv']

  // StarRocks 特有
  with_grant_option?: boolean;

  // 生成的 SQL（预览）
  preview_sql?: string;
}
```

### 2. 用户模型扩展

#### SystemUser (系统用户)

```typescript
interface SystemUser {
  id: number;
  username: string;
  email: string;
  is_super_admin: boolean;

  // 组织隔离
  organization_id: number;

  // 角色
  roles: SystemRole[];  // 系统角色关联

  created_at: DateTime;
  updated_at: DateTime;
}
```

### 3. 角色模型扩展

#### SystemRole (系统角色)

```typescript
interface SystemRole {
  id: number;
  name: string;
  description: string;

  // 区分内置和自定义
  is_builtin: boolean;

  // 权限
  permissions: Permission[];

  // 成员
  users: SystemUser[];

  created_at: DateTime;
  updated_at: DateTime;
}
```

### 4. 权限模型

#### Permission (权限)

```typescript
interface Permission {
  id: number;
  code: string;        // 唯一标识，如 "api:db-auth:accounts:list"
  name: string;
  description: string;

  // 权限来源（用于区分系统权限 vs 数据库权限）
  source: 'system' | 'database';

  // 数据库权限的额外属性
  database_type?: 'doris' | 'starrocks';
  privilege_name?: string;  // Doris: Admin_priv, StarRocks: SELECT, etc.
  privilege_level?: 'global' | 'database' | 'table' | 'column';

  created_at: DateTime;
}
```

### 5. 数据库账户/角色模型

#### DbAccount (数据库账户，从数据库动态查询)

```typescript
interface DbAccount {
  account_name: string;
  host: string;
  roles: string[];  // 拥有的角色

  // 额外信息
  created_at?: DateTime;
  last_login?: DateTime;
}
```

#### DbRole (数据库角色，从数据库动态查询)

```typescript
interface DbRole {
  role_name: string;
  role_type: 'built-in' | 'custom';

  // 如果能查询到的话
  permissions?: DbPermission[];
  members?: DbAccount[];
}

interface DbPermission {
  privilege_name: string;
  privilege_level: 'global' | 'database' | 'table' | 'column';
  database?: string;
  table?: string;
}
```

---

## 前端界面设计

### Tab 1: 我的权限（Permission Dashboard）

**设计方案B：卡片仪表板**

```
┌──────────────────────────────────────────────────────┐
│ 我的权限                              [刷新] [申请权限] │
├──────────────────────────────────────────────────────┤
│                                                      │
│ 用户: analyst_user   [详细信息]                      │
│                                                      │
│ ┌────────┐ ┌────────┐ ┌────────┐ ┌──────────┐       │
│ │ 拥有角色 │ │ 全局权限 │ │ DB权限  │ │ Table权限 │       │
│ │   3个   │ │   2个   │ │  10个  │ │   15个   │       │
│ └────────┘ └────────┘ └────────┘ └──────────┘       │
│                                                      │
│ ┌──────────────────────────────────────────────────┐ │
│ │ 权限清单 (搜索) [过滤▼]                           │ │
│ ├──────────────────────────────────────────────────┤ │
│ │ 权限类型 │ 资源范围        │ 归属角色 │ 操作    │ │
│ ├──────────────────────────────────────────────────┤ │
│ │ USAGE    │ default_catalog │ db_admin │ [撤销]  │ │
│ │ SELECT   │ my_db.t1        │ db_admin │ [撤销]  │ │
│ │ INSERT   │ my_db.t1        │ db_admin │ [撤销]  │ │
│ │ ALTER    │ my_db           │ db_admin │ [撤销]  │ │
│ │ SELECT   │ public          │ user     │ [撤销]  │ │
│ └──────────────────────────────────────────────────┘ │
│                                                      │
└──────────────────────────────────────────────────────┘
```

**核心功能：**
- ✅ 展示当前用户的所有权限（按角色分组）
- ✅ 支持搜索和过滤权限
- ✅ **撤销权限按钮** → 跳转到权限申请，预填 `revoke_permission` 请求

---

### Tab 2: 权限申请（Permission Request）

**上半部分：新建申请表单**

```
┌────────────────────────────────────────────────────────┐
│ 新建权限申请                                           │
├────────────────────────────────────────────────────────┤
│                                                        │
│ * 申请类型  [授予角色 ▼]  (3个选项)                   │
│             ├─ 授予角色                               │
│             ├─ 授予权限                               │
│             └─ 撤销权限                               │
│                                                        │
│  ┌─────────────────────────────────────────────────┐  │
│  │ 表单内容根据申请类型动态变化                      │  │
│  │                                                   │  │
│  │ 选择用户:    [analyst_user ▼]                   │  │
│  │ + 创建新用户 (如无合适用户)                      │  │
│  │                                                   │  │
│  │ 选择角色:    [db_admin ▼]                      │  │
│  │ + 创建新角色 (如无合适角色)                      │  │
│  │                                                   │  │
│  │ 申请理由:    [请详细说明申请权限的理由...]       │  │
│  │                                                   │  │
│  │ SQL预览:                                         │  │
│  │ ┌────────────────────────────────────────────┐  │  │
│  │ │ GRANT db_admin TO analyst_user             │  │  │
│  │ └────────────────────────────────────────────┘  │  │
│  │                                                   │  │
│  │ [提交申请] [重置]                                │  │
│  └─────────────────────────────────────────────────┘  │
│                                                        │
├────────────────────────────────────────────────────────┤
│ 我的申请列表                                           │
├────────────────────────────────────────────────────────┤
│                                                        │
│ [搜索] [状态过滤: 全部▼]                              │
│                                                        │
│ ┌──────────────────────────────────────────────────┐  │
│ │ ID │ 申请类型  │ 目标    │ 状态  │ 创建时间   │ 操作│  │
│ ├──────────────────────────────────────────────────┤  │
│ │ 10 │ 授予角色  │ user1  │ 执行中│ 2025-12-20│[查看]│  │
│ │ 9  │ 授予权限  │ user2  │ 已完成│ 2025-12-19│[查看]│  │
│ │ 8  │ 撤销权限  │ user3  │ 待审批│ 2025-12-18│[查看]│  │
│ │ 7  │ 授予角色  │ user4  │ 已拒绝│ 2025-12-17│[查看]│  │
│ └──────────────────────────────────────────────────┘  │
│                                                        │
└────────────────────────────────────────────────────────┘
```

**表单动态化：**

| 申请类型 | 显示字段 | SQL生成 |
|---------|---------|--------|
| **授予角色** | 用户 / 角色 | GRANT role TO user |
| **授予权限** | 用户 / 资源类型 / Catalog / Database / Table / 权限列表 | GRANT perms ON resource TO user |
| **撤销权限** | 用户 / 资源类型 / Catalog / Database / Table / 权限列表 | REVOKE perms ON resource FROM user |

---

### Tab 3: 权限审批（Permission Approval）

```
┌────────────────────────────────────────────────────────┐
│ 待审批申请                                             │
├────────────────────────────────────────────────────────┤
│                                                        │
│ [搜索] [申请类型过滤: 全部▼]                          │
│                                                        │
│ ┌──────────────────────────────────────────────────┐  │
│ │ ID │ 申请类型  │ 申请人  │ 目标   │ 创建时间   │ 操作 │  │
│ ├──────────────────────────────────────────────────┤  │
│ │ 10 │ 授予角色  │ user5  │ user1  │ 2025-12-20 │[详情]│  │
│ │ 9  │ 撤销权限  │ user6  │ user2  │ 2025-12-19 │[详情]│  │
│ │ 8  │ 授予权限  │ user7  │ user3  │ 2025-12-18 │[详情]│  │
│ └──────────────────────────────────────────────────┘  │
│                                                        │
└────────────────────────────────────────────────────────┘
```

**点击"详情"弹窗：**

```
┌────────────────────────────────────────────────┐
│ 权限申请详情 (ID: 10)                          │
├────────────────────────────────────────────────┤
│                                                │
│ 申请人:     user5                              │
│ 申请类型:   授予角色                           │
│ 目标用户:   user1                              │
│ 目标角色:   db_admin                           │
│ 申请理由:   提升用户权限以完成数据分析任务     │
│ 创建时间:   2025-12-20 10:30:00               │
│                                                │
│ SQL预览:                                       │
│ ┌──────────────────────────────────────────┐  │
│ │ GRANT db_admin TO user1                  │  │
│ └──────────────────────────────────────────┘  │
│                                                │
│ 审批意见: [请输入审批意见...]                  │
│                                                │
│ [批准] [拒绝] [返回]                           │
│                                                │
└────────────────────────────────────────────────┘
```

---

### 目录结构

```
frontend/src/app/pages/cluster-ops/
└─ permission-management/               ★ 新增
   ├─ permission-management-routing.module.ts
   ├─ permission-management.module.ts
   ├─ permission-management.component.ts        (主容器，Tab管理)
   ├─ permission-management.component.html
   │
   ├─ dashboard/                                (我的权限)
   │  ├─ permission-dashboard.component.*
   │  └─ shared/
   │     └─ permission-list.component.*         (权限清单表格)
   │
   ├─ request/                                  (权限申请)
   │  ├─ permission-request.component.*
   │  ├─ request-form.component.*              (动态表单)
   │  └─ request-list.component.*
   │
   ├─ approval/                                 (权限审批)
   │  ├─ permission-approval.component.*
   │  └─ approval-detail-modal.component.*
   │
   └─ shared/                                   (共享组件)
      ├─ sql-preview.component.*               (SQL预览框)
      ├─ user-selector.component.*             (用户选择器)
      ├─ role-selector.component.*             (角色选择器)
      └─ resource-selector.component.*         (资源选择器)

核心服务：
└─ frontend/src/app/@core/data/
   ├─ permission-request.service.ts           (权限工单API)
   └─ permission-request.model.ts             (数据模型)
```

## 权限工单流程

### 工单类型（3种）

权限申请支持以下三种操作：

```
1. 授予角色 (grant_role)
   SQL: GRANT role_name TO user_name

2. 授予权限 (grant_permission)
   SQL: GRANT permission_list ON resource TO user_name

3. 撤销权限 (revoke_permission) ★ 新增
   SQL: REVOKE permission_list ON resource FROM user_name
```

### 工单生命周期

```
┌──────────┐
│ 创建申请 │  (applicant)
└────┬─────┘
     │ POST /api/permission-requests
     ↓
┌──────────────┐
│ Pending      │  等待审批
│ (waiting)    │
└────┬─────────┘
     │
     ├─ 审批通过 [POST /api/permission-requests/:id/approve]
     │   ↓
     │   ┌──────────┐
     │   │ Approved │
     │   └────┬─────┘
     │        │
     │        ↓
     │   ┌──────────────┐
     │   │ Executing    │  (执行 GRANT/REVOKE SQL)
     │   │ (executing)  │
     │   └────┬─────────┘
     │        │
     │        ├─ 成功 → ┌──────────┐
     │        │        │ Completed│
     │        │        └──────────┘
     │        │
     │        └─ 失败 → ┌────────┐
     │                  │ Failed │
     │                  └────────┘
     │
     └─ 拒绝 [POST /api/permission-requests/:id/reject]
         ↓
         ┌─────────┐
         │ Rejected│
         └─────────┘
```

### 支持的申请类型详解

#### 1. 授予角色 (grant_role)

```typescript
{
  request_type: 'grant_role',
  request_details: {
    action: 'grant_role',
    target_user: 'analyst_user',      // 被授权用户
    target_role: 'db_admin',           // 授予的角色
    preview_sql: 'GRANT db_admin TO analyst_user'
  },
  reason: '提升用户权限以完成数据分析任务'
}
```

**生成的SQL（需根据引擎适配）:**
- StarRocks: `GRANT db_admin TO analyst_user`
- Doris: `GRANT db_admin TO 'analyst_user'@'%'`

---

#### 2. 授予权限 (grant_permission)

```typescript
{
  request_type: 'grant_permission',
  request_details: {
    action: 'grant_permission',
    target_user: 'analyst_user',
    resource_type: 'database',         // Catalog / Database / Table / Column
    catalog: 'default_catalog',        // (optional)
    database: 'my_db',                 // (optional, depends on resource_type)
    table: 'my_table',                 // (optional, only for Table level)
    permissions: ['SELECT', 'INSERT'], // StarRocks: SELECT, INSERT, ...
                                       // Doris: Select_priv, Load_priv, ...
    preview_sql: 'GRANT SELECT, INSERT ON DATABASE my_db TO analyst_user'
  },
  reason: '授予数据查询和导入权限'
}
```

**生成的SQL:**
- StarRocks: `GRANT SELECT, INSERT ON DATABASE my_db TO analyst_user`
- Doris: `GRANT SELECT_PRIV, LOAD_PRIV ON DATABASE my_db TO 'analyst_user'@'%'`

---

#### 3. 撤销权限 (revoke_permission) ★ 新增

```typescript
{
  request_type: 'revoke_permission',
  request_details: {
    action: 'revoke_permission',
    target_user: 'analyst_user',
    resource_type: 'database',
    catalog: 'default_catalog',
    database: 'my_db',
    permissions: ['INSERT'],           // 撤销哪些权限
    preview_sql: 'REVOKE INSERT ON DATABASE my_db FROM analyst_user'
  },
  reason: '用户离职，撤销导入权限'
}
```

**生成的SQL:**
- StarRocks: `REVOKE INSERT ON DATABASE my_db FROM analyst_user`
- Doris: `REVOKE LOAD_PRIV ON DATABASE my_db FROM 'analyst_user'@'%'`

---

### 权限请求数据模型（扩展）

```typescript
interface PermissionRequest {
  // 基本信息
  id: number;
  cluster_id: number;
  applicant_id: number;
  applicant_org_id: number;

  // 申请内容
  request_type: 'grant_role' | 'grant_permission' | 'revoke_permission';
  request_details: RequestDetails;
  reason: string;

  // 状态
  status: 'pending' | 'approved' | 'rejected' | 'executing' | 'completed' | 'failed';

  // 审批信息
  approver_id?: number;
  approval_comment?: string;
  approved_at?: DateTime;

  // 执行结果
  executed_sql?: string;        // 实际执行的SQL
  execution_result?: string;    // 执行结果（success / error message）
  executed_at?: DateTime;

  created_at: DateTime;
  updated_at: DateTime;
}

interface RequestDetails {
  action: 'grant_role' | 'grant_permission' | 'revoke_permission';

  // 针对 grant_role / grant_permission / revoke_permission
  target_user: string;
  target_role?: string;         // 仅 grant_role 使用

  // 针对 grant_permission / revoke_permission
  resource_type: 'catalog' | 'database' | 'table' | 'column';
  catalog?: string;
  database?: string;
  table?: string;
  column?: string;
  permissions: string[];        // 权限列表

  // 预览 SQL
  preview_sql?: string;
}
```

### 数据流

```
前端表单
  ↓
PermissionRequestService.submitRequest()
  ↓
Backend POST /api/permission-requests
  ↓
PermissionRequestService.createRequest()  (保存到数据库)
  ↓
生成 preview_sql 和 execution_sql
  ↓
返回 PermissionRequestResponse
  ↓
前端显示申请详情 (含审批历史)
```

---

## 权限配置管理

### 系统用户管理

#### 界面

```
┌─────────────────────────────────────────────────┐
│ 系统用户                                        │
├─────────────────────────────────────────────────┤
│                                                 │
│ [搜索框] [过滤: 角色] [排序]                    │
│                                                 │
│ ┌──────────────────────────────────────────┐   │
│ │ ID  │ 用户名 │ 邮箱        │ 角色   │ 操作 │   │
│ ├──────────────────────────────────────────┤   │
│ │ 1   │ admin │ admin@...   │ admin  │ [详情]│   │
│ │ 2   │ user1 │ user1@...   │ user   │ [详情]│   │
│ └──────────────────────────────────────────┘   │
│                                                 │
└─────────────────────────────────────────────────┘
```

**点击"详情"** → 弹窗显示：

```
┌─────────────────────────────────────┐
│ 用户详情: admin                     │
├─────────────────────────────────────┤
│ 用户名: admin                       │
│ 邮箱: admin@example.com             │
│                                     │
│ 系统角色:                            │
│  ☑ admin                            │
│  ☑ user                             │
│                                     │
│ 权限树:                              │
│  [权限树展示，只读]                 │
│                                     │
│ [关闭] [编辑角色]                   │
└─────────────────────────────────────┘
```

### 系统角色管理

#### 界面

```
┌─────────────────────────────────────────────────┐
│ 系统角色                                        │
├─────────────────────────────────────────────────┤
│                                                 │
│ [搜索框]                                        │
│                                                 │
│ ┌──────────────────────────────────────────┐   │
│ │ 角色名 │ 描述         │ 成员数 │ 操作   │   │
│ ├──────────────────────────────────────────┤   │
│ │ admin  │ 管理员       │ 2    │ [详情]  │   │
│ │ user   │ 普通用户     │ 10   │ [详情]  │   │
│ └──────────────────────────────────────────┘   │
│                                                 │
└─────────────────────────────────────────────────┘
```

**点击"详情"** → 弹窗显示：

```
┌──────────────────────────────────────┐
│ 角色详情: admin                      │
├──────────────────────────────────────┤
│ 角色名: admin                        │
│ 描述: 系统管理员                     │
│ 是否内置: 是                         │
│                                      │
│ 权限:                                │
│  ☑ api:clusters:list                │
│  ☑ api:users:create                 │
│  ☑ api:roles:update                 │
│  ...                                 │
│                                      │
│ 成员:                                │
│  • admin (admin@example.com)        │
│  • superuser (su@example.com)       │
│                                      │
│ [关闭]                               │
└──────────────────────────────────────┘
```

### 数据库账户管理（只读）

#### 界面

```
┌──────────────────────────────────────────┐
│ 数据库账户 (my-doris)                    │
├──────────────────────────────────────────┤
│                                          │
│ 集群: my-doris  [切换集群]              │
│                                          │
│ ┌────────────────────────────────────┐  │
│ │ 账户名 │ 主机  │ 拥有角色 │ 操作  │  │
│ ├────────────────────────────────────┤  │
│ │ root   │ %     │ admin    │ [查看] │  │
│ │ admin  │ %.%.% │ user     │ [查看] │  │
│ └────────────────────────────────────┘  │
│                                          │
│ [刷新] [申请权限]                        │
│                                          │
└──────────────────────────────────────────┘
```

**点击"申请权限"** → 跳转到"权限工单 → 新建申请"，预填：
- 申请类型: grant_permission
- 目标账户: 已选中的账户

### 数据库角色管理（只读）

#### 界面

```
┌──────────────────────────────────────────┐
│ 数据库角色 (my-doris)                    │
├──────────────────────────────────────────┤
│                                          │
│ 集群: my-doris  [切换集群]              │
│                                          │
│ ┌────────────────────────────────────┐  │
│ │ 角色名  │ 角色类型 │ 权限数 │ 操作 │  │
│ ├────────────────────────────────────┤  │
│ │ admin   │ 内置     │ 15    │ [查看] │  │
│ │ user    │ 内置     │ 8     │ [查看] │  │
│ │ custom1 │ 自定义   │ 3     │ [查看] │  │
│ └────────────────────────────────────┘  │
│                                          │
│ [刷新]                                  │
│                                          │
└──────────────────────────────────────────┘
```

**点击"查看"** → 弹窗显示角色权限清单（只读）

---

## 交互设计

### 新建申请流程

```
[权限工单] → [+ 新建申请]
              ↓
        ┌──────────────────┐
        │ 选择申请类型     │
        │ ○ 创建账户      │
        │ ○ 授予角色      │ (初始页面)
        │ ○ 授予权限      │
        │ [下一步]         │
        └──────────────────┘
              ↓
        根据类型加载表单
              ↓
        ┌──────────────────────────────┐
        │ 创建账户申请                  │
        │                              │
        │ 账户名: [_________]          │
        │ 主机: [___________]          │
        │ 密码: [___________]          │
        │ 申请原因: [_____...]         │
        │ 申请有效期: [_______]        │
        │                              │
        │ SQL 预览:                     │
        │ ┌─────────────────────────┐  │
        │ │ CREATE USER ...         │  │
        │ └─────────────────────────┘  │
        │                              │
        │ [取消] [提交]               │
        └──────────────────────────────┘
              ↓
        提交到后端，返回申请 ID
              ↓
        显示"申请成功"提示 + 跳转到"我的申请"
              ↓
        列表显示新申请（pending 状态）
```

### 权限树组件

```typescript
// 将权限按层级组织，支持展开/折叠

权限树结构示例（Doris）:
├─ 全局权限
│  ├─ Admin_priv
│  ├─ Node_priv
│  └─ Grant_priv
├─ 数据库权限
│  ├─ Select_priv
│  ├─ Load_priv
│  └─ Alter_priv
└─ 表级权限
   ├─ Select_priv
   └─ ...

权限树结构示例（StarRocks）:
├─ 系统权限
│  ├─ NODE
│  ├─ GRANT
│  └─ CREATE RESOURCE GROUP
├─ 数据库权限
│  ├─ ALTER
│  ├─ DROP
│  └─ CREATE TABLE
└─ 表级权限
   ├─ SELECT
   ├─ INSERT
   ├─ UPDATE
   └─ DELETE
```

---

## 实现路线图

### Phase 1: 权限工单模块 (Week 1-2) ✅ COMPLETED

- [x] 创建新的菜单结构（permission-management 3-tab 设计）
- [x] 创建路由配置（permission-management-routing.module.ts）
- [x] 创建主容器组件（permission-management.component.ts/html）
- [x] 创建3个Tab组件占位符：
  - [x] PermissionDashboardComponent (我的权限)
  - [x] PermissionRequestComponent (权限申请)
  - [x] PermissionApprovalComponent (权限审批)
- [x] 更新 cluster-ops 路由（从 /auth 迁移到 /permission-management）

### Phase 2: 前端UI实现 (Week 1-2) ✅ COMPLETED

#### Tab 1: Permission Dashboard 我的权限
- [x] 统计卡片（拥有角色、全局权限、DB权限、表级权限）
- [x] 权限清单表格（权限类型、资源范围、资源路径、授予角色、授予时间）
- [x] 搜索和过滤功能
- [x] 撤销按钮（emit事件给parent）
- [x] 刷新按钮
- [x] 空状态和加载状态

#### Tab 2: Permission Request 权限申请
- [x] 动态表单（3种请求类型）：
  - [x] grant_role: 用户 + 角色
  - [x] grant_permission: 用户 + 资源类型 + 权限列表
  - [x] revoke_permission: 用户 + 资源类型 + 权限列表
- [x] 实时SQL预览生成
- [x] 表单验证和提交
- [x] 我的申请列表（状态过滤）
- [x] 状态标记和颜色编码

#### Tab 3: Permission Approval 权限审批（占位符）
- [ ] 待审批列表（filter）
- [ ] 详情弹窗（显示申请信息和SQL预览）
- [ ] 批准/拒绝操作
- [ ] 审批意见输入

- [ ] 创建系统用户管理页面（调用 `/api/users`）
- [ ] 创建系统角色管理页面（调用 `/api/roles`）
- [ ] 创建权限树组件（展示权限清单）
- [ ] 优化数据库账户和角色页面

### Phase 3: 后端 API 扩展 (Week 2-3, 并行)

- [ ] 实现 `/api/users` 端点（列表、详情）
- [ ] 实现 `/api/roles` 端点（列表、详情、权限）
- [ ] 扩展权限请求 SQL 生成逻辑
- [ ] 实现权限执行引擎

### Phase 4: UI/UX 优化 (Week 4)

- [ ] 统一设计语言和交互模式
- [ ] 添加空状态和加载状态
- [ ] 添加数据验证和错误提示
- [ ] 编写用户文档

---

## 总结

本文档设计了一个支持 **Doris + StarRocks** 权限管理的统一前端系统，核心包括：

1. **权限工单** - 申请→审批→执行 的完整流程
2. **权限配置** - 用户、角色、权限的 RBAC 管理
3. **数据库管理** - 实时查询和管理数据库账户和角色

设计遵循 **最小化原则**（只做 MVP），但保留了 **可扩展性**（便于支持更多 OLAP 引擎）。

