# 数据导入管理（Load Management）设计文档

> 状态：P1 已实施；P2 的历史视图、游标分页、Routine Load 位点/子任务面板和 Doris 已选失败作业原始诊断已实施；P3a 的 StarRocks 本地 CSV/JSON、无凭据 HDFS/挂载 NAS 和 plaintext Kafka 外部导入已实施，其余外部数据源等待受管连接能力
> 目标：对齐阿里云 EMR StarRocks Manager 的导入任务管理能力，落地为 Stellar 自托管双引擎（StarRocks/Doris）场景下的新一代导入运维页面。本文件同时记录已实现边界，避免设计稿与实际行为漂移。
> 关联：`docs/design-system/MASTER.md`（全部 UI 决策遵循本设计系统）。前端技术约束：Angular 21 + Nebular 17；参考已安装的 shadcn/ui、frontend-design、ui-ux-pro-max 的交互原则，但不引入 React/Radix/Tailwind 依赖。

---

## 1. 现状盘点（基于代码核实）

| 现有能力 | 位置 | 局限 |
|---|---|---|
| 树节点右键"查看导入作业" | `query-execution.component.ts` `viewLoads` | 已跳转到 `/pages/starrocks/loads?db=<db>`，由独立页面统一展示状态聚合、失败详情和阶段时间线 |
| 导入查询兼容层 | `backend/src/handlers/query.rs`、`cluster_adapter/*` | 保留系统函数原始查询兼容；Load Management 已使用独立 DTO/API，不连接该链路 |
| Doris `load_error_hub` 适配 | `cluster_adapter/doris.rs:56` | 已实现"聚合 SHOW LOAD 错误"的系统函数降级；不作为 Load Management 数据源 |
| 系统函数入口 | `routine_loads / stream_loads / load_error_hub`（initial schema） | HTTP_QUERY 原始结果，面向开发者而非运维 |
| Profile DAG 渲染 | `profile-queries.component.ts`（dagre 0.8.5） | 依赖已在，布局经验可直接复用 |

**EMR 对标能力**（已核实官方文档）：导入任务可视化（进度/条数/字节数）、失败任务错误分析与原因定位、Stream Load profile（需内核开 `enable_load_profile`）、时间范围留存与过滤。

### 1.1 前端设计依据与技术边界

- **不安装 shadcn/ui**：当前 `frontend/` 没有 `components.json`，依赖是 Angular 21 + Nebular 17，没有 React、Radix 或 Tailwind。shadcn 的 `Card / Table / Badge / Alert / Progress / Sheet` 作为交互模型参考，不能直接作为实现依赖。
- **组件映射**：`Card → nb-card`，`Badge → nb-badge`，`Alert → nb-alert`，`Progress → nb-progress-bar`，`Sheet/Drawer → fixed aside + backdrop`，`Table → 语义化 HTML table + 现有页面表格样式`。
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

### 3.1 产品对象：外部导入

导入页只处理外部数据进入 StarRocks，不处理库内表搬运。创建流始终按以下顺序组织：

```text
外部数据源 → 解析/映射 → StarRocks 目标表 → 执行方式 → 运行观测
```

- **外部数据源**：本地文件、HDFS、挂载 NAS、Kafka、对象存储、数据湖、外部数据库或外部计算运行时；本期只开放不接收凭据的本地文件、无凭据 HDFS/BE(CN) 挂载目录和 CSV/JSON plaintext Kafka，其余来源须先具备受管连接、凭据托管和生命周期能力。
- **解析/映射**：只收集当前来源真正需要的格式、分隔符、Label 和表选择，不把所有引擎参数堆到一个表单里。
- **StarRocks 目标表**：提交前必须明确数据库和目标表；当前本地文件写入语义为追加，不隐含覆盖或删除。
- **执行方式**：用户按外部来源选择，系统据来源选择 Stream Load、Broker Load、Pipe、Routine Load、`FILES()`、Catalog + `INSERT INTO ... SELECT` 或外部 Connector；底层协议不是一级导航。
- **运行观测**：创建成功后回到任务列表；列表只回答任务是否完成、正在运行或失败，不重复承担创建表单的解释职责。

```
数据导入（/pages/starrocks/loads）
├── 任务态势摘要（当前筛选范围，不是集群 KPI）
│   ├── 有失败：优先提示失败数和处置方向，再补充运行中/排队数
│   ├── 无失败但有执行任务：提示仍在处理的任务数及运行中/排队分布
│   └── 无待处理任务：安静显示“当前没有待处理任务，N 个任务已完成”
├── 工具栏
│   ├── 类型筛选：全部 | Broker Load | Stream Load | Routine Load | Insert | Spark Load（Flink 字段未确认，不在 P1 展示）
│   ├── 状态筛选：全部 | 运行中 | 已完成 | 已取消 | 失败 | 排队
│   ├── 库选择（默认跟随树节点上下文）
│   ├── 时间范围（默认近 24h，可选 7d / 30d；按目标引擎的 loads/history 视图下推时间条件）
│   ├── 搜索（Label / JobId）
│   └── 手动刷新
├── 任务列表（ngx-admin 原生 `angular2-smart-table`，容器内横向滚动）
├── 新建外部导入（页头单一入口，打开后在同一窗口内选来源类型并配置目标表）
│   ├── 本地 CSV / JSON → 目标表（StarRocks Stream Load，已实施）
│   ├── 无凭据 HDFS / 挂载 NAS → 目标表（StarRocks Broker Load，已实施）
│   ├── plaintext Kafka Topic → 目标表（StarRocks Routine Load，已实施）
│   └── 对象存储、认证 HDFS/Kafka、数据湖、外部数据库 / 外部运行时（无表单入口，需先具备受管连接能力）
└── 任务详情 Sheet（点击行通过 Nebular `NbDialog` 从右侧打开，显示真实阶段、错误全文和复制入口）
```

任务态势摘要只回答“当前筛选范围内，是否存在需要我处理或持续关注的导入任务”。它不是吞吐、成功率或集群全局实时监控看板，因此不能把运行中、排队、失败和已完成等权排成四个 KPI。时间范围和更新时间放在页头，摘要将失败置于最高优先级：仅失败使用 `nb-alert`，运行中与正常状态使用紧凑文本；筛选栏负责定位，任务列表负责排障。

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

`information_schema.loads` 是 StarRocks 3.4+ 的统一运行视图；历史查询的目标表是 `_statistics_.loads_history`，不能误写成 `statistics.loads_history`，也不能把 `SHOW LOAD LIMIT 100` 当成完整历史方案。P2 对 StarRocks 合并运行视图和历史表，并以服务端下发的 `(CREATE_TIME, ID)` 游标继续翻页；历史视图不可用时自动降级回运行视图，再降级到按数据库执行 `SHOW LOAD`。页面只展示目标集群实际返回的字段。

Doris 当前通过统一查询优先读取 `information_schema.loads`，不可用时回退到按数据库执行 `SHOW LOAD`；后端只映射两者共有或已确认的字段。字段不存在就隐藏对应段，不显示虚假进度。详情页仅对已选失败作业，以已知数据库和精确 `LABEL` 查询 `SHOW LOAD`，再严格匹配 `JobId`，原样返回该行的 `URL`、`ErrorMsg`、`JobDetails`；不遍历数据库、不暴露同 Label 作业，也不连接全局 error hub。

Routine Load：不做批处理阶段时间线，改为**消费位点卡片**（父作业状态、分区进度、lag、子任务数量、最近 `ReasonOfStateChanged`/错误样本）。

### 4.2 阶段详情（原生列表）

- 列表只显示最后一个真实阶段及总耗时，避免在 ngx-admin Smart Table 内重绘图形。
- 详情 Sheet 使用原生 `nb-list` 展示阶段名和时长；行数速率、起止时间等引擎未确认字段不臆造。
- 失败任务在详情 Sheet 展示 `ERROR_MSG` 全文；有 `TRACKING_SQL` 时提供复制 SQL，有 `REJECTED_RECORD_PATH` 时提供复制路径；Doris `SHOW LOAD` 的 `URL`、`ErrorMsg`、`JobDetails` 保持原文并提供复制，不将原始 URL 自动当作外链访问。
- 不引入图库、手写阶段条或阶段独立动画。

### 4.3 为什么不是 DAG（本节回答评审必问）

- 节点/边模型对线性流水线是强行降维：1 出度链的图布局 = 一条线，原生阶段列表可读性更高
- DAG 布局引擎（dagre）解决的是"空间离散节点防重叠"，本场景无此问题
- 原生列表在 390px 手机屏可用；DAG 画布窄屏必须缩放或横向滚动，违反设计系统"不缩放页面"硬约束

## 5. 交互细节（响应式与人性化）

- **状态徽章**：Smart Table 使用 ngx-admin 现有的 `badge-*` 状态样式，`success/info/warning/danger/basic` 对应成功/运行中/排队/失败/取消；不添加独立动画。
- **刷新**：只在首次加载、筛选变化和用户点击刷新时请求数据；不在页面停留期间自动轮询。
- **详情 Sheet**：点击原生 Smart Table 行通过 Nebular `NbDialog` 从右侧打开；保留遮罩、焦点陷阱和 Esc/遮罩点击关闭。关闭时先播放右移退出动画，`prefers-reduced-motion` 下直接关闭；关闭后焦点回到触发行。Sheet 按“任务概览 / 真实阶段 / 诊断信息”组织字段，展示错误全文、`TRACKING_SQL`、拒绝记录路径和 `RUNTIME_DETAILS`。
- **失败优先排序**：默认排序 `失败 > 运行中 > 排队 > 已完成/取消`，组内按时间倒序——运维视角“先看坏消息”
- **错误原因归类（对齐 EMR 原因分析）**：后端按 ERROR_MSG 模式匹配归类，输出 `cause: 超时 / 超阈值 / 格式错误 / 权限 / 目标表不存在 / 资源不足 / 未知`，每类附一句处置建议（如“Scan bytes exceed threshold → 减小单次导入体量或调大 `broker_load_scan_bytes_threshold`”）；归类结果在详情 Sheet 错误框顶部渲染为结论行，无法识别时回退原文展示，不臆断
- **空态**：无任务时提示调整时间范围或清除筛选；树节点入口仍可直接带数据库筛选跳转
- **错误详情**：详情 Sheet 提供 `ERROR_MSG`、`TRACKING_SQL` 和 `REJECTED_RECORD_PATH` 的原生输入控件与复制按钮；Doris 失败作业额外展示同一 `SHOW LOAD` 行的原始 `URL`、`ErrorMsg`、`JobDetails`，不接入 `get_load_errors_compromise` 或全局 error hub。
- **新建外部导入**：页头保持**单一创建入口**（`api:clusters:queries:execute` 可见），点击一次即打开唯一创建窗口；来源类型（本地文件 / HDFS·NAS / Kafka）是**该窗口内的第一步选择**，切换类型不叠加第二层弹窗，也无二次确认弹窗。业界同类产品的创建流程均为“单一入口 + 类型作为创建流程的第一步”（Airbyte `New Source` → 连接器列表、Fivetran `Add connection` → 源 tile、DataWorks 新建节点 → 选择来源/去向类型、CloudCanal 创建任务 → 向导步骤），嵌套模态会阻断上下文并放大误操作。提交动作按类型分别使用：StarRocks 本地 CSV/JSON 使用专用 Stream Load 上传代理，文件内容不进入 SQL 历史；无用户信息的 `hdfs://`、已挂载到 BE/CN 的 `file:///` 使用 `WITH BROKER;` 提交异步 Broker Load；仅 `host:port` 的 plaintext Kafka 创建 Routine Load。Broker/Routine SQL 也不进入 Stellar SQL 历史。需要凭据的对象存储、认证 HDFS/Kafka、数据湖、外部数据库、Pipe、Catalog 和外部运行时不提供表单入口；`INSERT INTO ... SELECT` 只在已受管的外部 Catalog 或外部表上作为执行方式出现，不提供 StarRocks 内表互拷入口。
- **过滤行数提示（P1）**：`Filtered_Rows / Scan_Rows > 1%` 且分母有效时行内显示 warning；StarRocks 源码中 `SCAN_ROWS` 已包含正常、异常和未选中行，不能再次把 `FILTERED_ROWS` 加入分母。阈值和原因提示后续再接入集群变量
- **Stream Load profile**：P1 只展示可用的 `PROFILE_ID`/`RUNTIME_DETAILS` 原文，不自动修改集群变量；Profile 引导卡片列入后续迭代
- **键盘可达**：原生 Smart Table 行打开详情 Sheet；打开前让触发行获得焦点，Nebular 焦点陷阱销毁后回焦至该行；Esc 和遮罩点击均可关闭，所有图标按钮带 `aria-label`。

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

- `GET /api/clusters/loads?db=&type=&state=&search=&range=&limit=&cursor=`：查询当前组织活跃集群；默认查询最近 24 小时，服务端每页最多 500 条。StarRocks 合并 `information_schema.loads` 与 `_statistics_.loads_history`，使用响应中的 `next_cursor` 按 `(CREATE_TIME, ID)` 继续翻页；若历史视图不可用，自动退回运行视图。Doris 保持运行视图与 `SHOW LOAD` 回退，不将有限的 `SHOW LOAD` 结果误称为持久历史。
- `GET /api/clusters/loads/{job_id}?db=`：按作业 ID 返回详情；详情查询使用 `range=all`，避免历史任务因默认时间范围被误判不存在。Routine Load 在 `PROPERTIES.job_name` 可用时，按需查询其父作业的 `Progress`、`OFFSET_LAG`/`Lag`、最新源位点、统计和当前子任务。Doris 已选失败作业在已知库内以精确 Label 读取 `SHOW LOAD`，只在 JobId 严格匹配时附带原始 `URL`、`ErrorMsg`、`JobDetails`；字段缺失或目标引擎不支持时只返回基础任务详情。
- 首选 `information_schema.loads`；查询失败时按数据库回退到 `SHOW LOAD`，缺失字段保持 `null`
- 阶段列表只由 `CREATE_TIME`、`LOAD_START_TIME`、`LOAD_COMMIT_TIME`、`LOAD_FINISH_TIME` 计算；`RUNTIME_DETAILS` 当前作为原文保留，不把内部字段猜测成阶段
- 失败原因按 `ERROR_MSG` 关键词归类为 `Timeout`、`ThresholdExceeded`、`FormatError`、`PermissionDenied`、`TargetMissing`、`ResourceExhausted`、`Unknown`
- 使用独立 `api:clusters:loads` 权限；权限提取器将 `GET /api/clusters/loads*` 映射到该权限。迁移仅为已有 `api:clusters:queries` 查询读取权限的角色同时补发 `menu:loads` 与 `api:clusters:loads`；旧迁移对仅有 `menu:queries:execution` 的角色误补的 Load 菜单会在紧随其后的修正迁移中移除，保持菜单与路由守卫一致
- `_statistics_.loads_history`、Routine Load 位点/子任务和游标分页已保持在同一查询链中；不为当前一条查询链预先增加 adapter 抽象。Doris 失败详情不使用 error hub；`SHOW LOAD` 不支持已确认的 JobId 谓词，故只将精确 Label 的结果在服务端按 JobId 二次收窄

## 7. 前端组件划分

```
pages/starrocks/loads/
└── load-management.component.{ts,html}  # 原生筛选、Smart Table 与 Nebular 详情 Sheet
```

- 路由 `path: 'loads'` 挂在 starrocks 模块，一级菜单"数据导入"使用 `menu:loads`，路由守卫和 API 使用 `api:clusters:loads`
- 页头“新建外部导入”使用 `api:clusters:queries:execute` 控制可见性，并从同一窗口内切换来源类型；本地文件执行通过 `/api/clusters/queries/stream-load`，无凭据 Broker/Routine 作业复用现有 SQL 执行接口但不记录执行历史，三者均保留组织权限边界与提交后任务观测
- 树节点 `viewLoads` 改为路由跳转携带 `?db=<db>` 预置筛选
- 遵循 MASTER.md 的 ngx-admin 原生优先原则：`row/col + nb-card`、原生筛选控件、`angular2-smart-table`、Nebular `NbDialog` Sheet、`nb-alert` 与原生分页；二维表格在自身容器横向滚动，不重绘为自定义卡片列表或手写侧板。

## 8. 测试

- 后端：`backend/src/tests/load_service_test.rs` 覆盖失败原因分类、真实时间阶段计算、终态缺时间戳不造假、SQL 字面量转义/limit、历史合并稳定游标、Routine Load 父作业/子任务字段、Doris 精确 Label 查询与 JobId 二次匹配和路由权限映射
- 前端：`load-management.component.spec.ts` 覆盖态势摘要、Smart Table 详情入口和回焦、Sheet 初始焦点、服务端游标“加载更多”、单一创建入口只打开一个窗口、窗口内来源类型切换（降级 path 专用格式、补默认 Label）、Doris 不打开创建窗口、本地文件 Stream Load 的 FormData 提交、无凭据 Broker Load / plaintext Routine Load SQL 构造与不安全路径/Kafka 取值拒绝；`npm run build` 验证路由、模板、Nebular 组件和 Smart Table 可编译
- E2E 手册：对接真实集群造 1 个成功 + 1 个失败 Broker Load，核对 `RUNTIME_DETAILS` 原文、`TRACKING_SQL`、`REJECTED_RECORD_PATH` 与真实时间阶段；再验证一个 Routine Load 不被错误绘制成批处理阶段

## 9. 分期

| 期 | 内容 | 出口 |
|---|---|---|
| P1 | 原生 Smart Table 列表 + 状态聚合 + Nebular 详情 Sheet + 失败详情/追踪 SQL/拒绝记录路径 + StarRocks/Doris 查询回退 | 覆盖主要日常排障 |
| P2 | `_statistics_.loads_history` 历史查询、游标分页、Routine Load 位点/子任务面板、Doris 已选失败作业的 `SHOW LOAD` 原始诊断已实施 | 常驻导入场景闭环 |
| P3a | StarRocks 本地 CSV/JSON、无凭据 HDFS / 挂载 NAS、plaintext Kafka → 目标表 | 具备执行权限的用户可以创建真实外部导入任务，提交后回到统一运行观测 |
| P3b | 受管对象存储、认证 HDFS / NAS / Kafka、数据湖 / 外部数据库、外部运行时接入和字段映射（有真实需求再评估） | 连接、凭据、生命周期和状态契约齐备后再开放 |

## 10. 明确不做（YAGNI）

- 拖拽式导入编排画布、任务依赖图（无多阶段依赖数据源）
- 导入向导式建表+导表一体流（EMR 也未做，元数据 UI 建表已判低价值）
- 告警通知（归入平台级告警设计，不挂在导入页）
- 在通用 SQL 历史里保存本地文件内容或对象存储/Kafka 明文凭据
- 在外部导入入口中提供 StarRocks 内表之间的 `INSERT INTO ... SELECT` 搬运
- 在 Browser 表单中收集对象存储访问密钥、HDFS 用户口令 / Kerberos keytab、Kafka SASL 密码或 TLS 私钥
- 短信/电话通知渠道（自托管场景 webhook 已覆盖）
