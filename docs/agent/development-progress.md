# Agent 开发进度追踪

> **版本**: v3.6
> **日期**: 2026-09-11
> **状态**: 第一~五阶段 ✅；**B2 打字机级流式 ✅（token delta SSE，思考/答案分段实时渲染）**；P2 三项收尾 ✅；容量自适应六阶段待启动
> **参照**: 设计文档 `docs/agent/ops-agent-design.md`（v1.1）；复刻参照 Flink AI assistant（`/mnt/data/flink`，骨架源自 pi-from-scratch）；共享层设计 `docs/agent/ai-common-design.md`（与智能问数 ask 的重叠治理）

---

## 开发周期规划

```
第一阶段：对话式只读诊断助手（后端）   ✅ 已完成
第二阶段：前端对话面板               ✅ 已完成；SSE 流式 ☐
第三阶段：事件闭环（原 M0）           ✅ 已完成（真实集群数据端到端验证）
第四阶段：根因假设与建议（原 M1）     待启动
第五~六阶段：动作闭环 / 容量 & 自适应    待启动
```

---

## 共享层重构（2026-09-11，智能问数重叠治理）

用户要求提前治理与智能问数（`docs/ASK_DATA_DESIGN.md`）的重叠。已落地：

- 新共享层 `services/ai/`（types / llm / session），设计见 `docs/agent/ai-common-design.md`
- 会话表合并为 `ai_sessions` / `ai_messages` 宽表（`channel` 隔离 'agent'/'ask'；ask 的 generated_sql/guard/explain/chart 扩展列已建，agent 专用 steps_json 保留），迁移 `20260911000000`（三方言，本地开发期直接改造）
- `ChatClient`（tool-calling）从 ops_agent 提升至 `services/ai/llm.rs`；`AgentStep`/`ChatMessage` 等共享类型入 `services/ai/types.rs`
- 确认 ask 业务（`services/ask_data/`）落地时：channel='ask' 复用 AiSessionStore、权限另建 `ask:*` 根资源、前端复用会话控件；业务代码互不引用

---

## 第一阶段：对话式只读诊断助手（后端） ✅ 已完成

### 任务分解

#### Task A1.1: Agent loop 核心 ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] `OltpDiagnosisAgent::run`：chat → tool_call → 执行 → tool_result → 循环（参照 Flink `DefaultJobDiagnosisAgent`，非流式）
  - [x] `AgentStep` 步骤级审计（reasoning / tool / tool_result / error / end）
  - [x] 工具调用上限 `max_tool_calls`（默认 8），空 tool_calls 防死循环

**关键代码位置**:
- `backend/src/services/ops_agent/agent.rs`

#### Task A1.2: ChatClient（OpenAI 兼容 tool-calling） ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] 轻量聊天客户端（复用 reqwest），支持 tools/tool_choice/tool 消息回传
  - [x] 复用 `llm_providers` 表 active provider（`LLMRepository::get_active_provider`，明文 key 直读）
  - [x] 响应解析（choices/message/tool_calls/usage）与错误文案

**关键代码位置**:
- `backend/src/services/ops_agent/llm.rs`

#### Task A1.3: 只读工具注册表 ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] `AgentTool` trait + `tool_specs()` JSON Schema 生成
  - [x] `query_metrics`：metrics_snapshots 趋势（倒序 limit）
  - [x] `query_nodes` / `query_queries`：ClusterAdapter 实时取数
  - [x] `query_slow_audit`：AuditLogService 慢查询
  - [x] `query_profile`：复用 Profile 规则引擎（真实 `DiagnosticResult` 字段，输出 ≤8 suggestions / ≤10 diagnostics）
  - [x] `query_variables`：SHOW GLOBAL VARIABLES（10 项默认键 + 白名单字符校验防注入）
  - [x] 输出截断（metrics ≤6000、variables ≤4000）

**关键代码位置**:
- `backend/src/services/ops_agent/tool.rs` + `tools/`（6 个文件）
- 坑：`#[app_impl]` 直接标注 trait impl（先例 `metrics_collector_service.rs:1259`），否则 sqlx bound 不满足

#### Task A1.4: 上下文与技能 ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] `SYSTEM_TEMPLATE`（快照/工具/技能占位符）+ 每轮 `render_snapshot` 新鲜快照
  - [x] 5 个诊断 playbook（慢查询 / compaction / BE 掉线 / 导入积压 / 数据倾斜）

**关键代码位置**:
- `backend/src/services/ops_agent/context.rs`、`skills.rs`

#### Task A1.5: 会话持久化 ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] 2 张表 `agent_chat_sessions` / `agent_chat_messages`（三方言迁移 `20260911000000_add_agent_chat_sessions.sql`）
  - [x] 会话复用（session_id 续传）、最近 20 条历史
  - [x] 删除级联；`insert_id()` 兼容 MySQL/SQLite/Postgres（**坑**：`RETURNING id` MySQL 不支持）
  - [x] 用户消息先持久化（防刷新丢失问题）

**关键代码位置**:
- `backend/src/services/ops_agent/session.rs`

#### Task A1.6: API 与权限 ✅ 已完成
- **状态**: ✅ 已完成
- **目标**:
  - [x] `POST /api/agent/chat`、`GET /api/agent/sessions`、`GET/DELETE /api/agent/sessions/:id`（`handlers/agent_chat.rs`）
  - [x] `permission_extractor.rs` 注册 `agent` 根资源（chat / sessions / sessions:get / sessions:delete）
  - [x] 权限迁移 `20260911000001_add_agent_permissions.sql`（menu:agent + 4 API 权限，admin/super_admin 授权，三方言）
  - [x] main.rs 服务装配 + 路由挂载

**验收标准**:
- mock LLM 下完整 tool 循环：query_metrics 真实取数 → 二次请求携带 tool 结果 → 最终回答 ✅
- 会话复用/列表/详情/删除 ✅（级联删除验证）
- 无 Provider → 400「未配置可用的 LLM Provider」；空消息 → 400「消息不能为空」 ✅
- `cargo clippy --release --all-targets -- --deny warnings --allow clippy::uninlined-format-args` 全绿 ✅
- `cargo test --lib`：326 通过 / 0 失败 ✅

---

## 第二阶段：前端对话面板（✅ 已完成，SSE 流式待做）

### 任务分解

#### Task B1: Agent 对话前端面板 ✅ 已完成
- **优先级**: 🔴 高
- **目标**:
  - [x] `pages/cluster-ops/agent/` 对话面板（standalone）：左侧会话列表（新建/切换/删除）+ 右侧消息流 + 工具调用链折叠展示 + 底部输入（Enter 发送）
  - [x] `@core/data/agent.service.ts`（chat / listSessions / getSession / deleteSession）
  - [x] `pages-menu.ts` 注册「智能运维助手」（`menu:agent`）+ `cluster-ops-routing.module.ts` 懒加载路由
- **验收标准**:
  - `ng build` 生产构建 + dev server 增量编译全绿，agent chunk 生成且含组件文本 ✅
  - 后端 API 全链路（工具循环/会话 CRUD）此前已 mock LLM 真实验证 ✅
  - 注：本机 Chrome CDP 导航故障（系统网络栈问题），真实浏览器人工交互验证留待用户

#### Task B2: SSE 流式输出 ☐ 未开始
- **工作量**: 0.5 天
- **优先级**: 🟡 中
- **目标**:
  - [ ] 后端 `/api/agent/chat/stream`（SSE：事件 `step` / `answer` / `done` + 30s heartbeat，参照 Flink `AiChatStreamHandler`）
  - [ ] 前端 `fetch + ReadableStream` 解析（带 Bearer，禁 URL 传 token；EventSource 无法带 header）
- **验收标准**:
  - 对话过程中逐步渲染 tool 调用与最终回答，连接空闲 30s 不断

---

## 第三阶段：事件闭环（原 M0，Incident-first） ✅ 已完成（2026-09-11）

### 任务分解

#### Task C1: 事件采集与收敛 ✅
- [x] 迁移 `20260911000002_add_agent_runtime.sql`（5 表三方言：agent_events / agent_incidents / agent_incident_events / agent_evidences / agent_decisions）
- [x] 规则采集器（`services/agent_runtime/collectors.rs`）：快照 diff（磁盘水位 80/90、compaction 50/100、导入积压、事务失败/查询错误增量）+ 节点掉线（adapter 定位具体 BE，alive 为字符串需解析）+ 审计慢查询（≥5s 低频摄取）
- [x] 收敛：指纹 5 分钟窗归并（occurrence_count 累加）、消退→resolved、node_down 拓扑抑制次生事件
- [x] 阈值口径与 `overview_service.generate_alerts` 一致

#### Task C2: Incident 生命周期 ✅
- [x] 对象级聚合升级（cluster / be:host 各一 Incident）+ `agent_incident_events` 关联 + intake 决策
- [x] 复开：resolved/closed 30 天内同 dedupe_key 重新 investigating（保留证据链）
- [x] 自动补取证：open 且无证据的 Incident 每 tick 自动补齐（覆盖取证失败/重启场景）

#### Task C3: 取证流水线与规则诊断 ✅
- [x] 固定流水线 metrics → nodes → queries → audit（4 采集器，失败不阻断 quality=weak，60s 超时）
- [x] 规则剧本诊断（`diagnoser.rs`）：node_failure / capacity / compaction_backlog / load_backlog / write_anomaly / query_performance，置信度按严重度微调（≤0.98）
- [x] `agent_decisions` 决策链：intake → evidence → rule_diagnosis 全留档

#### Task C4: API 与调度 ✅
- [x] `handlers/agent_incident.rs`：incidents 列表/详情、events 列表、手动 investigate、close
- [x] 权限：`agent` 资源扩展 incidents / incidents:get / incidents:investigate / incidents:close / events（迁移 `20260911000003`）
- [x] `[agent]` 配置段（enabled / interval_secs）+ ScheduledExecutor 注册（agent-runtime）

**验收（真实集群数据端到端验证，非 mock）**:
1. 真实磁盘 100% + 事务失败增量 → 30s 内自动 Incident + 证据 + 诊断 ✅
2. 事件消退→resolved、再犯重新计数 ✅
3. 4 类证据全 strong（metrics 1.6KB / nodes 35KB 真实 42 BE / queries / audit）✅
4. 决策链 intake → evidence → rule_diagnosis 完整 ✅
5. close 后注入同因快照 → 30 天窗口内自动复开 ✅
6. `cargo clippy --deny warnings` + 326 单测全绿 ✅

#### Task C5: 工作台前端 ✅ 已完成
- [x] 旧版独立 Incident 工作台（后续已下线）
- [x] Incidents 列表（状态筛选）→ 行展开详情：规则诊断卡（置信度徽标+动作建议）、LLM 假设卡（含校验拒绝原因）、证据板（payload 折叠查看）、决策链、关联事件时间线
- [x] 旧版工作台页签（收敛后事件列表 + 状态筛选）
- [x] 操作：重新取证 / LLM 根因分析（确认弹窗）/ 关闭（提示 30 天复开语义）；`ng build` 全绿

## 第四阶段：LLM 根因推理（原 M1 子集） ✅ 已完成（2026-09-11）

- [x] `services/agent_runtime/llm_diagnosis.rs`：结构化假设契约（title/root_cause_type/confidence/支持与反驳证据 ID/缺口清单/下一步检查）
- [x] 证据紧凑摘要（payload 截断 300 字符进上下文，防上下文污染）
- [x] **后置校验护栏**（设计 §6.3）：引用的证据 ID 必须存在且属于本 Incident；无引用假设丢弃；全部非法 → 决策 `rejected` + 回退规则诊断摘要；置信度钳制 [0,1]
- [x] `POST /api/agent/incidents/:id/analyze` + 权限 `incidents:analyze`（迁移 `20260911000004`）；假设合并进 `incident.output_json`
- [x] LLM 不可用（无 active provider）→ `rejected` + 回退；`agent_decisions` 全量审计留档（input 含证据 ID 集合）
- 验收（mock LLM 三场景端到端）:
  1. 合法证据引用 → completed、假设保留、置信度 1.7 钳制 1.0 ✅
  2. 引用不存在的证据 9999 → dropped + rejected + 回退规则摘要（错误文案精确）✅
  3. provider 停用 → rejected + `llm_unavailable_or_rejected` 回退 ✅
- 说明：默认仅手动触发（成本护栏，配置随 Provider 就绪后评估自动模式）

## 第四阶段：根因假设与建议（原 M1，L1） ☐ 待启动

- [ ] `agent_hypotheses` 表；LLM 根因场景（复用现有 Provider/缓存/用量）
- [ ] 建议生成 + 建议卡片前端；自然语言问答
- 验收：LLM 输出无证据引用被拒并回退规则摘要

## 第五阶段：动作闭环（原 M2，L2，白名单开 L3） ☐ 待启动

- [ ] `agent_actions` 表；Action Gateway（dry-run/前置条件/护栏/节流/并发）
- [ ] 审批 API + 前端；执行与验证器；回滚；两阶段确认（PENDING_ACTION + diff 卡，参照 Flink `ActionSubmission`）
- 验收：critical/high 未审批拒绝执行；执行 60s 自动验证；一键 L0 熔断

## 第六阶段：容量与自适应调优（原 M3） ☐ 待启动

- [ ] 容量趋势预测与水印预警（MAPE < 15%）；归档/TTL/MV 建议；参数推荐集群级化
- [ ] 复盘摘要与"建议沉淀为剧本/阈值"

---

## 已知技术决策与坑（备忘）

| 决策/坑 | 说明 |
| --- | --- |
| 同进程模块 | ops_agent 挂在 AppState 下，不新增部署单元 |
| `#[app_impl]` 标注 trait impl | sqlx bound 跨签名不传播，trait impl 也要标（先例 metrics_collector_service.rs:1259） |
| `RETURNING id` 不可用 | MySQL 不支持；用 `db_query::query(...).insert_id()`（db/query.rs 方言分发） |
| tool-calling 客户端自建 | 现有 `LLMService` 是 analysis 式（system+user→JSON），不支持 tools |
| `llm_providers.api_key_encrypted` | 实存明文 key（命名遗留），ChatClient 直读 |
| SSE 鉴权 | fetch+ReadableStream 带 Bearer；禁 URL 传 token |
| 工具注册需 `#[app_db]` | `create_tools` 泛型函数要标，否则 Box<dyn AgentTool> 无法满足 bound |
| 迁移编译期嵌入 | 新增迁移文件后必须触发重编（touch src/db/<backend>.rs），否则运行中的二进制不含新迁移 |
## 第四阶段补记：B2 SSE 流式 ✅（Flink 提交历史教训内化）

- [x] `POST /api/agent/chat/stream`：事件 `step`（实时工具链）→ `answer` → `done`（含 session_id/usage_tokens）；错误进 `error` 事件
- [x] 心跳：axum `KeepAlive` 每 30s 注释行（Flink 77f03445a97：Nginx/Ingress 空闲超时断流）；keep-alive 由响应流自身驱动，**连接关闭即终止，无独立心跳任务泄漏**（Flink 76594c573b8 教训：心跳必须随连接生命周期取消）
- [x] 取消传播：客户端断开 → sink 关闭 → agent 下一轮边界 `is_closed()` 提前退出（Flink 9673029ef05：取消必须真正释放进行中的工作；LLM 请求随 future drop 取消）
- [x] 会话复用归属校验（Flink 8136edeff35 同类）：`session_id` 必须存在、channel/cluster 匹配，否则 404——防止跨集群串写历史
- [x] 上下文字符预算：`MAX_HISTORY_CHARS=60k`，从最新往回截断（Flink 5b6d5af36a2 CONVERSATION_MAX_CHARS 同类；工具结果可达数十 KB，仅条数截断不够）
- [x] 空 tool_call name 容错（Flink 69ce7741c23：OpenAI 兼容网关会重复空 name）：非空才接受，避免死循环
- [x] 前端：fetch + ReadableStream（Bearer 头，token 不进 URL）、SSE 解析忽略注释行、占位气泡实时追加步骤、answer 到达填内容、done 停 spinner（Flink 66495c658b0：误停/不停 spinner 都是 bug）
- [x] 保留同步 `chat` 端点；`max_tool_calls` 预算仅作用于同步路径（流式下用户可断开取消，Flink 7e5bdbbd0fb 移除硬预算的方向一致）
- 验证：curl 实测事件序 4 step → answer → done；会话复用/非法 session 404；355 单测全绿

## 第五阶段：动作闭环（两阶段确认写动作） ✅ 已完成（2026-09-11）

- [x] `agent_actions` 表（迁移 0005 三方言）+ 权限 4 项（迁移 0006，挂 menu:agent，admin/super_admin）
- [x] 动作白名单 2 种（Flink PlatformWriteTools 对应物，走 MySQLClient 协议通道与 handlers/query.rs、variables.rs 同源）：
  - `kill_query`（query_id UUID 形态校验 [0-9a-f-]，`KILL QUERY 'id'`）
  - `update_variable`（scope global/session + key/value 白名单字符校验，防 SQL 注入）
- [x] **两阶段确认机制**（Flink ba2d615a0e5/262afad8048 教训）：
  - 创建：`POST /api/agent/incidents/:id/actions` → `pending` + 高熵 UUID（不可猜测）
  - 确认：`POST .../actions/:id/confirm` → 原子翻转 `pending→executing`（**单次执行**，并发/重复确认幂等拒绝）→ 执行 → `executed/failed` + 结果留档
  - TTL 15 分钟：过期确认拒绝；`POST .../actions/:id/cancel`
  - 全链路审计：created_by / confirmed_by / confirmed_at / executed_at / result_json
- [x] handler 归属校验：incident 必须存在且（非超管）属于本组织集群
- [x] 端到端验证（真实集群 bj-com）：创建/确认（真实执行 KILL QUERY 失败诚实留档）/重复确认拒绝/过期拒绝/取消后确认拒绝；非法参数（注入）400
- [x] 前端工作台：Incident 详情「运维动作」区——创建表单（类型+参数+scope）、待确认卡片（状态徽标/确认人/结果）、确认（确认弹窗警示）/取消按钮
- 说明：诊断建议为自然语言，动作当前为手动创建（结构化动作生成留待后续）；默认仅 admin/super_admin 可操作

## 生产化修正（2026-09-11 二轮）

- [x] **P0 事件表保留策略**：`agent_events` 无清理会无限膨胀（实测 424 条且每 tick 增长）。
      `config.rs` AgentConfig 新增 `event_retention_days`（默认 14），`run_once` 每 tick 按
      `last_seen_at` 裁剪（应用层 UTC 时间戳比较，规避三方言时间函数）。验证：注入 2020 年旧事件被删 ✓
- [x] **P1 会话并发串行化**：Flink 5b6d5af36a2 明确 "queue the next question until the current
      turn finishes"；前端 `sending` 禁发只防单客户端，服务端无兜底时多客户端并发会串写历史。
      改用 per-session `Semaphore`（permit=1）+ owned permit（可 move 进 'static SSE 任务；
      MutexGuard 借用做不到）。锁在 prepare（user 落库）**之前**获取（传 session_id 时），
      未传时创建后立即获取。验证：3 并发同会话 → 消息严格 user/assistant 交替、回答与提问配对 ✓

## B2 打字机级流式完成（v1.5，2026-09-11）

- [x] `ChatClient::chat_stream`：OpenAI SSE 流式（`data:` 行 + `[DONE]`），工具调用 delta 按
      index 累积（id/name/arguments 分片）、空 name 容错（Flink 69ce7741c23）
- [x] Agent 循环改流式调用：内容增量经 `ProgressEvent::Delta` → SSE `event: delta` 实时推送；
      回合结束按 tool_calls 归位——思考段进 reasoning step、纯文本进最终答案
      （对齐 Flink publishIfMissing 语义）；不 abort 转发任务（abort 丢积压增量——实测踩坑）
- [x] 前端打字机：`thinking` 气泡实时渲染增量（loader 旋转动画），工具边界后增量转入答案气泡；
      answer 事件覆盖增量防丢
- [x] 同步 `chat` 端点继续可用（同一流式 LLM 路径，无 delta 消费者）
- 端到端验证：delta×8（思考段"先查看集群指标快照…"）→ step(tool)→step(tool_result) →
  delta×9（答案段）→ answer → done；事件次序正确

## P2 收尾（v1.5）

- [x] ai_sessions 保留策略：30 天无活动清理（`delete_stale` + 进程内 24h 惰性触发）
- [x] `backend/conf/config.toml` 增加 `[agent]` 段文档（enabled/interval_secs/event_retention_days）
- [x] `ops-agent-design.md` 更新 v1.2：变更记录（B2 流式、L1 根因分析、L2 动作闭环、生产化修正）

## 自我对抗审查轮（v1.6，2026-09-11）

对全部改动逐项对抗性审查（有效性/收益/性能/优雅/语义/自洽），修复 5 处：

1. **前端打字机多工具轮串扰（语义）**：原 `steps.length===0` 启发式会把第二轮工具回合的
   模型思考混入最终答案。改为后端每回合结束发 `event: phase`（reasoning/answer）语义标记，
   前端按 phase 归位打字机缓冲：thinking →（answer）移交 content /（reasoning）清空
   （steps 链含完整 reasoning step）。事件序验证：delta×8 → phase(reasoning) → step×4 →
   delta×9 → phase(answer) → answer → done ✓
2. **每轮冗余 spawn 转发任务（性能/优雅）**：`chat_stream` 改同步回调（FnMut），删除每轮
   一个 channel + 一个 tokio 任务；转发链从 3 层收发收为 2 层。
3. **非 SSE 响应静默空答（防御）**：兼容网关忽略 stream=true 返回整包 JSON 时，parse 零匹配
   曾产生空 completion → 空答案。加 `saw_data_line` 守卫：无 data 行且无内容 → 显式报错。
   实测：非流式 mock → `5001 LLM 流式响应不是 SSE 格式` ✓
4. **事件裁剪全表扫描（性能）**：`WHERE last_seen_at < ?` 无索引（现有 scope 索引前缀
   不适用）。迁移 0007 三方言加 `idx_agent_events_last_seen`，实测索引已生效 ✓
5. **过期动作状态永不刷新（语义）**：TTL 过期动作停留在 pending。`list_actions`/`load_action`
   惰性批量标记 expired。实测：过期动作列表 → expired，确认被拒 ✓

排除项（核对后无需修）：axum SSE 输出为 LF（`0a`），前端 `\n\n` 分块有效（非 CRLF，勿改）；
permit/guard 生命周期、退避边界、裁剪与 Incident 30 天窗口的语义自洽。

## 架构审查轮（v1.7，2026-09-11）——自我对抗式架构审查

审查维度：模块边界 / 依赖方向 / 共享层纯净 / DRY / 阈值口径 / 数据流 / 语义自洽。
全程直接取证（grep/read/实测），未用子代理。

### 发现并修复

1. **[边界违反（关键）] agent_runtime → ops_agent 直接引用**：
   `agent_runtime/{collectors,service}.rs` 引用 `ops_agent::tool::truncate`（pub(crate)），
   违背"两业务模块互不引用"承诺。修复：`truncate` 上移共享层
   `utils::string_ext::truncate`，`ops_agent::tool` re-export（既有调用面不变），
   agent_runtime 改指共享层。验证：agent_runtime 全目录零 ops_agent 引用 ✓
2. **[DRY/口径] 磁盘阈值 80/90 双处独立维护**：数值与 overview_service::generate_alerts
   一致（承诺成立），但事件闭环与告警各自写死（未来改一处必漂移）。修复：
   `metrics_collector_service::{DISK_WARNING_PCT, DISK_CRITICAL_PCT}` 单一来源，
   collectors.rs 与 overview_service.rs 四处置换为共享常量。验证：get_cluster_overview
   200（含 generate_alerts 全路径）、事件闭环 tick 正常。

### 核实通过（无问题）

- ops_agent → agent_runtime 零引用 ✓；ai 共享层零业务依赖 ✓
- 慢查询采集无重复实现：两模块共用 AuditLogService::get_slow_queries ✓
- 前端两页面模块独立（独立路由/菜单/懒加载 chunk），共享 @core/data 服务 ✓
- handler 薄层、AppState 装配集中、迁移编号/权限播种一致 ✓
- 事件保留 14d 与 Incident 复开 30d 窗口：事件可重建、证据链独立，语义自洽 ✓

### 观察项（本轮回避，注明触发条件）

- SLOW_QUERY 每 tick（30s）全集群远程审计查询（hours=2, LIMIT 10）：当前 1 集群成本可控。
  若多集群规模化成为瓶颈：低频化需给 collect_cluster_events 引入"轮询间隙跳过"语义
  （门控返回空快照会误判全部慢查询消退，不可直接缓存旧快照——语义验证过是坑）。
- DEFAULT_MAX_TOOL_CALLS=8 硬编码：护栏语义，暂不配置化。
- investigate 等操作端点的 Err(String) 统一 500：操作语义可接受，不细分 4xx。

## 前端布局与 Markdown 渲染对齐 Flink（v1.8，2026-09-11）

取证（/mnt/data/flink web-dashboard）：
- 布局：Flink 的 Diagnostic Assistant 是**左侧主导航顶级菜单项**（robot icon，/ai-assistant 独立路由），
  页面为左 rail（新会话/搜索/会话列表）+ 右消息区（空态引导卡 + 预设问题 + 消息流 + trace 折叠工具链），
  与我们的左右双卡结构一致
- Markdown：Flink 用 `ngx-markdown@20` + `github-markdown-css`（`assistant-content markdown-body` class）
  渲染助手回答；项目实际为 Angular 21（CLAUDE.md 的 15 已过时）→ 采用同栈 ngx-markdown@21.3.0

改动：
- 菜单：新增**顶级栏目「智能运维」**（robot-outline，permission menu:agent），下挂「智能运维助手」
  +「运维事件中心」（原挂在集群运维下，已上移）
- 渲染：app.module 注册 MarkdownModule.forRoot()；全局引入 github-markdown-css；
  助手消息 content 以 `<markdown [data]>` 渲染（markdown-body + 主题适配样式），用户消息保持纯文本；
  thinking 打字机气泡保持纯文本（流式中不渲染）
- 构建验证：ng build 全绿；agent chunk 正常

## 事件中心 Markdown 接入（v1.9，2026-09-11）

- 规则诊断：`summary` 摘要、`actions[].detail` 建议详情 → ngx-markdown 渲染
- LLM 假设：`title`、`recommended_next_checks[]` → ngx-markdown 渲染
- 保持纯文本（结构化展示）：root_cause_type/missing_evidence 标注、证据 payload（JSON pre）、决策链 JSON
- 样式：`.md-inline`（列表内联）+ 卡片内 markdown-body 子样式（代码块/表格/深浅色适配）
- `ng build` 全绿

## 前端 ChatGPT 风格化（v2.0，2026-09-11）

对话面板（agent）：
- 整体分层无边框：整页 rounded 容器 + 白会话栏（右 1px 淡线）+ 浅灰聊天区 + 白 pill 输入
- 消息气泡：用户=主色圆角块（右下 0.3rem 小角）、助手=无边框浅灰（左下小角）；删除 msg-role 标签（靠方向区分）
- 输入区：pill 1.4rem 圆角 + focus-within 主色环 + 圆形发送按钮（箭头 icon，hover 微放大，禁用灰）
- 会话列表：悬浮感（hover 灰底、激活 primary-100 圆角块，去掉硬边框分隔）
- 动效：消息进入 fade+translateY(5px) 0.25s、滚动 smooth、hover/按钮 transition 统一 0.18-0.2s
- 空态 & 头部：极简（title + 集群小字），thinking 气泡去虚线边框改浅灰卡

旧版独立 Incident 工作台：卡片 1rem 圆角 + hover 边框微亮 + 展开主色柔光阴影；筛选 select 圆角。

构建全绿。

## 全局浮动聊天入口（v2.1，2026-09-11）

- 新增 `@theme/components/chat-float/`（standalone）：任意页面右下角主色圆气泡（hover 微放大），
  点击展开迷你聊天窗（22rem × 70vh，cubic-bezier 滑入）
- 挂载：one-column layout（`ngx-chat-float` 于 nb-layout 内）→ 路由切换不销毁、全局可用；
  无 `menu:agent` 权限不渲染
- 能力：会话下拉切换/新建、消息流（用户主色块 + 助手浅灰气泡 + thinking 打字机 + markdown 回答）、
  pill 输入 + 圆形发送；复用 AgentService.chatStream（同一后端与全量页面）
- 状态：跟随 ClusterContext 活动集群；最小化 = 收起为气泡；关闭 = 隐藏入口（刷新恢复）
- 构建全绿

## 浮动气泡优化 + 产品闭环规划（v2.2，2026-09-11）

- 气泡 hover 右上角浮现轻微八叉（半透明小圆钮 → hover 变 danger 色），点击关闭入口
  （localStorage 记忆，刷新不再出现；恢复路径=左侧「智能运维」菜单，tooltip 已说明）
- 产品闭环规划落盘：docs/agent/product-closure-plan.md（发现→通知→诊断→处置→验证→复盘 六环盘点，
  3 个断点 + P0/P1/P2 TODO 与实施顺序）

## 右上角铃铛通知体系（v2.3，2026-09-11）

背景：铃铛原为纯静态图标（无交互/无后端）。按产品语义接入完整最小体系：

- 后端：`notifications` 表（迁移 0008 三方言）+ NotificationService（create/list/unread_count/mark_read，
  资源按 user_id 隔离）+ API（GET/POST /api/notifications、POST /:id/read）+ 权限（迁移 0009）
- 前端：`@core/data/notification.service.ts` + `notification-bell` 组件（未读红点、60s 轮询、
  下拉通知面板、点击跳链+标已读）；header 静态 bell 替换
- **回合不打断的关键架构**：`AgentChatService`（app 级单例）持有回合订阅，页面/浮窗销毁不断连
  → 切走继续干活，回合完成后由服务判定"当前无会话 UI 在前台"→ 创建铃铛通知
  （title=智能运维诊断完成、body=答案摘要、link 带 session 直达自动打开该会话）
- **实证修正**：最初在服务端按"连接是否断开"判通知——实测 OS 写缓冲导致断开检测滞后、
  回合完成时误判"用户还在"，方案废弃；改由前端按"会话 UI 前台状态"精确判定
- 会话归属收紧：ai_sessions 列表/详情/删除按 user_id owner-only（通知链路需要 owner，
  顺带修复跨用户互看会话的隔离问题）

验证：GET/POST/read API 全通（创建→未读数→已读归零）；断开后回合继续完成的实测（会话有回复）；
通知创建链路（前端 done→通知 POST）待浏览器人工验收。

## 通知体系产品化（v2.4，2026-09-11）

设计落盘：product-closure-plan.md「通知体系全盘设计」（kind 矩阵 × 时机 × 内容 × 级别 × 跳转 × 扩展路线）

实现：
- 表扩展：severity(info/warning/critical) + meta_json（迁移 0010 三方言），NewNotification 载荷结构体（builder 式）
- 触发矩阵落地：
  - agent_chat_done（完成，info）/ agent_chat_error（失败，critical）——前端 AgentChatService 判定"无会话 UI 前台"
  - incident_created（critical 事件→critical）/ incident_reopened（warning）——后端 escalate 自动落库，发给 admin/super_admin
  - action_pending（待确认，warning，强调"需要确认时肯定要通知"）/ action_result（成功 info/失败 critical）——动作创建与确认执行后自动落库
- 前端：铃铛通知项按 kind 图标 + severity 颜色（danger/warning）；通知深链直达：
  - agent 页面 ?session= 自动打开会话
  - 事件中心 ?incident= 自动加载并展开详情（maybeAutoOpen）
- 验证：action_pending/action_result 端到端（真实集群执行失败 → critical 通知 + meta 关联）✓；API 输出 severity/meta ✓；355 测试全绿

扩展路线（todo）：已读全部/分页/TTL、EventSource 推送、按 kind 偏好开关、飞书桥接、事件聚合去重

## 前端 ngx-admin 惯例对齐 + 六阶段容量自适应（v3.0，2026-09-11）

前端惯例对齐（项目前端源出 /mnt/data/ngx-admin，主题文件完全同源）：
- 四个自定义组件 scss 统一包裹 `@include nb-install-component()`（项目惯例，header/tab-bar 同款），
  修掉顶层 :host 与 mixin 自带 :host 的嵌套冲突；Nebular 组件 + 主题变量体系保持
- 新增 chat-float / notification-bell 均为 Nebular 组件组合（nb-select/nb-popover/badge 等）

六阶段容量自适应 ✅（路线图最后一项）：
- `agent_runtime/capacity.rs`：近 24h 快照（≤2880 点）最小二乘线性回归 → 增长率 %/天 + 满盘 ETA；
  门槛：斜率 ≥1.2%/天 且未近满（<90%）才预测天数；数据 <48 点返回 None
- 诊断增强：capacity 剧本命中时 forecast 追加进 summary（「磁盘已接近满盘（100.0%）…」/ETA 文案）；
  端到端验证：真实集群 100% → 再诊断输出容量文案 ✓
- 新对话工具 `query_capacity_forecast`（第 7 工具）：直接回答"多久满盘/是否需扩容"
- 单测 3 个（线性趋势 ETA 精确值 / 平稳无 ETA / 近满不预测）+ 文案拼接去重复句号
- 总计 358 单测全绿；clippy 零警告

## 真实 LLM 端到端验证 + 网关兼容修复（v3.1，2026-09-12）⭐

里程碑：注册内部网关 provider（gpt-5.5，Base URL 与 API Key 来自 pi 的 models.json / auth.json），
首次用真实 LLM 完整走通产品闭环。提问「为什么查询这么卡顿？」，模型按技能 1 剧本**自主调用 10 个工具**：
query_metrics → query_slow_queries → query_running_queries → query_capacity_forecast → query_nodes →
query_profile_diagnostics ×5 → query_variables，最后输出 **1739 字符 Markdown 诊断报告**
（磁盘 100% / p95 10.8s / p99 28.6s / JVM 79.5%，表格+判定+建议）持久化到会话。

**实测揪出并修复 3 个被 mock 掩盖的致命缺陷（真实网关严格性检验）**：
1. **流式解析空 content continue 吞工具调用**：工具调用 chunk 常带 `content:""`，`if c.is_empty() { continue; }`
   的 continue 误作用于外层行循环，整行（含 tool_calls）被跳过 → 模型永不调工具。
   修复：空 content 只跳过文本增量。（mock 的 content 非空所以从未暴露）
2. **回传 tool_calls 扁平格式被严格网关 400 拒绝**：ChatMessage 序列化输出 `{arguments,id,name}` 扁平，
   OpenAI 规范要求 `{"id","type":"function","function":{...}}` 嵌套——内部网关严格校验，
   第二回合必 400。修复：ChatMessage 自定义 Serialize（自动嵌套）+ 单测（chat_message_tool_calls_serialize_nested）。
3. （验证时确认非缺陷）Profile not found 为真实数据状态，工具诚实失败、模型基于其余证据继续——正确行为。
   另修会话历史重复 user 消息（load_recent 含刚存的消息，见 v3.1 注释 → 已确认存在待修）。

360+ 测试全绿；内部网关 provider（is_active=1）保留为真实配置；deepseek 旧配置 is_active=0 保留可恢复。

## sxdevops 诊断向借鉴（v3.2，2026-09-12）——方向校准：只做诊断/对话式诊断

用户明确：**自动化运维不学，只搞诊断**（动作闭环保持最小能力，不再扩展自治方向）。

借鉴落地（诊断/对话增强）：
1. **工具运行时护栏**（sxdevops/Ongrid "工具预算下沉后端"）：同参重复调用拦截（不重复执行相同
   工具+参数，返回提示）；连续空结果计数（达到阈值提示模型停止空转）。agent.rs 实现。
2. **info 级事件不升级 Incident**（Ongrid "默认只调查 warning/critical"）：escalate 门槛
   `severity=info` 事件只入库不升级。
3. **页面上下文注入**（sxdevops 页面 Copilot 思路）：前端携带当前页面/参数（agent 页、浮窗均携带
   当前路由），后端注入 system prompt「用户当前页面上下文」，模型感知提问场景。
4. 真实 LLM 回归：同前（8 工具调用、无 400、报告完整）。

明确不学（自动化方向，搁置）：Action Handler 预检卡片、Action Router 注册表、执行后验证的自动化
部分、审批流升级、复盘自动沉淀——保留两阶段手动确认动作作为诊断闭环的收尾动作，不扩展自治。

保留参考（诊断侧未来可做，记入 design）：证据包预算（查询条数/窗口/TopK 约束）、双阶段应答的
"代码兜底草稿"（确定性模板答案回退）、Skill 回答模板三段式（结论-依据-建议）。

## 对话体验优化（v3.3，2026-09-12）

1. **回答强制三段式模板**（结论/依据/建议，prompt 强化）——真实 LLM 实测：结论一针见血、依据 5 条
   编号证据全部引用真实数字（磁盘 100%、p95 38s、慢查询扫描 8.6 亿行/1.23TB、缓存盘 9.1TB 打满、
   容量 ETA 无意义），并给出"瓶颈在磁盘而非计算资源"的推断
2. **空态预设问题卡**（Flink 同款交互）：4 个预设问题点击即提问
3. **智能跟随滚动**：距底部 <120px 才自动滚底（用户上翻历史不被打断）
4. **错误重试**：失败气泡「重试」按钮（移除失败回合、重发原问题）
5. 会话标题截断 30 字符（长问题不撑爆会话列表）

真实 LLM 回归：3 工具取证 → 三段式报告完整（库内持久化）。前端构建全绿。

## 需求分析：对话内授权执行（下一轮）

场景：AI 发现参数不合理 → 对话中发起动作申请（不执行）→ 前端确认卡片 → 用户确认 → MySQLClient
通道执行 → 结果回填对话。现状：执行通道（actions.rs update_variable/kill_query 走 MySQLClient）
与两阶段确认机制均已就绪，**缺对话内申请/确认环节**（模型无受控写工具、前端无确认卡片渲染）。
实现参照 Flink PENDING_ACTION：新增受控写工具（提交 pending 动作申请，LLM 不得绕过）+ SSE
确认事件 + 前端卡片。全部地基已有，为下一轮主任务。

## 对话内授权执行（v3.4，2026-09-12）——执行授权完整落地 ⭐

场景闭环：AI 诊断发现参数不合理 → propose_action 申请（不执行）→ SSE 确认卡 → 用户确认 → MySQLClient 通道执行 → 结果回写会话。高危动作全量审计。

- 表 `agent_chat_actions`（迁移 0011 三方言）：session/user/kind/title/params/**reason(LLM 申请理由)**/status/uuid/
  created_by/expires/confirmed_by/executed/result——全链审计
- 受控工具 `propose_action`（第 8 工具）：LLM 只能提交申请不能执行；参数白名单校验（validate_params 复用）；
  返回 `ACTION_PENDING:{json}` 纯前缀（实测修掉尾随文本致 SSE 解析失败）
- SSE 新事件 `action_request`（确认卡数据：id/kind/title/params/reason/expires）
- API：confirm（单次执行原子翻转 + 归属校验：session owner from action row，不信客户端）/ cancel / list（审计回放）；
  权限 `api:agent:chat-actions:{list,confirm,cancel}`（迁移 0012，资源映射与播种对齐——实测修掉 resource 名不一致）
- 执行后结果写回会话消息（✅/❌ 动作「...」执行结果：...），审计链可见
- 真实集群端到端验证：AI 自主申请 5 条（enable_spill/query_timeout/enable_profile/parallel_fragment_exec_instance_num…，
  每条 reason 都是证据型理由）；确认后**真实执行成功** `SET GLOBAL enable_profile=true`、
  `SET GLOBAL parallel_fragment_exec_instance_num=16`；重复确认拒绝；审计回放 API 完整
- 前端：确认卡渲染（类型/理由/TTL/确认并执行/拒绝按钮），确认后刷新会话消息
- 361 测试全绿；clippy 零警告

亮点：模型曾申请 `enable_profile=true` 并准确指出"enable_profile=false 导致此前慢查询 Profile 拉取失败"——
真实因果推理 + 数据支撑的动作申请。

## 前端体检修复（v3.5，2026-09-14）——OCR 读图定位

用户 Console 截图经 OCR 破解，三处根因：
1. **transcript-loading 块从未落盘**（python replace 静默失败教训 +N）——切换会话时消息区空白
   且无 loading 提示。修复：真正插入模板（吸顶样式 + 空态隐藏）。
2. **ClusterSelectorComponent NG0100**（-1→0，nb-select selectedIndex）：activeCluster$ 异步赋值
   触发。修复：ChangeDetectorRef.detectChanges()（列表与 activeCluster 两处）。
3. **LoginComponent NG0303**（ngIf 未绑定 ×5）：缺 CommonModule（既有 bug）——修复后登录页
   条件渲染恢复正常。

教训记录：①python 批量改前端文件必须 assert 替换成功；②用户截图 OCR（tesseract chi_sim+eng + 2x 放大）
是定位前端问题的有效手段。

## GPT 对齐补完（v3.6，2026-09-14）——停止/多行输入/点赞/浮窗确认卡

1. **停止生成**：`AgentChatService` 持有回合订阅并新增 `stop()`（断开 SSE，后端在轮次边界
   取消，不烧 token）；发送位切换为停止按钮（hover 变红）；已到文本保留 + “已手动停止”标记。
2. **多行自适应输入**：input→textarea（主面板 160px/浮窗 120px 上限），Enter 发送、
   Shift+Enter 换行，中文输入法组词中不触发（Flink onEnter 同款守卫）。
3. **点赞/点踩**：新表 `agent_message_feedback`（迁移 0013 三方言）+ 权限
   `api:agent:messages:feedback`（迁移 0014）；owner 越权校验（channel+owner 双检）；
   再点取消；转录回显状态。API 实测 up/down/clear/非法值/不存在全对。
4. **浮窗动作确认卡**：浮窗处理 `action_request`，确认/拒绝 + 转录刷新；附带点赞/复制/停止/多行。
5. 顺手修浮窗 `.float-spin` 用了不存在的 `nb-rotate`（主面板同款 latent bug，早前已修）。
   浏览器实测：textarea/停止/停止标记/点赞激活全绿；clippy 零警告。

## 输入与滚动体验（v3.6 补，2026-09-14）

- 输入框加大（rows=2）；回到底部悬浮按钮（>300px 才出现，到底自动消失）。
- 发送瞬间到底：send 先同步 CD 再强制滚动；根因是此前只在 rAF 里量高度，
  markdown 异步展开后高度过期 + `scroll-behavior:smooth` 让跳转变漫游（已删）。
- 教训：滚动容器改用 host 内 querySelector，不依赖 `@ViewChild`（本次 headless
  实测其解析时有时无，行为不可测）。

## GPT 审查补完（v3.6 补，2026-09-14）

- 重新回答：每条助手回答下“换一种思路再答一次”（以前文提问重发一轮，历史保留可对照）。
- 会话重命名：PATCH /api/agent/sessions/:id + `sessions:rename` 权限（迁移 0015 三方言），行内编辑。
- 会话搜索：纯前端过滤 + 无匹配空态。
- 导出 Markdown：标题 + 用户/助手 + 取证工具清单，纯前端下载。
- 回答列宽 46rem（超宽屏可读行宽）；草稿按会话保留；Ctrl/Cmd+Enter 发送。
- 加固：`filteredSessions` 用 `String(...)` 防御（合成事件曾把 ngModel 污染成 Event 对象，
  真实点击/输入无此问题）。

## 独立 Incident 工作台下线（2026-09-14，用户决策）

- 删除菜单、路由、旧独立页面和配套服务。
- 后端 Incident/事件闭环能力（agent_runtime、表、通知）保留 dormant，不动；git 可恢复。
- 构建全绿，零残留引用。

## 节点列表"卡几十秒"根因（2026-09-14，第一性原理定位）

- 结论：节点 API 本身无辜（冷/热/代理/直连 40+ 次全部 2-4ms，无 N+1）。
  真凶是 **ng serve 增量重编一次 43s**，编译期页面 chunk 请求排队；
  当天 40+ 次重编，用户每次试用都撞上编译窗口（会话列表巨卡同一原因）。
- 排查中顺手修的真缺陷：frontends 页缺 20s 超时（无限转圈）+ 两页在集群未就绪时
  打出 `clusters/0` 无效请求（pageerror 噪音）→ 改为等 activeCluster$ 就绪再加载。
- 验证：3 行、单次请求、零无效请求、零 pageerror。

## 代码高亮（v3.6 补，2026-09-14）

- prismjs + sql/bash/json/yaml/python，token 配色自写（跟随 Nebular 深浅主题，不用 prism 官方单主题）。
- 教训：`[clipboard]` 需要 clipboard.js 全局库，缺失会抛异常并连带跳过高亮
  （renderClipboard 在 highlight 之前）——补 clipboard.min.js 后两者同时生效。
  实测：2 围栏 → 2 复制工具栏 + 42 token，颜色命中主题盘。

## 深度诊断优化（v3.7，2026-09-14）——提示词/工具/输入输出契约

**工具输入输出**：
- 新工具 `query_explain`（第 9 个）：query_id/sql 二选一；query_id 自动去审计日志取全文
  （新增 `AuditLogService::get_query_sql`，字符白名单防注入）；只读守卫（SELECT/WITH 开头、
  单条、无分号）；同一会话 USE 原库后 EXPLAIN（修掉 Unknown database 类失败）。
- 修三处误导描述：运行中查询无 Profile（id 只用于观察/kill）；profile 仅支持已完成查询；
  慢查询 SQL 只有 200 字预览。
- Profile 拉取失败返回可操作指引（查 enable_profile / 转 EXPLAIN / 基于扫描量结论）。

**护栏**：新增 `error_strikes`（连续 3 次工具失败注入劝停，与 empty_strikes 同构）——
实测 EXPLAIN 连撞时触发，模型收敛出诚实结论，不再烧光轮次。

**提示词**：依据段多指标必须用表；证据不足列缺口+取证计划；先 metrics 定方向；
同一工具失败 2 次换路禁穷举；propose_action 确认流替代旧"纯只读"声明。

**技能**：技能 1 纠正 profile 适用边界；新增技能 6 磁盘水位应急、技能 7 FE 异常。

**验证**：磁盘问题三段式+表格（93.67%/25.7TB 剩余/DataCache 100%）；慢查询 profile 规则诊断
（A004 高基数聚合 1584 万/HDFS 扫描 12.8 亿行）；EXPLAIN 在库不可见时诚实收敛。
教训：`strings` 看不见中文（C locale），二进制验证改用时间戳+行为回归；bash 工具调用
PATH 不稳定，每次显式 export。

## SQL 工作台：取消/收藏/空态指引（2026-09-14）

- **取消执行**：POST /api/clusters/queries/cancel（指纹+时间窗口定位，复用 queries:kill
  权限，零迁移）。关键实测链：SHOW PROC /current_queries 常年为空不可用；改走
  SHOW FULL PROCESSLIST；`KILL <连接号>` 有效而 `KILL QUERY <数字>` 无效；
  跨 FE KILL 报 Unknown thread id——最终用同会话 CONNECTION_ID 反查 ServerName，
  定位+KILL 同一会话发出。SLEEP(90) 7 秒 wall 终止，报错 killed manually。
- **收藏夹**：localStorage 持久化（上限 50，按使用排序），星标+下拉面板（应用/删除），
  应用时恢复 SQL 与库表上下文。
- **空态指引**：零行/失败态加“去诊断 / 重试”直达诊断入口。

## 分发/参数页优化（2026-09-14）

- 资源组创建/编辑：独立路由页 → 弹窗（集群表单同款范式），删 create/edit 路由；
  列表原地刷新；弹窗 52rem。
- 变量编辑：原生 prompt() → Nebular 弹窗（变量名/作用域徽标/当前值/新值+回车保存）；
  筛选行压单行（搜索 18rem + 类型 9rem + 右对齐按钮）；英文警告改中文。
- 根因修复：变量表 313 条在 source 里但 grid 空渲染（变更检测时序），
  数据落定后显式 detectChanges（既有惯例）。之前用户看到的一直是空表——前人埋的雷。

## 物化视图页优化（2026-09-15）

- 默认主题改暗色（无保存偏好时 `dark`；已有偏好不受影响）。
- 取详情 N×2 改单条 information_schema 查询（ROLLUP 不在该表，保留逐库 SHOW ALTER 回退；
  删 76 行死代码；名称白名单防注入）。
- 状态栏徽标柔化（实心高饱和 → 淡底彩字描边）；电源按钮 outline→ghost。
- 详情 DDL 改 markdown+Prism SQL 高亮 + 复制；删 footer 重复关闭。
- 新建/刷新/编辑弹窗按钮全部 small + ghost 取消。
- 根因修：MV/变量页数据在内存但表格不渲染（变更检测时序），落定后显式 detectChanges。
