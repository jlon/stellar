# AI 应用共享架构设计（ask 智能问数 × ops agent 智能运维助手）

> 版本: v1.0
> 日期: 2026-09-11
> 背景: `docs/ASK_DATA_DESIGN.md`（智能问数）与 `docs/agent/ops-agent-design.md`（智能运维助手）存在重叠
> ——会话持久化、消息模型、LLM 通道、SSE 传输、前端对话 UI。本文定义共享层与业务边界。

---

## 1. 重叠分析

| 能力面 | 智能问数 (ask) | 运维助手 (agent) | 重叠判定 |
| --- | --- | --- | --- |
| 会话持久化 | `ask_sessions` / `ask_messages`（设计 §10.5/10.6） | `agent_chat_sessions` / `agent_chat_messages`（已落地） | **重复建表** |
| 消息内容 | 问题 + 回复 + generated_sql/guard/explain/chart | 问题 + 回复 + 工具调用链 steps | 同一骨架，不同扩展列 |
| LLM 调用 | LLMScenario::AskData（analysis 式 JSON 协议） | ChatClient（OpenAI 兼容 tools） | 客户端可统一（ChatClient 是超集） |
| 传输 | REST + 计划 SSE | REST + 计划 SSE（fetch+ReadableStream 带 Bearer） | 同一模式 |
| 前端 | 问数页 + 会话历史 | 对话面板 + 会话列表 + 步骤链 | 对话控件可复用 |
| 权限 | 计划 `ask:*` | 已落地 `agent:*` | 各自独立（业务隔离） |

## 2. 决策：共享表 + channel 隔离，不建两套平行表

- **统一表** `ai_sessions` / `ai_messages`，`channel` 列（`'agent'` / `'ask'`）隔离业务域；
- ask 专用列（`generated_sql` / `guard_status` / `guard_reason` / `explain_text` / `chart_json` / `context_json` / `llm_session_id` / `execution_history_id`）与 agent 专用列（`steps_json`）同为宽表可空列，各取所需；
- 会话 CRUD、历史加载、删除级联、SSE 骨架、前端会话列表控件全部复用；
- **业务代码互不引用**：ops_agent 不 import ask 逻辑，ask 落地时不 import ops_agent 逻辑，只依赖 `services::ai`。

## 3. 共享层 `services/ai/`

| 组件 | 职责 | 现状 |
| --- | --- | --- |
| `session.rs` AiSessionStore | ai_sessions/ai_messages 全部读写（channel 参数），`to_chat_messages` 静态转换 | ✅ 已落地（`services/ai/session.rs`，自 ops_agent/session.rs 泛化） |
| `types.rs` | ChatMessage / ToolCallDelta / ChatCompletion / AgentStep（共享类型） | ✅ 已落地，业务代码零反向依赖 |
| `llm.rs` ChatClient | OpenAI 兼容 chat + tools（复用 `llm_providers` active provider） | ✅ 已落地（`services/ai/llm.rs`，自 ops_agent/llm.rs 提升；ask 落地时直接复用，analysis 式 JSON 协议亦可用或继续 LLMScenario） |
| `stream.rs` SSE 骨架 | 会话流推送：step/answer/done + 30s heartbeat（参照 Flink AiChatStreamHandler） | ☐ 第二阶段随 SSE 落地 |

## 4. 业务边界

```
services/ai/           ← 共享：会话/消息/未来的 ChatClient 与 SSE 骨架
   ├── session.rs
   └── (llm.rs / stream.rs 计划)

services/ops_agent/    ← agent 业务：AgentTool 族 + 诊断 playbook + agent loop
services/ask_data/     ← ask 业务（未落地）：元数据同步 + SQL 生成 + SqlGuard + 执行
```

- 表级边界：`ai_messages.steps_json` 只由 agent 写；guard/explain/chart 列只由 ask 写；
- 权限边界：`agent:*` 与 `ask:*` 各自根资源（`permission_extractor.rs` 分别注册）；
- 菜单边界：`menu:agent` 与未来 `menu:ask` 并列，互不嵌套。

## 5. 表结构（已随迁移 20260911000000 落地）

### ai_sessions

| 字段 | 说明 |
| --- | --- |
| id | 主键 |
| channel | `'agent'` / `'ask'`（未来频道） |
| organization_id / user_id | 归属（可空，agent 暂不填，ask 必填） |
| cluster_id | 集群隔离（FK → clusters ON DELETE CASCADE） |
| title | 首条消息摘要 |
| catalog / database_name | ask 默认上下文（agent 忽略） |
| created_at / last_active_at | 时间 |

### ai_messages

| 字段 | 说明 |
| --- | --- |
| id / session_id (FK CASCADE) | 主键 + 会话 |
| role | `user` / `assistant` |
| content | 问题或回复 |
| steps_json | agent：工具调用链审计 |
| generated_sql / guard_status / guard_reason / explain_text / chart_json / context_json / llm_session_id / execution_history_id | ask 扩展列（可空） |

索引：`(channel, cluster_id, last_active_at DESC)`、`(session_id, id)`。

## 6. 演进注意

- 迁移 20260911000000（sqlite/mysql/postgres）本地开发期直接改造为 `ai_sessions` / `ai_messages` 宽表（未提交任何发布版），无需兼容迁移；
- ask 落地（`services/ask_data/`）时：复用 `AiSessionStore`（channel='ask'）、权限迁移新建 `ask:*` 资源、前端复用会话列表/消息流控件；
- SSE 落地时 SSE 骨架入 `services/ai/stream.rs`，ask 与 agent 共用同一传输模式。