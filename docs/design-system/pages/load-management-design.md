# 数据导入管理（Load Management）设计文档

> 状态：设计稿（未实施）
> 目标：对齐阿里云 EMR StarRocks Manager 的导入任务管理能力，落地为 Stellar 自托管双引擎（StarRocks/Doris）场景下的新一代导入运维页面。
> 关联：`docs/design-system/MASTER.md`（全部 UI 决策遵循本设计系统）。前端技术约束：Angular 21 + Nebular 17；参考已安装的 shadcn/ui、frontend-design、ui-ux-pro-max 的交互原则，但不引入 React/Radix/Tailwind 依赖。

---

## 1. 现状盘点（基于代码核实）

| 现有能力 | 位置 | 局限 |
|---|---|---|
| 树节点右键"查看导入作业" | `query-execution.component.ts` `viewLoads` | 已跳转到 `/pages/starrocks/loads?db=<db>`，由独立页面统一展示状态聚合、失败详情和阶段时间线 |
| 导入查询兼容层 | `backend/src/handlers/query.rs`、`cluster_adapter/*` | StarRocks/Doris 已有字段归一化和 Doris error hub 降级，但尚未形成独立 Load DTO/API |
| Doris `load_error_hub` 适配 | `cluster_adapter/doris.rs:56` | 已实现"聚合 SHOW LOAD 错误"的降级路径 |
| 系统函数入口 | `routine_loads / stream_loads / load_error_hub`（initial schema） | HTTP_QUERY 原始结果，面向开发者而非运维 |
| Profile DAG 渲染 | `profile-queries.component.ts`（dagre 0.8.5） | 依赖已在，布局经验可直接复用 |

**EMR 对标能力**（已核实官方文档）：导入任务可视化（进度/条数/字节数）、失败任务错误分析与原因定位、Stream Load profile（需内核开 `enable_load_profile`）、时间范围留存与过滤。

### 1.1 前端设计依据与技术边界

- **不安装 shadcn/ui**：当前 `frontend/` 没有 `components.json`，依赖是 Angular 21 + Nebular 17，没有 React、Radix 或 Tailwind。shadcn 的 `Card / Table / Badge / Alert / Progress / Sheet` 作为交互模型参考，不能直接作为实现依赖。
- **组件映射**：`Card → nb-card`，`Badge → nb-tag/nb-badge`，`Alert → nb-alert`，`Progress → nb-progress-bar`，`Sheet/Drawer → NbDialogService 的响应式全屏详情模板`，`Table → 语义化 HTML table + 现有页面表格样式`。
- **保留 shadcn 的有效原则**：语义色、组件组合、清晰的空态/加载态/错误态、可访问的标题与焦点、按钮状态不自定义造轮子；不照搬 React API、Tailwind class 或 Radix 的 `asChild` 约定。
- **现有 sibling 前端仅作兼容参考**：`/mnt/data/starrocks-admin` 是 Angular 15 + Nebular 11 的旧版前端，当前 `viewDatabaseLoads()` 通过 `information_schema.loads` 查询 100 条记录，再放进通用 `NbDialog + ng2-smart-table`。Stellar 当前已升级到 Angular 21 + Nebular 17，不能复制旧实现，只复用领域入口和用户习惯。
- **引擎源码作为字段真相**：`/mnt/data/starrocks-4.1.4` 的 `information_schema.loads` 与 `_statistics_.loads_history` 才是 StarRocks 字段、状态和历史留存的依据；页面不得根据示意图臆造字段。

## 2. 核心设计决策：任务详情用不用 DAG/流程图？

**结论：列表页不用 DAG；单个任务的"阶段时间线"用紧凑阶段条（stage bar），Routine Load 的错误样本分布保留表格。不引入拖拽画布、不画依赖图。**

理由（第一性原理）：

1. **数据形状决定表达，但不能把所有导入类型混为一谈**。Broker Load / Insert / Spark Load 可以从 `LOAD_START_TIME`、`LOAD_COMMIT_TIME`、`LOAD_FINISH_TIME` 和 `RUNTIME_DETAILS` 组成阶段条；Stream Load 的阶段来自 `RUNTIME_DETAILS` 中的 `begin_txn_ms / plan_time_ms / receive_data_time_ms / commit_publish_time_ms`；Routine Load 是常驻父作业，应展示消费位点和子任务，而不是强行套批处理阶段。无论哪种类型，都不是多分支依赖图。
2. **导入任务之间相互独立**，没有依赖关系，任务列表本身是"表格 + 状态徽章"的最优场景。EMR 也没有把任务列表画成图——业界没有先例。
3. **用户的真实诉求是三件事**：跑完没有？跑多快？为什么失败/慢？——分别对应状态徽章、阶段耗时对比、错误消息 + `TRACKING_SQL`/`REJECTED_RECORD_PATH`。三者都不需要 DAG。
4. 唯一"图有增量价值"的位置：**阶段耗时占比**。用一条水平分段进度条（stage bar）表达，比节点图信息密度更高、一眼可读，且移动端天然友好。

**保留的例外**：若未来支持多表导入编排（一个作业写 N 张目标表），届时目标表分支是真正的 DAG，再启用 dagre 画布（复用 profile 的渲染管线）。本期 YAGNI。

## 3. 信息架构

新增一级页面 `数据导入`（与查询管理平级），替代/收编现有树节点"查看导入作业"入口（入口保留，跳转到本页并预置库筛选）。

```
数据导入（/pages/starrocks/loads）
├── 概览条（4 个状态计数卡：运行中 / 排队 / 失败(24h) / 成功(24h)）
├── 工具栏
│   ├── 类型筛选：全部 | Broker Load | Stream Load | Routine Load | Insert | Spark/Flink
│   ├── 状态筛选：全部 | 运行中 | 已完成 | 已取消 | 失败 | 排队
│   ├── 库选择（默认跟随树节点上下文）
│   ├── 时间范围（默认近 24h，可选 7d / 30d；按目标引擎的 loads/history 视图下推时间条件）
│   ├── 搜索（Label / JobId）
│   └── 自动刷新（P1 为 30s 静默轮询）+ 手动刷新
├── 任务列表（语义化 table，遵循设计系统表格规范）
│   └── 行展开（expand）：可用时显示阶段时间线；Routine Load 显示消费概览；错误详情见 §4
└── 任务详情抽屉（点击行打开，宽 42rem，窄屏全屏）
```

## 4. 任务详情：阶段时间线（核心组件）

### 4.1 数据来源与映射

StarRocks 优先使用统一视图：

```sql
SELECT ID, LABEL, PROFILE_ID, DB_NAME, TABLE_NAME, USER, WAREHOUSE,
       STATE, PROGRESS, TYPE, PRIORITY,
       SCAN_ROWS, SCAN_BYTES, FILTERED_ROWS, UNSELECTED_ROWS, SINK_ROWS,
       RUNTIME_DETAILS, CREATE_TIME, LOAD_START_TIME, LOAD_COMMIT_TIME,
       LOAD_FINISH_TIME, PROPERTIES, ERROR_MSG, TRACKING_SQL,
       REJECTED_RECORD_PATH
FROM information_schema.loads
WHERE DB_NAME = ?
ORDER BY CREATE_TIME DESC
```

| 展示阶段 | 数据来源 | 说明 |
|---|---|---|
| 创建/排队 | `CREATE_TIME → LOAD_START_TIME` | 只有两个时间都存在时计算等待时长 |
| 执行 | `LOAD_START_TIME → LOAD_COMMIT_TIME` 或 `LOAD_FINISH_TIME` | 当前 P1 只使用真实时间戳；缺失时不估算 |
| 提交 | `LOAD_COMMIT_TIME → LOAD_FINISH_TIME` | 两个时间都存在且顺序有效时才绘制 |
| Runtime Details | `RUNTIME_DETAILS` 原文 | 当前 P1 原样展示，不将 JSON 内部字段猜测成阶段 |
| 数据质量 | `FILTERED_ROWS / UNSELECTED_ROWS / SINK_ROWS` | 这是指标，不是独立执行阶段 |

`information_schema.loads` 是 StarRocks 3.4+ 的统一运行视图；历史查询的目标表是 `_statistics_.loads_history`，不能误写成 `statistics.loads_history`，也不能把 `SHOW LOAD LIMIT 100` 当成完整历史方案。当前 P1 先查询 `information_schema.loads`，失败时才回退到按数据库执行 `SHOW LOAD`；历史表接入、保留期和游标分页列入后续迭代。页面只展示目标集群实际返回的字段。

Doris 当前通过统一查询优先读取 `information_schema.loads`，不可用时回退到按数据库执行 `SHOW LOAD`；后端只映射两者共有或已确认的字段。字段不存在就隐藏对应段，不显示虚假进度。`URL`、Routine Load 位点和 error hub 样本暂不在 P1 DTO 中强行补齐。

Routine Load：不做批处理阶段时间线，改为**消费位点卡片**（父作业状态、分区进度、lag、子任务数量、最近 `ReasonOfStateChanged`/错误样本）。

### 4.2 阶段条视觉（唯一"图"的形态）

```
排队 ██ 执行 ████████████████ 提交 ██ 完成 ███
    0s      12s                1s      8s    总 21s · 12.4M rows · 84.2 MB

数据质量：扫描 12.4M · 过滤 0.2% · 写入 12.2M
```

- 水平分段条，段宽=各阶段时长占比；悬停出 tooltip（阶段名/起止时刻/行数速率）
- 颜色语义：成功段用 primary，过滤段用 warning，失败任务整体置 danger 描边
- 失败任务：条终止于真实失败阶段，下方展开 `ERROR_MSG` 全文；有 `TRACKING_SQL` 时提供复制 SQL，有 `REJECTED_RECORD_PATH` 时提供复制路径/下载入口；Doris 的 `URL` 仅在确认是可访问错误地址时显示外链
- 高度 2.25rem，纯 div + flex 实现，**不用任何图库**；`prefers-reduced-motion` 下无动画

### 4.3 为什么不是 DAG（本节回答评审必问）

- 节点/边模型对线性流水线是强行降维：1 出度链的图布局 = 一条线，可读性反低于分段条
- 阶段间是时间连续（上一阶段结束时间=下一阶段开始时间），分段条天然表达"时间连续"；DAG 布局引擎（dagre）解决的是"空间离散节点防重叠"，本场景无此问题
- 分段条在 390px 手机屏直接可用；DAG 画布窄屏必须缩放/横向滚动，违反设计系统"不缩放页面"硬约束

## 5. 交互细节（响应式与人性化）

- **状态徽章**：Nebular badge，`success/info/warning/danger/basic` 对应成功/运行中/排队/失败/取消；运行中徽章带呼吸点动画（reduced-motion 下为静态）
- **自动刷新（P1）**：页面有活跃集群时每 30 秒静默刷新一次；请求进行中不会并发发起下一次刷新。更细粒度的运行中/排队自适应频率列入后续迭代。
- **详情侧板**：点击列表行或操作按钮打开详情侧板，展示字段、真实阶段、错误全文、`TRACKING_SQL`、拒绝记录路径和 `RUNTIME_DETAILS`；窄屏（≤640px）侧板占满宽度
- **失败优先排序**：默认排序 `失败 > 运行中 > 排队 > 已完成/取消`，组内按时间倒序——运维视角“先看坏消息”
- **错误原因归类（对齐 EMR 原因分析）**：后端按 ERROR_MSG 模式匹配归类，输出 `cause: 超时 / 超阈值 / 格式错误 / 权限 / 目标表不存在 / 资源不足 / 未知`，每类附一句处置建议（如“Scan bytes exceed threshold → 减小单次导入体量或调大 `broker_load_scan_bytes_threshold`”）；归类结果在详情抽屉错误框顶部渲染为结论行，无法识别时回退原文展示，不臆断
- **空态**：无任务时提示调整时间范围或清除筛选；树节点入口仍可直接带数据库筛选跳转
- **错误详情**：`ERROR_MSG` 等宽字体、可复制；StarRocks 的 `TRACKING_SQL` 用复制 SQL 按钮，`REJECTED_RECORD_PATH` 用复制路径/下载按钮；Doris 失败任务自动带出 error hub 聚合样本（后端已有 `get_load_errors_compromise`）
- **过滤行数提示（P1）**：`Filtered_Rows / Scan_Rows > 1%` 且分母有效时行内显示 warning；StarRocks 源码中 `SCAN_ROWS` 已包含正常、异常和未选中行，不能再次把 `FILTERED_ROWS` 加入分母。阈值和原因提示后续再接入集群变量
- **Stream Load profile**：P1 只展示可用的 `PROFILE_ID`/`RUNTIME_DETAILS` 原文，不自动修改集群变量；Profile 引导卡片列入后续迭代
- **键盘可达**：列表行可 Tab 聚焦、Enter 打开详情；抽屉 Esc 关闭；所有图标按钮带 `aria-label`

## 6. 后端设计

P1 已落地 `backend/src/services/load_service.rs` 与 `backend/src/handlers/load.rs`：

```rust
pub struct LoadJob {
    pub job_id: Option<String>,
    pub database: Option<String>,
    pub table_name: Option<String>,
    pub state: String,
    pub load_type: String,
    pub progress: Option<String>,
    pub scan_rows: Option<u64>,
    pub filtered_rows: Option<u64>,
    pub sink_rows: Option<u64>,
    pub load_start_time: Option<String>,
    pub load_commit_time: Option<String>,
    pub load_finish_time: Option<String>,
    pub error_msg: Option<String>,
    pub tracking_sql: Option<String>,
    pub rejected_record_path: Option<String>,
    pub stage_timeline: Vec<LoadStage>,
    pub failure_cause: Option<LoadFailureCause>,
}

/// 按 ERROR_MSG 关键模式归类；无法识别 → Unknown（前端回退原文，不臆断）
pub enum LoadFailureCause { Timeout, ThresholdExceeded, FormatError, PermissionDenied,
                            TargetMissing, ResourceExhausted, Unknown }

// LoadStage 只由真实时间戳生成；字段缺失时不估算。
```

- `GET /api/clusters/loads?db=&type=&state=&search=&range=&limit=`：查询当前组织活跃集群；默认查询最近 24 小时，服务端限制最多 500 条
- `GET /api/clusters/loads/{job_id}?db=`：按作业 ID 返回详情；详情查询使用 `range=all`，避免历史任务因默认时间范围被误判不存在
- 首选 `information_schema.loads`；查询失败时按数据库回退到 `SHOW LOAD`，缺失字段保持 `null`
- 阶段条只由 `CREATE_TIME`、`LOAD_START_TIME`、`LOAD_COMMIT_TIME`、`LOAD_FINISH_TIME` 计算；`RUNTIME_DETAILS` 当前作为原文保留，不把内部字段猜测成阶段
- 失败原因按 `ERROR_MSG` 关键词归类为 `Timeout`、`ThresholdExceeded`、`FormatError`、`PermissionDenied`、`TargetMissing`、`ResourceExhausted`、`Unknown`
- 当前复用已有 `api:clusters:queries` 权限；权限提取器将 `GET /api/clusters/loads*` 映射到该权限，不新增迁移，也不让旧角色突然失去访问
- 后续接入 `_statistics_.loads_history`、Routine Load 位点/error hub 和游标分页时，再拆分专用 adapter 方法；不为当前一条查询链预先增加抽象层

## 7. 前端组件划分

```
pages/starrocks/loads/
└── load-management.component.{ts,html,scss}  # 页面、工具栏、列表和响应式详情面板
```

- 路由 `path: 'loads'` 挂在 starrocks 模块，菜单项"导入任务"复用 `menu:queries:execution` 可见性
- 树节点 `viewLoads` 改为路由跳转携带 `?db=<db>` 预置筛选
- 遵循 MASTER.md 与 shadcn 的可组合组件原则：单主卡、图标化工具栏、Nebular ghost 按钮、`nb-alert/nb-progress-bar`、语义化 table、桌面表格/窄屏卡片式行布局、详情侧板焦点与 Esc 关闭、`prefers-reduced-motion` 兜底

## 8. 测试

- 后端：`backend/src/tests/load_service_test.rs` 覆盖失败原因分类、真实时间阶段计算、终态缺时间戳不造假、SQL 字面量转义/limit 和路由权限映射
- 前端：`npm run build` 验证路由、模板、Nebular 组件和响应式样式可编译；专项交互 spec 列入后续组件拆分阶段
- E2E 手册：对接真实集群造 1 个成功 + 1 个失败 Broker Load，核对 `RUNTIME_DETAILS` 原文、`TRACKING_SQL`、`REJECTED_RECORD_PATH` 与真实时间阶段；再验证一个 Routine Load 不被错误绘制成批处理阶段

## 9. 分期

| 期 | 内容 | 出口 |
|---|---|---|
| P1 | 列表页 + 状态聚合 + 详情侧板 + 条件阶段条 + 失败详情/追踪 SQL/拒绝记录路径 + StarRocks/Doris 查询回退 | 覆盖主要日常排障 |
| P2 | `_statistics_.loads_history` 历史查询、Routine Load 位点面板、error hub 样本和自适应刷新策略 | 常驻导入场景闭环 |
| P3 | SQL 文件树联动发起导入向导、导入→Profile 诊断跳转、多表编排 DAG（有真实需求再评估） | — |

## 10. 明确不做（YAGNI）

- 拖拽式导入编排画布、任务依赖图（无多阶段依赖数据源）
- 导入向导式建表+导表一体流（EMR 也未做，元数据 UI 建表已判低价值）
- 告警通知（归入平台级告警设计，不挂在导入页）
- 短信/电话通知渠道（自托管场景 webhook 已覆盖）
