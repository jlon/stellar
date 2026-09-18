# 数据导入管理（Load Management）设计文档

> 状态：设计稿（未实施）
> 目标：对齐阿里云 EMR StarRocks Manager 的导入任务管理能力，落地为 Stellar 自托管双引擎（StarRocks/Doris）场景下的新一代导入运维页面。
> 关联：`docs/design-system/MASTER.md`（全部 UI 决策遵循本设计系统）。前端技术约束：Angular 21 + Nebular 17；参考已安装的 shadcn/ui、frontend-design、ui-ux-pro-max 的交互原则，但不引入 React/Radix/Tailwind 依赖。

---

## 1. 现状盘点（基于代码核实）

| 现有能力 | 位置 | 局限 |
|---|---|---|
| 树节点右键"查看导入作业" | `query-execution.component.ts` `viewLoads` | 仅库级 `information_schema.loads` 通用表格（最多 100 条），无状态聚合、无失败分析、无自动刷新 |
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
- **引擎源码作为字段真相**：`/mnt/data/starrocks-4.1.4` 的 `information_schema.loads` 与 `statistics.loads_history` 才是 StarRocks 字段、状态和历史留存的依据；页面不得根据示意图臆造字段。

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
│   └── 自动刷新开关（运行中任务 5s 轮询，仅在有运行中任务时启用）+ 手动刷新
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
| 执行/接收 | `RUNTIME_DETAILS` | Broker/Insert 可读 ETL 时间；Stream 可读 `plan_time_ms`、`receive_data_time_ms` |
| 提交 | `LOAD_START_TIME → LOAD_COMMIT_TIME` 或 Stream 的 `commit_publish_time_ms` | 缺失时不绘制该段 |
| 完成 | `LOAD_COMMIT_TIME → LOAD_FINISH_TIME` | 只表达真实时间，不用估算补齐 |
| 数据质量 | `FILTERED_ROWS / UNSELECTED_ROWS / SINK_ROWS` | 这是指标，不是独立执行阶段 |

`information_schema.loads` 是 StarRocks 3.4+ 的统一运行视图；历史页优先查询 `statistics.loads_history`，不要依赖 `SHOW LOAD LIMIT 100`。StarRocks 4.1.4 源码中历史表还包含 `RUNTIME_DETAILS`、`TRACKING_SQL` 和 `REJECTED_RECORD_PATH`，页面只展示目标集群实际返回的字段。

Doris 继续通过 adapter 读取其 `SHOW LOAD` 字段（如 `EtlStartTime/EtlFinishTime/LoadStartTime/LoadFinishTime/URL`），在后端转换成相同的可选阶段 DTO。字段不存在就隐藏对应段，不显示虚假进度。

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
- **自动刷新**：仅当存在运行中/排队任务时默认开启（5s），全部终态自动停止并静默关闭开关——不制造"永远在转"的假忙碌
- **行展开 vs 抽屉**：列表行首 chevron 展开可用的阶段时间线（快速扫多任务）；点击行主体打开详情抽屉（深查：完整字段表、错误全文、`TRACKING_SQL`、拒绝记录路径）。窄屏（≤768px）无行展开，全部走响应式全屏详情模板
- **失败优先排序**：默认排序 `失败 > 运行中 > 排队 > 已完成/取消`，组内按时间倒序——运维视角“先看坏消息”
- **错误原因归类（对齐 EMR 原因分析）**：后端按 ERROR_MSG 模式匹配归类，输出 `cause: 超时 / 超阈值 / 格式错误 / 权限 / 目标表不存在 / 资源不足 / 未知`，每类附一句处置建议（如“Scan bytes exceed threshold → 减小单次导入体量或调大 `broker_load_scan_bytes_threshold`”）；归类结果在详情抽屉错误框顶部渲染为结论行，无法识别时回退原文展示，不臆断
- **空态**：无任务时给两条指引——"从表树右键发起数据预览/导入"与文档链接；筛选无结果给"清除筛选"内联动作
- **错误详情**：`ERROR_MSG` 等宽字体、可复制；StarRocks 的 `TRACKING_SQL` 用复制 SQL 按钮，`REJECTED_RECORD_PATH` 用复制路径/下载按钮；Doris 失败任务自动带出 error hub 聚合样本（后端已有 `get_load_errors_compromise`）
- **过滤行数提示**：`Filtered_Rows / Scan_Rows > 1%` 且分母有效时行内显示 warning 徽章 + tooltip（"过滤比例偏高，常见原因：分隔符不匹配/字段类型不兼容"）；分母缺失时不计算——把 EMR 的"失败原因分析"前移到"异常预警"
- **Stream Load profile**：后端新增"开启 enable_load_profile"引导卡片（检测到全局变量关闭时显示 SET GLOBAL 语句 + 一键复制），不代执行 DDL——遵循"不静默改用户集群配置"原则
- **键盘可达**：列表行可 Tab 聚焦、Enter 打开详情；抽屉 Esc 关闭；所有图标按钮带 `aria-label`

## 6. 后端设计

新增 `backend/src/services/load_service.rs`：

```rust
pub struct LoadJobSummary {   // 列表行
    pub job_id: String, pub label: String, pub db: String,
    pub load_type: LoadType,           // broker/stream/routine/insert/spark
    pub state: LoadState,              // 映射两引擎状态词表 → 统一枚举
    pub progress: Option<u8>,          // 百分比，routine 为消费 lag 派生
    pub rows_scanned: Option<u64>, pub rows_filtered: Option<u64>, pub rows_loaded: Option<u64>,
    pub created_at: Option<DateTime<Utc>>, pub finished_at: Option<DateTime<Utc>>,
    pub error_msg: Option<String>,
    pub tracking_sql: Option<String>, pub rejected_record_path: Option<String>,
    pub profile_id: Option<String>, pub runtime_details: Option<serde_json::Value>,
    pub cause: Option<LoadFailureCause>,   // 错误模式归类（对齐 EMR 原因分析）
}

/// 按 ERROR_MSG 关键模式归类；无法识别 → Unknown（前端回退原文，不臆断）
pub enum LoadFailureCause { Timeout, ThresholdExceeded, FormatError, PermissionDenied,
                            TargetMissing, ResourceExhausted, Unknown }

pub struct LoadStageTimeline {  // 详情-阶段条
    pub stages: Vec<LoadStage>,  // 只由真实时间或 runtime_details 生成；字段缺失→None
    pub source: StageSource,      // timestamps / runtime_details / unavailable
}

pub enum LoadState { Pending, Running, Finished, Cancelled, Failed }  // 两引擎词表统一
```

- `GET /api/clusters/{id}/loads?db=&type=&state=&search=&from=&to=&cursor=`：列表；StarRocks 活跃任务查 `information_schema.loads`，历史查 `statistics.loads_history`，时间条件下推到集群
- `GET /api/clusters/{id}/loads/{job_id}/timeline?db=`：从时间戳和 `RUNTIME_DETAILS` 生成阶段条；不可生成时返回 `unavailable`，前端展示指标而非空图
- `GET /api/clusters/{id}/loads/routine/{job_name}`：Routine Load 父作业、子任务、位点和错误样本
- 引擎差异收敛在 `cluster_adapter`（StarRocks 读取统一 loads 视图；Doris 使用 SHOW LOAD/已有 error hub 降级；`load_tracking_logs` 只作为错误追踪的可选能力，不作为所有版本的硬依赖）
- 复用现有 `MySQLClient` 会话池与权限模型：沿用集群级连接 + `menu:starrocks` 权限树下新增 `api:loads:list/detail` 权限点（对齐 op-audit 的权限播种模式，migration 按三方言各一份）

## 7. 前端组件划分

```
pages/starrocks/loads/
├── loads.component.{ts,html,scss}      # 页面骨架 + 工具栏 + 概览计数
├── load-table.component.{ts,html,scss}  # 语义化表格 + 行展开模板
├── load-stage-timeline.component.ts    # 阶段条（纯 div，独立组件便于复用与单测）
├── load-detail-drawer.component.ts     # 详情模板（字段表/错误/追踪 SQL）
└── load-routine-panel.component.ts     # Routine Load 位点面板
```

- 路由 `path: 'loads'` 挂在 starrocks 模块，菜单项"数据导入"（`menu:starrocks` 组）
- 树节点 `viewLoads` 改为路由跳转携带 `?db=<db>` 预置筛选
- 遵循 MASTER.md 与 shadcn 的可组合组件原则：单主卡、图标化工具栏、Nebular ghost 按钮、`nb-tag/nb-alert/nb-progress-bar`、语义化 table `table-layout: fixed` 桌面/自然宽窄屏、对话框标题与焦点、`prefers-reduced-motion` 兜底

## 8. 测试

- 后端：`load_service` 状态词表映射（StarRocks/Doris 两套→统一枚举，含未知状态兜底）、阶段时间线字段宽松映射（缺时间戳字段/乱序时间）、分页截断边界——`#[cfg(test)] mod tests` + `tests/` 集成（三方言迁移通过）
- 前端：阶段条渲染（0 阶段/单阶段/全阶段/缺字段）、状态徽章映射、失败排序稳定性——spec 文件
- E2E 手册：对接真实集群造 1 个成功 + 1 个失败 Broker Load，核对 `RUNTIME_DETAILS` 阶段、`TRACKING_SQL`、`REJECTED_RECORD_PATH` 与阶段时长；再验证一个 Routine Load 不被错误绘制成批处理阶段

## 9. 分期

| 期 | 内容 | 出口 |
|---|---|---|
| P1 | 列表页 + 状态聚合 + 详情抽屉 + 条件阶段条（Broker/Stream/Insert）+ 失败详情/追踪 SQL/拒绝记录路径 | 覆盖主要日常排障 |
| P2 | Routine Load 位点面板 + 自动刷新策略 + 过滤比例预警 | 常驻导入场景闭环 |
| P3 | SQL 文件树联动发起导入向导、导入→Profile 诊断跳转、多表编排 DAG（有真实需求再评估） | — |

## 10. 明确不做（YAGNI）

- 拖拽式导入编排画布、任务依赖图（无多阶段依赖数据源）
- 导入向导式建表+导表一体流（EMR 也未做，元数据 UI 建表已判低价值）
- 告警通知（归入平台级告警设计，不挂在导入页）
- 短信/电话通知渠道（自托管场景 webhook 已覆盖）
