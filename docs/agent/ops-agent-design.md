# Stellar 智能运维 Agent 设计方案

> 版本: v1.2  
> 日期: 2026-03-11（v1.1 修订：2026-09-11）  
> 状态: v1.0 设计待评审 → **第一阶段（Flink 复刻对话式只读诊断助手）已落地并通过端到端验证**；本文档为全局设计蓝图，落地进度见 `docs/agent/development-progress.md`  
> 思想来源: 《PowerFS AI Agent：让存储系统从被动响应走向主动自治》（管理面旁挂 + 感知-决策-执行-验证闭环 + 确定性优先 + 分级自治 L0-L3）  
> 同类参考: SxDevOps AIOps（Incident-first 平台工作流驱动 Agent，`/mnt/data/sxdevops/docs/AIOps-告警驱动自治运维闭环设计.md`）；Flink（故障恢复与信号驱动思想）；pi（Agent 工具预算/事件驱动，借鉴其边际护栏设计）

---

## 〇、Flink 参照与第一阶段复刻方案（2026-09 决策）

> 本方案的落地顺序经用户确认调整：**不按 M0 事件闭环起步，而是先复刻 Flink AI job diagnostic assistant 的对话式只读诊断助手**。

### 0.1 参照链与依据

实现骨架参照链：**pi-from-scratch → Flink AI assistant → Stellar**。

Flink AI assistant 位于内部 fork `/mnt/data/flink`（`flink-runtime/src/main/java/org/apache/flink/runtime/ai/`，docs 见 `flink-runtime-web/.../handlers/ai/`），已完成 agent loop、工具协议、会话、SSE、写动作两阶段确认等工程问题。

### 0.2 Flink → Stellar 映射

| Flink AI assistant | Stellar 第一阶段的对应实现 |
| --- | --- |
| `DefaultJobDiagnosisAgent`（stream → tool_call → 执行 → tool_result 循环） | `services/ops_agent/agent.rs` `OltpDiagnosisAgent::run`（非流式同类循环，步骤级 `AgentStep` 审计） |
| `AgentTool` 接口（name+description+JSON Schema+execute→string） | `services/ops_agent/tool.rs` `AgentTool` trait + `tool_specs()` |
| `ContextBuilder`（prompt 模板 + 每轮刷新 job 快照） | `services/ops_agent/context.rs`（`SYSTEM_TEMPLATE` + 每轮 `render_snapshot`） |
| `DiagnosisSkills` playbook | `services/ops_agent/skills.rs`（5 个诊断 playbook） |
| `ContextCompactor`（>40 条 LLM 摘要） | 暂留：历史保留最近 20 条，压缩留待后续阶段 |
| `DiskBackedAiChatSessionManager`（磁盘 JSON 会话） | `services/ops_agent/session.rs` AppDb 两张表（`agent_chat_sessions` / `agent_chat_messages`），SQLx 三方言 `insert_id()` 兼容 |
| `PlatformWriteTools` + `ActionSubmission`（PENDING_ACTION + diff 确认卡） | **第一阶段不注册写工具**（Non-goal）；两阶段确认结构在 M2 落地 |
| `AiChatStreamHandler`（SSE + 30s heartbeat） | 第二阶段（SSE 流式） |

### 0.3 复刻缺口（已在设计中保留的决策）

1. **纯手动触发，无事件闭环**：事件→Incident 自动闭环（PowerFS 文章核心价值）保留在后续阶段（修正后的 M1）；
2. **LLM 主导 vs 确定性优先**：工具只读 + 结论引用真实指标，规则引擎（Profile 12 类规则）优于 Flink 纯 LLM 诊断，保留在路线中；
3. **写动作无分级/回滚/验证**：L0-L3、回滚、验证在 M2 落地，第一阶段完全不注册写工具。

### 0.4 第一阶段已落地内容（2026-09-11 验证通过）

- 后端模块 `backend/src/services/ops_agent/`：agent loop / 6 个只读工具 / 快照上下文 / 步骤审计，共享会话与 LLM 通道在 `services/ai/`（`ai_sessions` / `ai_messages` 宽表 + `channel` 隔离，见 `docs/agent/ai-common-design.md`）
- 权限：`agent` 根资源（chat / sessions / sessions:get / sessions:delete）+ 菜单 `menu:agent`（迁移 `20260911000001_add_agent_permissions.sql` 三方言语义落地）
- API：`POST /api/agent/chat`、`GET /api/agent/sessions?cluster_id=`、`GET/DELETE /api/agent/sessions/:id`（`handlers/agent_chat.rs`）
- 端到端验证：mock LLM 验证完整 tool 循环（query_metrics 真实取数 → 二次请求携带 tool 结果 → 最终回答）、会话复用/CRUD、无 Provider 时 400 明确报错；`cargo clippy --deny warnings` 全绿

---

## 一、背景与缺口

### 1.1 现状：Stellar 已经"看得见、查得动、改得了"，但缺少决策大脑

| 层次 | 已有能力 | 实现位置 | 缺口 |
| --- | --- | --- | --- |
| 感知 | 30s 周期指标采集，落 `metrics_snapshots`（QPS/延迟/节点存活/磁盘/compaction/txn/load/JVM/IO 共 40+ 字段） | `backend/src/services/metrics_collector_service.rs` | 采集结果只做展示与即时判断，无事件流、无历史对比触发 |
| 感知 | 即时健康告警串（`generate_alerts`，无状态阈值判断） | `backend/src/services/overview_service.rs:1164` | 不持久化、不去重、不聚合、无生命周期 |
| 感知 | 慢查询与访问热度（审计日志：慢查询、Top 访问表） | `backend/src/services/audit_log_service.rs` | 未与诊断/动作联动 |
| 决策 | Profile 规则引擎（12 类算子规则）+ 根因推断（因果图）+ 自适应阈值 + 智能参数推荐 | `backend/src/services/profile_analyzer/` | 仅单查询级、只在用户点开 Profile 时触发，无集群级编排 |
| 决策 | LLM 服务（OpenAI 兼容 Provider、场景化 Prompt、缓存、用量统计） | `backend/src/services/llm/` | 只有 SQL 诊断 / Profile 根因两个场景，未服务集群运维 |
| 执行 | 集群操作能力：kill query、kill session、下线 BE、刷 MV、SQL 黑名单、改变量、执行 SQL、资源组管理 | `backend/src/services/cluster_adapter/mod.rs`（trait 20+ 方法）、`resource_group_service.rs` | 无统一动作网关，无风险分级、无 dry-run、无回滚、无执行后验证 |
| 治理 | 申请-审批流程、RBAC/Casbin、多租户组织、审计日志 | `permission_request_service.rs`、`casbin_service.rs`、`middleware/` | 审批只服务"权限申请"，不服务"运维动作" |
| 调度 | `ScheduledExecutor` + `ScheduledTask` | `backend/src/utils/scheduled_executor.rs` | 无事件驱动任务链 |

一句话总结：**Stellar 停在"有感知、有执行、缺少决策大脑"的状态**——这正是 PowerFS 文章描述的缺口，只是领域从分布式存储换成 OLAP 集群。

### 1.2 目标

把已有能力串成闭环：

```
感知（事件） → 决策（规则优先，LLM 补位） → 执行（分级 + 护栏） → 验证（回归观测） → 沉淀（剧本/阈值）
                        ↑                                                                  │
                        └──────────────────── 决策审计与证据链 ────────────────────────────┘
```

可验证的达成标准：

1. 集群出现 BE 掉线 / 磁盘水位越界 / compaction 失控 / 导入积压 / 慢查询陡增时，**无需人工点击**，系统自动产出结构化诊断报告（含证据、根因假设、置信度、建议动作）。
2. 每个建议动作带风险等级、前置条件、回滚方案、验证计划；执行必须经过自治等级与护栏校验。
3. 每条决策可追溯：输入快照、命中的规则、LLM 输入摘要与用量、护栏判定结果、执行结果、验证结论。
4. LLM 或集群不可达时自动降级，不影响现有平台任何功能。

---

## 二、设计原则（对齐文章五原则，落到 Stellar 约束）

| # | 原则 | Stellar 具体落地 |
| --- | --- | --- |
| 1 | **确定性优先** | 阈值规则、因果图（`analyzer/root_cause.rs` 已实现）先跑；LLM 仅在规则无法定论时调用一次，且**规则与 LLM 冲突时采信规则**。LLM 不得进入阈值判定路径。 |
| 2 | **只读自动，写入审批** | 取证（读）全自动；任何写动作先落 `agent_actions` 待确认记录。L3 自动执行仅限白名单动作类型且护栏通过。 |
| 3 | **证据先于结论** | 根因假设必须绑定 `agent_evidences.id`；证据不足时输出"证据缺口 + 下一步只读取证"，禁止裸结论。 |
| 4 | **可审计、可回滚、可预演** | 决策审计独立成表；高危动作支持 dry-run（能预演用 `EXPLAIN`/只读校验，不能预演输出影响评估）；每个动作必须声明回滚方式。 |
| 5 | **失败安全降级** | LLM 不可用 → 纯规则模式；集群不可达 → 只读 + 缓存；动作效果退化 → 该动作类型自动降级（见 §6.8）。 |
| 6 | **复用而非重建** | 不新建独立进程、不新建采集通路、不新建 LLM 客户端、不新建权限体系。 |

### 边界声明（避免与既有能力重叠）

- **Profile 分析器 = 查询级诊断器**（单条 SQL 的执行画像，已有）；**运维 Agent = 集群级自治编排层**（跨时间、跨节点、跨事件）。Agent 把 Profile 分析器当作一个决策器调用，不重复实现规则。
- **`permission_request_service` = 数据权限申请审批**（语义：用户要权限）；**`agent_actions` = 运维动作审批**（语义：系统要改集群）。两者不合并，只共用 Casbin 权限码与前端审批交互模式。

---

## 三、四层架构

```
┌─────────────────────────── L4 交互层 ────────────────────────────┐
│  运维工作台（事件/故障/证据/假设/动作）  │  建议卡片与审批  │  决策审计  │  自然语言问答  │
└───────────────────────────────┬──────────────────────────────────┘
                                │ REST + SSE（事件推送）
┌─────────────────────────── L3 决策层（Agent 大脑）───────────────┐
│  事件收敛器 → 取证编排器 → 规则诊断器 → LLM 根因推理 → 剧本匹配    │
│                                   └→ 护栏校验器 → 自治仲裁器      │
└───────────────────────────────┬──────────────────────────────────┘
                                │
┌─────────────────────────── L2 感知层 ───────────────────────────┐
│ 指标快照对比器 │ 健康告警（升级）│ 慢查询/审计摄取 │ Profile 诊断摄取 │ 集群变量与拓扑 │
└───────────────────────────────┬──────────────────────────────────┘
                                │
┌─────────────────────────── L1 执行层（Action Gateway）──────────┐
│ 动作白名单 │ 风险分级 │ dry-run │ 限流并发 │ 审批闸门 │ 审计 + 回滚 + 验证 │
└───────────────────────────────┬──────────────────────────────────┘
                                │ 复用 ClusterAdapter / ResourceGroupService
                         StarRocks / Doris 集群
```

### 3.1 部署形态：同进程模块，不新增部署单元

**决策**：作为 `backend/src/services/ops_agent/` 模块 + 一个 `ScheduledTask` 后台循环，与现有后端同进程。

**理由**（第一性原理）：
- Stellar 后端本身就是管理面进程，Agent 天然"旁挂"，不进数据面 IO 路径（对比 PowerFS 需要新进程是因为 Agent 与存储进程物理分离）。
- 直接复用 `AppDb` 连接池、`LLMService`、`ClusterAdapter`、`CasbinService`，零跨进程序列化、零新增部署/运维成本。

**风险与对策**：Agent 长任务可能占用 API 进程资源。
- 所有 Agent 执行在 `tokio::spawn` 的独立任务中，全局并发上限 1（见护栏）；
- 每个阶段设超时（取证 60s、LLM 90s、动作 300s），超时即降级输出已有证据；
- 采集路径（`metrics_collector`）**只做纯内存事件投递（`mpsc::try_send`，满则丢弃并计数）**，绝不阻塞采集。

---

## 四、三大主线（领域映射）

PowerFS 的三大主线平移到 OLAP 运维域：

| PowerFS 主线 | Stellar 主线 | 事件源 | 典型决策 | 可执行动作 |
| --- | --- | --- | --- | --- |
| 可靠性运维 | **可靠性主线**（P0，优先落地） | FE/BE 存活、磁盘水位、compaction score、txn 失败率、load 积压、JVM 堆、副本不健康 | 告警收敛 → 根因假设 → 自愈剧本 | 下线坏 BE、调 compaction 并发变量、SQL 黑名单限流、暂停/恢复导入、副本修复 SQL |
| 访问轨迹优化 | **性能主线** | 慢查询、审计日志、Profile 规则诊断、查询延迟 p95/p99 突增 | Profile 规则引擎 + 因果图 + LLM 根因 → 优化建议 | kill query、SQL 黑名单、变量调参、建索引/MV（DDL） |
| 数据生命周期 | **容量与负载主线** | 磁盘水位趋势、表访问热度、数据倾斜、资源组用量 | 容量预测 → 分层/归档/配额建议 | 资源组配额调整、TTL/分区/冷热归档建议（默认 L1）、MV 复用建议 |

### 4.1 协同与冲突仲裁

- 容量主线输出"表访问热度"→ 作为性能主线（MV/索引建议）与容量主线（归档判断）的共同输入。
- 可靠性事件发生时，**抑制**同节点/同集群的容量与性能类自动动作（故障节点上的数据不参与容量决策）。
- 冲突优先级：**可靠性 > 容量与负载 > 性能**。
- 同一时刻全局只允许 1 个动作在执行（护栏硬约束）。

---

## 五、数据模型

新增表全部需要 **sqlite / mysql / postgres 三方言同步**（`backend/migrations/{sqlite,mysql,postgres}/`，编译期嵌入，参考 `docs/MYSQL_SUPPORT_DESIGN.md`）。JSON 字段统一用 `TEXT` 存序列化字符串（与既有 `metrics_snapshots.raw_metrics` 一致）。

### 5.1 M0 阶段（只读诊断，5 张表）

#### `agent_events` — 事件事实表

| 字段 | 说明 |
| --- | --- |
| `id`, `organization_id`, `cluster_id` | 主键 + 多租户隔离 + 集群 |
| `fingerprint` | 归并键：`{cluster}:{kind}:{object}:{metric}` |
| `kind` | `metric_breach` / `node_down` / `disk_pressure` / `compaction` / `load_backlog` / `txn_error` / `slow_query` / `profile_diagnosis` |
| `severity` | `info` / `warning` / `critical` |
| `object_type`, `object_id` | 目标对象：`cluster` / `be` / `fe` / `query` / `table` / `load_job` |
| `title`, `summary` | 面向人的描述 |
| `metrics_json` | 触发时的关键指标快照（用于复现） |
| `state` | `open` / `aggregated` / `suppressed` / `resolved` |
| `occurrence_count`, `first_seen_at`, `last_seen_at` | 收敛计数与窗口 |
| `suppressed_by` | 被哪条事件抑制（拓扑抑制链） |

索引：`(cluster_id, state, last_seen_at)`、`(fingerprint, state)`、`(organization_id, severity)`。

#### `agent_incidents` — 故障/问题对象（聚合根）

| 字段 | 说明 |
| --- | --- |
| `id`, `organization_id`, `cluster_id` | |
| `dedupe_key` | `{cluster}:{primary_fingerprint}` |
| `title`, `impact_summary` | |
| `status` | `open` → `investigating` → `mitigating` → `verifying` → `resolved` → `closed`；旁路 `suppressed` |
| `severity` | `critical` / `warning` / `info` |
| `primary_event_id` | 主事件 |
| `current_hypothesis_id` | 当前主假设（M1 起可空） |
| `owner_user_id` | 认领人 |
| `started_at`, `resolved_at`, `closed_at` | |
| `metadata_json` | 扩展 |

#### `agent_incident_events` — 事件与故障关联

`incident_id`、`event_id`、`role`（`primary` / `related` / `symptom` / `resolved_signal`）、`linked_reason`。

#### `agent_evidences` — 证据

| 字段 | 说明 |
| --- | --- |
| `id`, `incident_id` | |
| `kind` | `metric` / `node` / `query` / `audit` / `profile` / `variable` / `baseline` / `topology` |
| `source` | 采集器名（如 `metric_collector`、`audit_log`） |
| `scope_json` | 时间窗、节点、表、查询等范围 |
| `window_start`, `window_end` | |
| `summary` | 给人和 LLM 用的压缩摘要（**原始数据不入 LLM 上下文**） |
| `payload_json` | 结构化原始结果（供前端展开） |
| `weight` | `strong` / `medium` / `weak` / `counter` |
| `collected_at` | |

#### `agent_decisions` — 决策审计（每阶段一条）

| 字段 | 说明 |
| --- | --- |
| `id`, `incident_id`, `stage` | `intake` / `evidence` / `rule_diagnosis` / `llm_hypothesis` / `guardrail` / `execution` / `verification` |
| `engine` | `rule` / `llm` / `guardrail` |
| `input_digest` | 输入摘要哈希（证据 ID 集合 + 关键指标） |
| `output_json` | 该阶段产出（规则命中列表 / LLM 结构化输出 / 护栏判定） |
| `llm_provider`, `llm_model`, `llm_tokens` | LLM 用量（复用 `llm_usage_stats` 思路） |
| `duration_ms`, `status`, `error` | |
| `created_at` | |

> M0 不建 `agent_hypotheses`：只读阶段把假设存放于 `agent_decisions.output_json`（stage=`llm_hypothesis`），避免过早引入状态机。

### 5.2 M1 阶段（执行闭环，+2 张表，累计 7 张）

#### `agent_hypotheses` — 根因假设

`incident_id`、`title`、`root_cause_type`（`resource_saturation` / `node_failure` / `data_skew` / `config_regression` / `workload_spike` / `capacity` / `unknown`）、`confidence`(0-1)、`supporting_evidence_ids`、`counter_evidence_ids`、`missing_evidence_json`、`recommended_next_checks_json`、`status`（`candidate` / `primary` / `rejected` / `confirmed`）、`created_at`。

#### `agent_actions` — 动作（建议 → 审批 → 执行 → 验证）

| 字段 | 说明 |
| --- | --- |
| `id`, `organization_id`, `incident_id`, `hypothesis_id` | |
| `action_type` | 白名单枚举，见 §6.4 |
| `autonomy_level` | 生成时按策略计得的 L0-L3 |
| `risk_level` | `low` / `medium` / `high` / `critical` |
| `status` | `proposed` → `approved` → `running` → `completed` / `failed` / `canceled`；`rolled_back` |
| `payload_json` | 动作参数（如目标 BE、变量名与目标值、SQL 文本） |
| `dry_run_json` | 预演结果（影响评估 / `EXPLAIN` 摘要 / 拒绝理由） |
| `preconditions_json` | 前置条件（执行前逐条校验，失败即中止） |
| `rollback_json` | 回滚方案（可执行的反向动作或人工步骤） |
| `verification_json` | 验证计划（观察哪些指标、窗口、判定阈值） |
| `verification_status` | `pending` / `verified_resolved` / `partially_improved` / `no_improvement` / `verification_failed` |
| `approved_by`, `approved_at`, `executed_at`, `finished_at` | |
| `result_summary`, `error` | |

> **剧本库与自治策略不建表**：以内建注册表（Rust 常量）+ `conf/config.toml` 的 `[agent]` 段承载（YAGNI：策略项少且变更需评审）。

---

## 六、关键机制

### 6.1 事件接入与收敛（规则优先，目标收敛 80% 常规风暴）

事件源（全部只读）：

1. **指标快照对比器**：`metrics_collector_service` 每轮采集结束后投递事件（前后快照比较：节点存活数下降、磁盘水位跨档、compaction score 越阈、txn 失败增量、load 积压）。投递为 `try_send`，队列满则丢弃 + 计数告警，**采集路径零阻塞**。
2. **健康告警升级**：`overview_service::generate_alerts` 的即时字符串告警升级为结构化事件（保留原 UI 展示不变）。
3. **慢查询摄取**：`audit_log_service` 慢查询列表 → `slow_query` 事件。
4. **Profile 诊断摄取**：慢查询事件触发取证时，对 Top 1 查询跑 Profile 规则引擎（§6.2 ProfileCollector），产出 `profile_diagnosis` 证据及其相关诊断事件（复用已有规则，不重复实现）。

收敛策略（纯规则，按优先级）：

1. `fingerprint` 完全匹配 → 累加 `occurrence_count`，不新建事件；
2. 时间窗（默认 5 分钟）+ `object_type` 相同 → 聚合为一组；
3. **拓扑抑制**：`fe` 或 `be` 失联事件产生后，同节点上的 `tablet` / `query` / `disk` 类事件标记 `suppressed`（次生告警）；
4. 抑制规则可配置（`conf/config.toml` `[agent.suppress]`）。

**阈值注册表**：集群级 breach 判定阈值（磁盘水位档、compaction score、txn 失败率等）以代码常量注册表承载，复用 `overview_service::generate_alerts` 已有判定口径，`[agent.thresholds]` 可覆盖；不建 DB 表（YAGNI）。

**复发规则**：Incident `resolved` 后 30 天内同 `dedupe_key` 事件再触发 → 原 Incident 重新 `open`（保留证据链与历史动作）；超过 30 天 → 新建 Incident。

### 6.2 取证编排（固定流水线，非 DAG）

Incident 创建/升级后启动**固定阶段**取证（借鉴 SxDevOps 结论："第一阶段用固定阶段即可，不需要复杂 DAG"）：

```
metrics → nodes → queries → audit → profile → variables → baseline
```

| 采集器 | 复用能力 | 预算 |
| --- | --- | --- |
| `MetricCollector` | `metrics_snapshots`（前后 N 轮） | 窗口 ±30 分钟，最多 60 点 |
| `NodeCollector` | `adapter.get_backends()` / `get_frontends()` | 1 次调用 |
| `QueryCollector` | `adapter.get_queries()` | 1 次，Top 20 |
| `AuditCollector` | `audit_log_service.get_slow_queries()` / `get_top_tables_by_access()` | 每次最多 20 行 |
| `ProfileCollector` | 直接复用 `profile_analyzer` 的 `parser + RuleEngine`（services 层内部调用，不经过 HTTP handler；仅对 Top 1 慢查询） | 1 条 Profile，规则引擎无 LLM |
| `VariableCollector` | `SHOW VARIABLES`（`ClusterVariables`） | 关心的键白名单 |
| `BaselineCollector` | `baseline_service`（历史基线/自适应阈值） | 1 次 |

硬约束：单次取证总超时 60s；任一采集器失败不阻断其余（失败记为 `weak` 证据）；**每类证据只采一次**，避免成本失控。

### 6.3 决策：规则先行，LLM 补位

```
证据就绪
 ├─ 规则诊断器（确定性）
 │    ├─ 阈值规则（复用 adaptive thresholds / cluster_variables）
 │    ├─ Profile 规则引擎（查询级，已有 12 类规则）
 │    ├─ 因果图（analyzer/root_cause.rs，已有）
 │    └─ 剧本匹配（症状模式 → 已知根因 + SOP）
 │            ↓ 命中且置信度 ≥ 0.8
 │        直接产出根因（不调用 LLM）
 └─ 未定论 → LLM 根因推理（单次调用）
             输入：Incident 元数据 + 证据摘要（含 ID）+ 集群画像
             输出：结构化假设列表（见下），强制证据 ID 引用
             ↓
          护栏校验 → 建议生成 → 自治仲裁（L0-L3）
```

**LLM 输出契约**（结构化 JSON，禁止自由文本）：

```json
{
  "hypotheses": [{
    "title": "BE-3 磁盘 IO 饱和导致导入积压",
    "root_cause_type": "resource_saturation",
    "confidence": 0.72,
    "supporting_evidence_ids": [12, 15],
    "counter_evidence_ids": [21],
    "missing_evidence": ["缺少 BE-3 近 10 分钟 compaction 明细"],
    "recommended_next_checks": ["查看 BE-3 compaction score 曲线"]
  }]
}
```

**动作建议不直接由 LLM 生成**：动作参数、风险分级、回滚与验证计划一律出自确定性剧本模板（根因类型 × 动作类型）；LLM 假设只用于解释根因与选择剧本，输出非法时回退纯规则建议。

后置校验（后端强制，LLM 输出不合格则丢弃并回退规则摘要）：
- 引用的证据 ID 必须存在且属于本 Incident；
- `confidence` 归一化到 [0,1]；
- 无证据引用的假设直接丢弃；
- 所有 ID 非法 → 记 `agent_decisions.status = rejected`，Incident 结论标注"证据不足"。

### 6.4 动作白名单与风险分级

**执行通道有两类**（动作按现有 handlers 真实能力路由，不臆造）：

1. **ClusterAdapter（FE HTTP 管理 API）**：`drop_backend`、SQL 黑名单、物化视图刷新等（`services/cluster_adapter/mod.rs`）；
2. **MySQLClient（MySQL 协议连接池，`mysql_pool_manager`）**：kill 查询/会话（`KILL QUERY/SESSION`）、变量更新（`SET [GLOBAL] ...`）、执行 SQL 与导入控制（`PAUSE/RESUME ROUTINE LOAD` 等 SQL 模板）——与现有 `handlers/query.rs`、`handlers/variables.rs` 实现通道完全一致。

| action_type | 风险 | 默认自治 | dry-run | 回滚 | 验证 |
| --- | --- | --- | --- | --- | --- |
| `kill_query` | medium | L2 | 影响评估（查询耗时/扫描量） | 无法回滚（可重跑） | 查询延迟 p95 回落 |
| `kill_session` | high | L2 | 会话影响评估 | 无 | 连接数/错误率 |
| `drop_backend` | critical | L1 | BE 上 tablet 影响评估 | 重新 `ADD BACKEND` | BE 列表与副本健康 |
| `add_sql_blacklist` / `delete_sql_blacklist` | medium | L2 | 命中预估（对审计日志试匹配） | 删除黑名单项 | 命中查询数下降 |
| `update_variable` | medium | L2 | 当前值 → 目标值 diff + 影响面 | 恢复原值 | 相关指标回归窗口 |
| `refresh_materialized_view` | low | L3 | 刷新范围与耗时估算 | 无（幂等） | 刷新状态与耗时 |
| `pause_load` / `resume_load` | high | L2 | 积压量评估 | 反向 resume/pause | 积压量与错误率 |
| `execute_sql`（DDL，如 `ALTER TABLE ... ADD INDEX`） | high | L1 | `EXPLAIN` + 影响行数预估 | 反向 DDL（仅登记，人工执行） | 目标查询耗时 |
| `adjust_resource_group` | medium | L2 | 配额变更 diff + 受影响用户 | 恢复原配额 | 资源组用量与排队时长 |

> `pause_load`/`resume_load` 无独立 adapter 方法，经 `execute_sql` 白名单模板（`[PAUSE|RESUME] ROUTINE LOAD FOR <job>`）执行，受同一批模板校验。

> **`execute_sql` 是后门**：默认禁用；仅在白名单语句模板（正则受限，例如 `ALTER TABLE ... ADD/DROP INDEX`、`ALTER TABLE ... MODIFY PARTITION ... TTL`）匹配下允许，其余一律拒绝并降级为 L1 人工建议。

### 6.5 护栏 Guardrails（后端硬约束，不交给 LLM）

| 护栏 | 默认值 | 说明 |
| --- | --- | --- |
| 全局并发 | 1 个动作同时执行 | 保护集群与 API 进程 |
| 取证并发 | 2（信号量） | 防止多 Incident 同时取证拖垮 API 进程 |
| 动作节流 | 同类动作 ≤ 3 次/小时 | 防震荡 |
| 维护窗口 | 高危动作仅窗口内自动执行 | 窗口外自动降级到 L1 |
| 对象保护 | 不 kill 未识别/非目标 query_id（仅 kill Agent 判定失控的查询）；不下线最后 1 个可用 BE；不停止唯一的 routine load；不修改非白名单变量 | 硬编码 |
| 目标规模 | 影响行数/数据量超阈值即降级 | 例如 DDL 影响 > 1 亿行 → 仅建议 |
| 全局熔断 | `agent.enabled = false` → 一键全量降 L0 | 紧急止血 |
| 失败熔断 | 同类动作连续 2 次失败 → 该类动作自动降到 L0 并告警 | §6.8 |

### 6.6 分级自治 L0-L3

| 等级 | 行为 | 适用 |
| --- | --- | --- |
| L0 观察 | 只输出报告与证据 | 巡检、周报 |
| L1 建议 | 输出卡片，人工点击才执行 | 高危动作（drop_backend、DDL） |
| L2 报批 | 生成 dry-run 计划 + 待确认动作，人工一键审批 | kill、变量调参、黑名单、资源组 |
| L3 自动 | 护栏内自动执行 + 事后通知 | 低风险幂等动作（刷 MV） |

- **新动作类型注册时默认 L0**，需人工显式提级到 6.4 表中的建议等级（该表为"启用后的推荐值"）；已提级的动作类型可随时一键降回。
- 每类动作的自治等级可配置（`[agent.autonomy]`）。
- 紧急降级：`PUT /api/agent/settings` 一键把全部动作降到 L0，已运行动作不中断但不再发起新动作。

### 6.7 执行与验证闭环

```
agent_actions(proposed)
  → 前置条件校验（失败 → canceled，记录原因）
  → dry-run（失败或影响超阈 → 降级为人工建议）
  → 护栏校验 + 自治等级闸门
  → 审批闸门：L2 需人工 `approved_by` 后执行；L3（护栏内）自动执行并事后通知
  → ClusterAdapter 调用（超时 300s）
  → 结果回写
  → 验证：等待 2 个采集周期（默认 60s）后重采指标，按 verification_json 判定
  → verified_resolved / partially_improved / no_improvement
  → no_improvement → 提示回滚；回滚后升级 Incident 严重度
```

验证结论回写 Incident 状态：`mitigating` → `verifying` → `resolved`（自动）/ 人工 `closed`。

### 6.8 降级与失败安全

| 故障 | 行为 |
| --- | --- |
| LLM 不可用 / 超预算 | 切换纯规则模式，报告标注"LLM 未参与" |
| 集群不可达 | 只读降级：使用最近快照分析，动作类结论一律 L1 |
| 取证失败 | 输出已有证据 + 缺口清单，不阻塞 Incident 创建 |
| 动作效果退化 | 该动作类型自动降级（L3→L2→L1→L0），记 `agent_decisions` |
| Agent 任务 panic | `ScheduledTask` 循环捕获错误继续，Incident 标记 `investigating` 待人工 |

---

## 七、API 与前端

### 7.1 API（`backend/src/handlers/agent.rs`，注册于 `main.rs`）

权限遵循项目既有模型：`permission_extractor`（`middleware/permission_extractor.rs`）从 URI/Method 推导 `(resource, action)`，交 Casbin 校验（`casbin_service.rs`）。权限码风格与现有 `clusters:queries:kill`、`clusters:variables:update` 一致：

| 方法 | 路径 | 说明 | 权限码 |
| --- | --- | --- | --- |
| GET | `/api/agent/events` | 事件列表（过滤：cluster/kind/severity/state） | `agent:events` |
| GET | `/api/agent/incidents` | 故障列表 | `agent:incidents` |
| GET | `/api/agent/incidents/:id` | 详情（含事件、证据、假设、动作、时间线） | `agent:incidents` |
| POST | `/api/agent/incidents` | 手工创建故障 | `agent:incidents:create` |
| POST | `/api/agent/incidents/:id/investigate` | 手动触发只读调查 | `agent:incidents:investigate` |
| POST | `/api/agent/incidents/:id/close` | 关闭并生成复盘摘要 | `agent:incidents:close` |
| GET | `/api/agent/actions` | 动作列表（含待确认） | `agent:actions` |
| POST | `/api/agent/actions/:id/approve` | 审批执行 | `agent:actions:approve` + 目标域权限 |
| POST | `/api/agent/actions/:id/cancel` | 取消 | `agent:actions:cancel` |
| POST | `/api/agent/actions/:id/rollback` | 回滚 | `agent:actions:rollback` |
| GET | `/api/agent/decisions` | 决策审计查询（按 incident/stage） | `agent:decisions` |
| GET | `/api/agent/settings` / PUT | 自治等级、护栏参数、开关 | `agent:settings` |
| GET | `/api/agent/stream` | **SSE**：事件与状态变更推送 | `agent:events` |

**实现注意**：

1. `permission_extractor.rs` 需新增 `agent` 根资源及其 action 提取规则（现有仅 roles/permissions/users/clusters 四个根）；
2. 前端权限枚举与权限管理页（`frontend/src/app/@core/data/`）需同步登记 `agent:*` 权限码；Casbin 策略经 `reload_policies_from_db`（`casbin_service.rs:138`）从 DB 加载，无需改代码；
3. **SSE 鉴权**：浏览器 `EventSource` 无法携带 `Authorization` header，而 `auth_middleware` 只认 Bearer 头——前端改用 `fetch + ReadableStream` 解析 SSE（可带 Bearer 头），后端零鉴权改动；禁止用 URL query 传 token（会进访问日志）。

SSE 是本设计唯一新增的传输通道（PowerFS 文章亦要求"SSE 替代单纯轮询"）；Axum 原生支持，无需新依赖。

### 7.2 前端（Angular 15 + Nebular，新增 `pages/cluster-ops/agent/`）

1. **运维工作台**（`incident-workbench`）：左时间线 / 中证据板（按 kind 分组）/ 右根因假设卡（置信度、支持证据、反证、缺口）/ 底部动作区（风险、前置条件、回滚、验证计划、审批按钮）。
2. **事件中心**（`event-center`）：事件列表 + 收敛分组视图（父事件 + 被抑制的次生事件）。
3. **建议卡片组件**（复用现有权限审批交互模式 `pages/cluster-ops/permission-management/approval/`）。
4. **决策审计页**：按 Incident 展示各阶段决策链，可展开 LLM 输入摘要与用量。
5. **Agent 设置页**：自治等级矩阵（动作类型 × L0-L3）、护栏参数、全局开关。

菜单项注册于 `pages-menu.ts`；路由注册于 `pages-routing.module.ts`（沿用现有约定）。

---

## 八、实施路线（v1.1 修订：第一阶段已按 Flink 复刻落地）

路线对照 PowerFS 文章：**跳过其 Step0（Mock 演示先行）**——Stellar 已有真实采集/审计/Profile 数据链路，前端交互模式（Nebular 表格、审批弹窗）已有成熟先例，直接从真实只读诊断开始。

### 第一阶段 对话式只读诊断助手（✅ 已完成，2026-09-11）

范围：复刻 Flink AI assistant 形态（见上文 §〇），非事件闭环。

- [x] Agent loop（`OltpDiagnosisAgent`）：chat → tool_call → 执行 → tool_result → 循环，步骤级审计
- [x] ChatClient：OpenAI 兼容 tool-calling（自建轻量客户端，复用 `llm_providers` active provider）
- [x] 6 个只读工具：query_metrics / query_nodes / query_queries / query_slow_audit / query_profile / query_variables（输出截断 + 白名单防注入）
- [x] 快照上下文 + 5 个诊断 playbook
- [x] 会话持久化（2 张表三方言迁移）+ 会话复用/列表/详情/删除 API
- [x] 权限：`agent` 根资源 4 项动作 + 菜单（三方言语义迁移）

验收（已验证）：
1. mock LLM 下 agent 真实调用工具取数并携带 tool 结果完成二次对话 ✅
2. 会话跨请求复用，历史正确加载 ✅
3. LLM 未配置时返回明确中文错误（400），不崩溃 ✅
4. `cargo clippy --deny warnings` + `cargo test --lib`（326 用例）全绿 ✅

### 第二阶段 前端对话面板 + SSE 流式（进行中）

范围：Angular 15 + Nebular 对话面板（会话列表 / 消息流 / 工具调用链可视化）+ SSE 流式输出（fetch+ReadableStream 带 Bearer，后端 Axum 原生支持）。

- [ ] `pages/cluster-ops/agent/` 对话面板（chat-box + session-list + steps 审计展示）
- [ ] `@core/data/agent.service.ts` 与后端 API 对齐
- [ ] `pages-menu.ts` / `pages-routing.module.ts` 注册
- [ ] SSE：`/api/agent/chat/stream`（30s heartbeat 参照 Flink）
- [ ] 会话标题/删除交互

### 第三阶段 事件闭环（原 M0，Incident-first）

范围：`agent_events` + `agent_incidents` + `agent_incident_events` + `agent_evidences` + `agent_decisions`；事件收敛器；7 个采集器（rules）；规则诊断器；只读 API + 工作台前端；后端配置项 `[agent]`。

验收：
1. 制造 BE 掉线（或注入快照）→ 30s 内自动产生事件 + Incident + 证据 + 诊断结论。
2. 同一 BE 的 5 类次生告警被抑制为 1 个 Incident（收敛率可统计）。
3. 采集服务在事件队列满时不被阻塞（压测：注入 1000 事件，采集周期不漂移）。
4. 每条 Incident 有完整 `agent_decisions` 链（intake → evidence → rule_diagnosis）。

### 第四阶段 根因假设与建议（原 M1，L1）

范围：`agent_hypotheses`；LLM 根因场景（`services/llm/scenarios/ops_root_cause.rs`，复用现有 Provider/缓存/用量）；建议生成；建议卡片前端；自然语言问答（基于 Incident 上下文的单轮问答）。

验收：
1. LLM 输出无证据 ID 引用时被后端拒绝并回退规则摘要（可用 mock Provider 验证）。
2. 证据不足的 Incident 输出缺口清单与下一步只读取证，而非结论。
3. LLM 不可用时系统仍产出规则版结论（断网/禁用 Provider 测试）。

### 第五阶段 动作闭环（原 M2，L2，白名单动作开 L3）

范围：`agent_actions`；Action Gateway（dry-run、前置条件、护栏、节流、并发）；审批 API 与前端；执行与验证器；回滚；两阶段确认（PENDING_ACTION + diff 确认卡，参照 Flink `ActionSubmission`）。

验收：
1. `critical`/`high` 动作未审批时无法执行（API 层拒绝）。
2. dry-run 影响超阈时自动降级为建议。
3. 执行后 60s 自动验证并写入 `verification_status`。
4. 回滚动作可执行且结果可审计。
5. 全局熔断开关生效（一键 L0）。

### 第六阶段 容量与自适应调优（原 M3，含 M2 扩展）

范围：容量趋势预测与水位预警；表访问热度 → 归档/TTL/MV 建议；参数推荐复用（`smart-parameter-recommendation` 集群级化）；资源组配额建议；复盘摘要与"建议沉淀为剧本/阈值调整"。

验收：
1. 磁盘水位趋势预测误差可接受（回放历史快照验证，MAPE 目标 < 15%）。
2. 生成的归档/TTL 建议均带容量收益估算与风险说明。
3. 关闭 Incident 后可一键生成复盘摘要并导出。

---

## 九、明确不做（Non-goals）

1. 不做通用 SOAR/工作流引擎，不做用户自定义 YAML workflow。
2. **不做 LLM 自主工具调用循环（agent loop）**：固定流水线 + 单次 LLM 推理，成本与可审计性优先（对比：SxDevOps 亦得出同一结论）。
3. 不让 LLM 直接执行 SQL/Shell；所有执行必须经过 Action Gateway 白名单。
4. 不新增独立进程、独立存储（不用 RocksDB；审计复用 `AppDb`）。
5. 不做全量日志/Trace 入库；证据只存摘要 + 结构化 payload。
6. 不做多 Agent 协作协议；单 Agent 顺序执行。
7. 不做自动扩容（涉及基础设施，仅输出建议）。

---

## 十、风险与待决问题

| # | 风险/问题 | 现状判断 | 建议 |
| --- | --- | --- | --- |
| 1 | 事件队列满导致丢事件 | 可接受（丢弃 + 计数 + 告警） | M0 观察丢事件率，必要时改为持久化事件表直接落库 |
| 2 | 多租户隔离 | 事件/故障/动作/证据均带 `organization_id`，中间件按组织过滤 | 复用现有 middleware 逻辑（`middleware/`） |
| 3 | 动作审批复用 vs 新建 | 决策：新建 `agent_actions`（语义不同），共用 Casbin 权限码 | 待评审确认 |
| 4 | LLM 成本 | 单 Incident 最多 1 次 LLM 调用 + 缓存 | 复用 `llm_cache`；在 `agent_decisions` 记录 token 用量 |
| 5 | StarRocks/Doris 能力差异 | 动作白名单按两执行通道（adapter / MySQLClient）能力裁剪，缺失能力自动从白名单移除 | 启动时探测（`ClusterType` 分支） |
| 6 | 与 Profile 分析器边界 | 查询级诊断在 Profile 模块，集群级编排在 ops_agent | 文档 §2 已声明，评审确认 |
| 7 | 决策"效果退化"判定标准 | 定义为"同类动作连续 2 次 `no_improvement`" | M2 上线后按真实数据校准阈值 |

---

## 附：与 PowerFS 文章设计要素对照

| PowerFS 要素 | Stellar 对应 |
| --- | --- |
| 管理面旁挂，不侵入 IO 路径 | 同进程模块，采集路径零阻塞（`try_send` + 丢弃计数） |
| 确定性优先，LLM 只补位 | 规则/因果图/剧本先行，LLM 单次调用且冲突时采信规则 |
| 分级自治 L0-L3 + 护栏 | §6.5 / §6.6，新动作默认 L0，需人工显式提级 |
| 可审计、可回滚、dry-run | `agent_decisions` + `agent_actions.dry_run_json/rollback_json` |
| 复用而非重建 | 复用 metrics/audit/profile/LLM/adapter/RBAC/AppDb |
| 失败安全降级 | §6.8 五类降级路径 |
| 决策审计存 RocksDB | 改为存 `AppDb`（Stellar 已有关系库，避免新依赖） |
| IoProfile 低开销采样 | 不适用（Stellar 不采集数据面 IO）；以指标快照与审计日志替代 |

---

## 变更记录（v1.2，2026-09-11）

本版本落地并验证了设计中的 L1/L2 能力与 B2 流式要求，并在开发过程中依据 Flink AI assistant
（内部 fork）提交历史教训补齐了生产化细节：

### B2 SSE 步骤级 + 打字机流式（对齐 Flink AiChatStreamHandler）

- `POST /api/agent/chat/stream`：事件 `step`（工具链）/ `delta`（模型内容增量，打字机）/
  `answer`（完整答案，覆盖增量防丢）/ `done`；30s `KeepAlive` 注释行心跳
  （Flink 77f03445a97：代理空闲超时断流；KeepAlive 随响应流生命周期销毁，无泄漏——
  Flink 76594c573b8 曾踩"心跳任务独立于连接"的坑）
- LLM 调用全量走流式（`ChatClient::chat_stream`，OpenAI SSE 协议）：token 级增量实时推送，
  前端"思考中"气泡打字机展示；回合结束按 tool_calls 归位——思考段进 reasoning step、
  纯文本进最终答案（与 Flink publishIfMissing 语义一致）
- 流式工具调用 delta 按 index 累积（id/name/arguments 分片拼接），空 name delta 容错
  （Flink 69ce7741c23）
- 取消传播：客户端断开 → sink 关闭 → agent 下一轮边界 is_closed 提前退出（Flink 9673029ef05）
- 前端 fetch + ReadableStream 解析 SSE（Bearer 头，token 不进 URL），delta/step/answer 实时渲染

### L1 LLM 根因分析（第四阶段）

- `POST /api/agent/incidents/:id/analyze`：证据紧凑摘要（payload 截断 300 字符）→ 结构化假设契约
  → 后置校验（证据 ID 必须存在/无引用丢弃/置信度钳制/全非法回退规则摘要）→ `agent_decisions`
  全量留档 → 假设合并 `incident.output_json`
- 默认仅手动触发（成本护栏）

### L2 动作闭环（第五阶段，对齐 Flink PlatformWriteTools 两阶段确认）

- `agent_actions` 表：kind（kill_query / update_variable，白名单）/ status
  （pending/executing/executed/failed/cancelled/expired）/ created_by / confirmed_by /
  expires_at / result_json
- 两阶段确认：创建（高熵 UUID，不可猜测——Flink ba2d615a0e5）→ 确认（原子翻转 pending→executing
  单次执行，重复/并发拒绝；TTL 15 分钟过期拒绝）→ 执行（MySQLClient 协议通道，与既有
  handlers/query.rs、variables.rs 同源）→ 结果留档
- 参数白名单校验防 SQL 注入；handler 层 incident 归属校验（非超管限本组织集群）

### 生产化修正（Flink 提交历史教训）

1. 会话复用归属校验（会话必须存在且 channel/cluster 匹配，防跨集群串写——Flink 8136edeff35）
2. 上下文字符预算 `MAX_HISTORY_CHARS=60k`（工具结果可达数十 KB，条数截断不够——
   Flink 5b6d5af36a2）
3. 事件表保留策略 `event_retention_days=14`（每 tick 裁剪，事实表可重建）
4. 会话并发串行化：per-session Semaphore(1)，同一会话同时只有一个 turn（含 user 落库）——
   Flink 5b6d5af36a2 "queue the next question until the current turn finishes"
5. 会话保留：30 天无活动清理（进程内 24h 惰性执行）
