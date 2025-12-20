# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概览

Stellar 是一个企业级 OLAP 集群管理平台，支持 StarRocks 和 Apache Doris 的统一管理。

**技术栈**:
- Backend: Rust + Axum + SQLx (SQLite)
- Frontend: Angular 15 + Nebular UI
- 部署: Docker / Kubernetes / 传统部署

## 构建与开发命令

### 后端 (Rust)

```bash
# 开发环境 - 后端单独运行
cd backend
cargo check                    # 快速类型检查
cargo clippy                   # Lint 检查
cargo test                     # 运行所有测试
cargo test <test_name>         # 运行特定测试
cargo run                      # 开发模式运行

# 使用开发脚本启动后端（推荐）
scripts/dev/start_backend.sh   # 自动初始化 Rust 环境并启动

# 生产构建
cargo build --release          # 生产版本构建
```

### 前端 (Angular)

```bash
cd frontend
npm install                    # 安装依赖
npm start                      # 开发服务器 (http://localhost:4200)
npm run build                  # 开发构建
npm run build:prod             # 生产构建 (带 AOT)
npm run lint                   # ESLint 检查
npm run lint:fix               # 自动修复 lint 问题
npm test                       # 运行单元测试
npm run test:coverage          # 测试覆盖率报告
```

### 全栈构建与部署

```bash
# 完整构建（前端 + 后端，生成发布包）
make build                     # 构建并创建 dist/ 目录

# Docker 部署
make docker-build              # 构建 Docker 镜像
make docker-up                 # 启动容器
make docker-down               # 停止容器

# 清理
make clean                     # 清理所有构建产物
```

### 测试命令

```bash
# 后端测试
cd backend
cargo test                                    # 所有测试
cargo test --test integration_tests           # 集成测试
cargo test profile_analyzer                   # Profile 分析器测试
cargo test --lib services::cluster_service    # 特定模块测试

# 前端测试
cd frontend
npm test                                      # 单元测试
npm run test:coverage                         # 覆盖率报告
```

## 代码架构

### 后端架构 (backend/src/)

**分层设计**:
```
main.rs              # 应用入口，路由注册，中间件配置
├── handlers/        # HTTP handlers (thin layer)
├── services/        # 业务逻辑核心层
│   ├── cluster_service.rs          # 集群管理
│   ├── overview_service.rs         # 集群概览与监控
│   ├── metrics_collector_service.rs # 指标采集后台任务
│   ├── audit_log_service.rs        # 审计日志查询
│   ├── profile_analyzer/           # Query Profile 智能分析 ⭐
│   │   ├── parser/                 # Profile 文本解析
│   │   ├── analyzer/               # 规则引擎与诊断
│   │   │   ├── rule_engine.rs      # 规则评估引擎
│   │   │   ├── rules/              # 诊断规则库
│   │   │   │   ├── scan.rs         # 扫描算子规则 (S001-S003)
│   │   │   │   ├── join.rs         # Join 规则 (J001-J003)
│   │   │   │   ├── aggregate.rs    # 聚合规则 (A001-A002)
│   │   │   │   └── common.rs       # 通用规则 (G001-G003)
│   │   │   └── thresholds.rs       # 动态阈值配置
│   │   └── models.rs               # Profile 数据模型
│   ├── llm/                        # LLM 集成 (SQL 诊断)
│   └── cluster_adapter/            # 多 OLAP 引擎适配层
│       ├── doris_adapter.rs        # Apache Doris 适配器
│       └── starrocks_adapter.rs    # StarRocks 适配器
├── models/          # 数据库模型 (ORM entities)
├── middleware/      # 认证、权限、多租户中间件
├── db/              # 数据库连接与迁移
├── utils/           # 工具类 (JWT, 调度器等)
└── config.rs        # 配置加载 (conf/config.toml)
```

**核心特性**:
- **多租户架构**: Organization-based isolation (Casbin 权限控制)
- **多引擎支持**: 统一接口抽象 StarRocks 和 Doris
- **Profile 智能诊断**: 规则引擎 + 动态阈值 + LLM 增强
- **后台任务调度**: ScheduledExecutor (指标采集、基线刷新)

### 前端架构 (frontend/src/app/)

**模块组织**:
```
app/
├── @core/                    # 核心基础设施
│   ├── data/                 # 数据服务接口
│   └── mock/                 # Mock 数据 (开发用)
├── @theme/                   # UI 主题与布局
│   ├── components/           # 通用组件
│   ├── layouts/              # 布局模板
│   └── styles/               # 全局样式
├── auth/                     # 认证模块
├── pages/                    # 业务页面模块
│   ├── starrocks/            # StarRocks 管理页面
│   │   ├── cluster/          # 集群管理
│   │   ├── overview/         # 集群概览
│   │   ├── queries/          # 查询管理
│   │   │   └── profiles/     # Profile 可视化 ⭐
│   │   ├── backends/         # BE 节点管理
│   │   └── materialized-views/ # 物化视图
│   ├── system/               # 系统管理
│   │   ├── users/            # 用户管理
│   │   ├── roles/            # 角色管理
│   │   └── organizations/    # 组织管理
│   └── user-settings/        # 用户设置
└── app.module.ts             # 根模块
```

**关键组件**:
- **Profile 诊断可视化**: `pages/starrocks/queries/profiles/` - 展示 Profile 解析结果与智能诊断
- **集群概览**: `pages/starrocks/overview/` - ECharts 实时监控图表
- **多租户 UI**: Organization selector + 权限控制

### Profile 诊断系统详解

**这是系统的核心创新功能，需要重点理解**。

**工作流程**:
1. **解析阶段** (`parser/`): 文本 Profile → 结构化数据
2. **分析阶段** (`analyzer/`): 规则引擎评估 → 生成诊断建议
3. **LLM 增强** (`llm/`): 复杂场景调用 LLM 生成优化建议

**规则系统** (`backend/src/services/profile_analyzer/analyzer/rules/`):

| 规则分类 | 规则 ID | 检测内容 | 文件 |
|---------|---------|---------|------|
| 扫描算子 | S001 | 数据倾斜 (max/avg) | scan.rs |
| 扫描算子 | S002 | IO 倾斜 (IOTime 不均) | scan.rs |
| 扫描算子 | S003 | 过滤效率差 | scan.rs |
| Join 算子 | J001 | Join 结果爆炸 | join.rs |
| Join 算子 | J002 | Join 倾斜 | join.rs |
| Join 算子 | J003 | 大表驱动小表 | join.rs |
| 聚合算子 | A001 | Aggregation 倾斜 | aggregate.rs |
| 聚合算子 | A002 | 高基数聚合 | aggregate.rs |
| 通用规则 | G001 | 最耗时节点 | common.rs |
| 通用规则 | G003 | 执行时间倾斜 | common.rs |

**关键设计模式**:
- **3 层阈值保护**: 全局时间 (1s) → 绝对值 (500ms) → 动态阈值 (集群变量)
- **上下文感知**: RuleContext 携带集群配置、查询类型
- **规则间关系**: 抑制、依赖、互斥（见 `docs/profile/profile-diagnostic-system-review.md`）

**重要文档** (必读):
- `docs/profile/profile-diagnostic-system-review.md` - 深度架构审查与改进设计
- `docs/profile/development-progress.md` - 开发进度与任务分解
- `docs/profile/profile-metrics-reference.md` - Profile 指标字段说明

## 开发约定

### Rust 代码规范

1. **遵循 clippy 规则**: 所有 PR 必须通过 `cargo clippy --release --all-targets -- --deny warnings`
2. **错误处理**: 使用 `anyhow::Result` (业务逻辑) + `thiserror` (库错误类型)
3. **异步编程**: 优先 `async/await`，使用 `tokio::spawn` 处理并发
4. **模块化**: 功能按 `services/` 组织，保持单一职责
5. **测试**: 单元测试放 `#[cfg(test)] mod tests`，集成测试放 `tests/` 目录

### Angular 代码规范

1. **遵循 Angular Style Guide**: 组件命名 `*.component.ts`，服务命名 `*.service.ts`
2. **RxJS 最佳实践**: 订阅统一在 `ngOnDestroy` 取消，使用 `async` pipe 优先
3. **类型安全**: 接口定义放 `data/` 目录，与后端 API 同步
4. **i18n**: 硬编码文本使用 `translate` 管道（当前中文为主）

### Profile 规则开发注意事项

**修改规则前必须检查**:
1. 全局时间门槛: `MIN_DIAGNOSIS_TIME_SECONDS = 1.0` (rule_engine.rs)
2. 绝对值门槛: 样本数 ≥ 4, 时间 ≥ 500ms (thresholds.rs)
3. 动态阈值: 从 `ClusterVariables` 读取集群配置
4. 单元测试: 每条规则至少 3 个测试用例（正常/边界/保护）

**新增规则流程**:
```bash
1. 在 analyzer/rules/ 添加规则实现 (实现 DiagnosticRule trait)
2. 在 rule_engine.rs 注册规则
3. 在 tests.rs 添加单元测试
4. 更新 profile-diagnostic-system-review.md 文档
```

## 配置文件

**主配置**: `conf/config.toml`
```toml
[server]
port = 8080

[database]
url = "sqlite://data/stellar.db"

[metrics]
interval_secs = "30s"    # 指标采集间隔
retention_days = "7d"    # 数据保留期

[audit]
database = "starrocks_audit_db__"
table = "starrocks_audit_tbl__"
```

**环境变量覆盖**: `APP_METRICS_INTERVAL_SECS=1m`

## StarRocks 权限配置

**重要**: 添加集群前必须创建监控用户。

```bash
cd scripts/permissions
mysql -h <fe_host> -P 9030 -u root -p < setup_stellar_role.sql
```

详见: `scripts/permissions/README_PERMISSIONS.md`

## 常见开发任务

### 添加新的诊断规则

参考: `backend/src/services/profile_analyzer/analyzer/rules/scan.rs:S001DataSkew`

```rust
pub struct MyNewRule;

impl DiagnosticRule for MyNewRule {
    fn rule_id(&self) -> &'static str { "X001" }

    fn evaluate(&self, context: &RuleContext) -> Option<Diagnostic> {
        // 1. 检查全局/绝对值门槛
        if !context.meets_min_threshold() { return None; }

        // 2. 计算指标
        let metric = context.calculate_something();

        // 3. 动态阈值判断
        let threshold = context.cluster_variables.get("key")?;
        if metric > threshold {
            Some(Diagnostic { ... })
        } else {
            None
        }
    }
}
```

### 添加新的 API 端点

1. 在 `handlers/` 添加 handler 函数
2. 在 `main.rs` 注册路由
3. 在 `services/` 实现业务逻辑
4. 在 `models/` 定义数据模型
5. 更新 OpenAPI 文档 (`#[utoipa::path]`)

### 添加前端页面

1. 在 `pages/` 创建模块文件夹
2. 使用 `ng generate component pages/my-feature`
3. 在 `pages-routing.module.ts` 添加路由
4. 在 `pages-menu.ts` 添加菜单项
5. 创建对应的 service 调用后端 API

## 故障排查

### Rust 编译失败

```bash
# 检查 Rust 版本 (需 1.75+)
rustc --version

# 清理重建
cargo clean && cargo build
```

### 前端编译失败

```bash
# 清理 node_modules
rm -rf node_modules package-lock.json
npm install

# 清理 Angular 缓存
rm -rf .angular
```

### Profile 解析错误

- 检查 Profile 格式是否符合 StarRocks/Doris 标准
- 查看 `backend/src/services/profile_analyzer/parser/` 解析逻辑
- 开启 debug 日志: `RUST_LOG=stellar_backend::services::profile_analyzer=debug`

## 部署注意事项

1. **生产环境**: 修改 `conf/config.toml` 的 `jwt_secret`
2. **数据持久化**: 确保 `data/` 和 `logs/` 目录挂载
3. **权限配置**: 使用专用监控用户，禁止 root 账号
4. **指标采集**: 根据集群规模调整 `metrics.interval_secs`

## 相关文档

- API 文档: 启动后访问 `http://localhost:8080/swagger-ui/`
- 部署指南: `docs/deploy/DEPLOYMENT_GUIDE.md`
- 审计日志配置: `docs/AUDIT_LOG_CONFIG.md`
- 发布流程: `docs/RELEASE_PROCESS.md`
