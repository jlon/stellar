# Stellar 智能问数功能设计文档

> 版本：v1.0  
> 日期：2026-09-11  
> 状态：设计待评审  
> 主参考：SQLBot；长期语义层参考 WrenAI；Text-to-SQL 流程参考 Vanna；Chat2DB 仅参考 SQL 工作台体验。

---

## 1. 背景

Stellar 目前是 StarRocks / Apache Doris 的 OLAP 集群管理平台，已有能力包括：

- 集群、FE/BE/CN 节点、会话、实时查询、变量、物化视图管理。
- 审计日志、慢查询、Query Profile 分析、SQL 性能诊断。
- 多租户组织、JWT 认证、Casbin 权限、数据权限申请审批。
- OpenAI 兼容的 LLM Provider 配置与缓存能力。

缺口是：用户仍需要手写 SQL 才能取数。智能问数要让用户用自然语言提问，系统自动生成安全 SQL，执行查询，并返回表格、图表和中文解释。

一句话目标：**把 Stellar 从“OLAP 管理平台”扩展为“可治理的 OLAP 智能分析入口”。**

---

## 2. 参考项目取舍

| 项目 | 适合参考 | 不适合直接采用的原因 | 结论 |
| --- | --- | --- | --- |
| SQLBot | 智能问数产品形态、术语库、SQL 示例校准、嵌入方式、ChatBI 流程 | 许可证类似 GPLv3 且有额外限制，不能直接复制代码；技术栈是 Python/Vue/PostgreSQL | **主参考产品设计，不拷代码** |
| WrenAI | 语义层、指标口径、上下文治理、可信 Text-to-SQL | 更偏 agent-driven GenBI，首期集成成本高 | 二期参考语义层 |
| Vanna | Text-to-SQL 核心链路、安全、用户感知、Web 组件思路 | 偏 Python SDK，不是完整平台 | 参考生成链路 |
| Chat2DB | SQL 编辑器、数据库客户端体验、SQL 解释/优化 | 偏单用户本地数据库客户端；Stellar 已有大量管理能力 | 只参考交互细节 |

设计原则：**SQLBot 形，WrenAI 魂，Vanna 路，Stellar 自己落地。**

---

## 3. 目标与非目标

### 3.1 目标

MVP 必须做到：

1. 用户选择 StarRocks / Doris 集群、Catalog、Database 后，用中文提问。
2. 系统检索表结构、字段注释、表关系、业务术语、历史 SQL 示例。
3. LLM 生成只读 SQL，并给出使用的表、字段、假设和置信度。
4. 后端做 SQL 安全校验、`EXPLAIN` 预检、默认 `LIMIT`、超时控制。
5. 用户确认后执行 SQL，返回表格、图表建议和中文解释。
6. 保存问答历史、生成 SQL、执行记录和用户反馈。
7. 复用 Stellar 现有组织隔离、权限、集群连接、SQL 执行历史和 LLM 配置。

### 3.2 非目标

首期不做：

- 不复制 SQLBot / WrenAI / Vanna 代码。
- 不新建独立问数服务进程。
- 不重复实现数据源管理；数据源就是 Stellar 已注册的集群。
- 不自动执行 DDL / DML / 管理语句。
- 不做完整语义层建模、指标市场、Dashboard 发布平台。
- 不接 Excel / CSV / 文档问答。
- 不让 LLM 直接决定权限。

---

## 4. 总体架构

```
┌──────────────────────── 前端 Angular / Nebular ────────────────────────┐
│ 智能问数页 │ 问答历史 │ 术语库 │ SQL 示例 │ 结果表格 │ 图表预览 │ 反馈 │
└───────────────────────────────┬───────────────────────────────────────┘
                                │ REST，后续可加 SSE
┌──────────────────────── 后端 ask_data 模块 ─────────────────────────────┐
│ AskService                                                           │
│  ├─ MetadataSyncService    同步库/表/字段/DDL/物化视图/统计信息         │
│  ├─ ContextRetriever       检索相关 schema、术语、SQL 示例              │
│  ├─ SqlGenerationService   调用 LLM 生成结构化 SQL                     │
│  ├─ SqlGuard               单语句、只读、LIMIT、黑名单、风险分级         │
│  ├─ ExplainService         EXPLAIN / 语法预检 / 一次修复                 │
│  ├─ AskExecutionService    复用现有 SQL 执行链路                         │
│  └─ FeedbackService        反馈、命中率、样例沉淀                         │
└───────────────────────────────┬───────────────────────────────────────┘
                                │ 复用
┌──────────────────────── Stellar 现有能力 ───────────────────────────────┐
│ ClusterAdapter │ MySQLClient │ LLMService │ ProfileAnalyzer │ Casbin │ 审计 │
└───────────────────────────────┬───────────────────────────────────────┘
                                │ MySQL 协议 / HTTP API
                         StarRocks / Apache Doris
```

### 4.1 复用现有代码

| 现有能力 | 文件 | 复用方式 |
| --- | --- | --- |
| StarRocks / Doris 统一适配 | `backend/src/services/cluster_adapter/mod.rs` | 问数按 `cluster_type` 生成方言提示词和切换 Catalog 语句 |
| 连接与会话 | `backend/src/services/mysql_client.rs` | `SET CATALOG` / `SWITCH` + `USE database` + 执行 SQL |
| Catalog / Database / Table 查询 | `backend/src/handlers/query.rs` | 元数据同步复用或下沉为 service |
| SQL 执行历史 | `query_execution_history_service` | 问数执行后复用现有历史记录 |
| SQL 诊断 | `backend/src/handlers/sql_diag.rs` | 复用 EXPLAIN、schema、vars 的取证思路 |
| LLM Provider | `backend/src/services/llm/` | 新增 `LLMScenario::AskData`，复用 provider、缓存、session 记录 |
| Profile 分析 | `backend/src/services/profile_analyzer/` | 二期：问数 SQL 执行后自动诊断慢查询 |
| 多租户与权限 | middleware + Casbin | 所有 ask API 必须走组织隔离和权限码 |

---

## 5. 核心流程

### 5.1 元数据同步

触发方式：

- 用户手动点击“同步元数据”。
- 集群新增或修改后提示同步。
- 后续可加低频后台同步，例如每天一次。

同步内容：

1. Catalog 列表。
2. Database 列表。
3. 表 / 视图 / 物化视图列表。
4. `SHOW CREATE TABLE` DDL。
5. 字段名、类型、注释、是否分区键、是否分桶键。
6. 表类型：内表 / 外表。
7. 可选统计信息：行数、数据量、分区数量、Tablet 数量。

性能约束：

- 默认只同步用户可见的 Catalog / Database。
- 单次同步设置表数量上限，例如 2000 张表。
- 不默认采样业务数据，避免隐私与大查询风险。
- 大集群支持按库同步。

### 5.2 用户提问

输入：

```json
{
  "cluster_id": 1,
  "catalog": "default_catalog",
  "database": "sales",
  "question": "最近 30 天每天的订单金额趋势",
  "session_id": "可选"
}
```

流程：

1. 校验用户是否有该集群、Catalog、Database 的访问权限。
2. 从 `ask_metadata_items`、`ask_terms`、`ask_sql_examples` 检索相关上下文。
3. 拼装 Prompt，要求 LLM 只输出 JSON。
4. LLM 返回候选 SQL、图表建议、假设、置信度。
5. `SqlGuard` 做安全校验。
6. `EXPLAIN` 预检。
7. 若语法失败，最多让 LLM 修复一次。
8. 返回“待确认执行”的结果。

### 5.3 SQL 执行

首期默认采用 **生成 SQL 后用户确认执行**。理由：

- OLAP 查询可能很重。
- LLM 可能选错表或缺少过滤条件。
- Stellar 面向企业管理场景，安全优先。

管理员后续可开启“低风险自动执行”：

- SQL 是单条 `SELECT` / `WITH ... SELECT`。
- `EXPLAIN` 通过。
- 有 `LIMIT`。
- 没有全表扫描大表风险。
- 用户有执行权限。

执行后返回：

```json
{
  "columns": ["dt", "amount"],
  "rows": [["2026-09-01", "12345.67"]],
  "row_count": 30,
  "execution_time_ms": 153,
  "summary": "最近 30 天订单金额整体上升，9 月 8 日达到峰值。",
  "chart": {
    "type": "line",
    "x": "dt",
    "y": "amount"
  }
}
```

---

## 6. SQL 安全设计

智能问数的核心安全边界：**LLM 只能建议 SQL，后端决定是否允许执行。**

### 6.1 只读白名单

允许：

- `SELECT`
- `WITH ... SELECT`
- `EXPLAIN SELECT`

禁止：

- `INSERT` / `UPDATE` / `DELETE` / `MERGE`
- `CREATE` / `ALTER` / `DROP` / `TRUNCATE`
- `GRANT` / `REVOKE`
- `LOAD` / `EXPORT`
- `KILL` / `ADMIN` / `SET` / `INSTALL`
- 多语句执行

### 6.2 校验规则

`SqlGuard` 必须做：

1. 去掉注释后检测多语句；只允许一条 SQL。
2. 只允许只读语句。
3. 默认追加或包裹 `LIMIT`，例如 1000。
4. 禁止访问系统库，除非管理员显式允许。
5. 禁止访问用户无权限的库表。
6. 对 `ORDER BY` 无 `LIMIT`、大表无过滤、跨大表 Join 标记为高风险。
7. 记录原始问题、生成 SQL、校验结果、执行人。

> 实现上不要只靠一个正则。首期可以用轻量 token 扫描，后续引入 SQL parser。正则只做辅助。

### 6.3 EXPLAIN 预检

执行前运行：

```sql
EXPLAIN VERBOSE <generated_sql>
```

判断：

- 语法是否通过。
- 是否全表扫描大表。
- 是否 Broadcast 大表。
- 是否缺少分区裁剪。
- 是否包含明显高风险算子。

MVP 只做阻断和提示，不做复杂自动优化。

---

## 7. RAG / 上下文设计

首期不引入新向量数据库。原因：Stellar 已支持 SQLite / MySQL / PostgreSQL 三种元数据库，新增向量库会增加部署成本。

### 7.1 M0 检索策略

使用简单可控的关键词评分：

1. 中文分词先不做复杂依赖，按连续汉字、英文、数字切 token。
2. 表名、字段名、注释、业务术语、SQL 示例统一构建 `search_text`。
3. Rust 内存评分：
   - 问题 token 命中表名：+10
   - 命中字段名：+6
   - 命中注释：+4
   - 命中术语：+8
   - 命中历史成功 SQL：+5
4. 取 Top 10 表、Top 20 字段、Top 5 示例进入 Prompt。

这比直接把全库 schema 塞给 LLM 更省 token，也更稳定。

### 7.2 M1 检索增强

- 支持用户维护“业务术语 → 表字段/指标解释”。
- 支持把成功问答沉淀为 SQL 示例。
- 支持按数据库、标签、使用频率加权。

### 7.3 M2 语义层

参考 WrenAI，引入轻量语义层：

- 指标：GMV、订单数、活跃用户数。
- 维度：日期、区域、渠道、商品类目。
- 表关系：事实表与维表 Join 关系。
- 口径：过滤条件、时间字段、去重规则。
- 禁用字段：敏感字段、废弃字段。

---

## 8. Prompt 输出协议

LLM 必须输出纯 JSON：

```json
{
  "need_clarification": false,
  "clarification_question": null,
  "sql": "SELECT dt, SUM(amount) AS amount FROM orders WHERE dt >= CURRENT_DATE() - INTERVAL 30 DAY GROUP BY dt ORDER BY dt LIMIT 1000",
  "tables": ["orders"],
  "columns": ["dt", "amount"],
  "assumptions": ["订单金额字段为 amount", "日期字段为 dt"],
  "chart": {
    "type": "line",
    "x": "dt",
    "y": ["amount"],
    "reason": "时间字段 + 数值指标适合折线图"
  },
  "summary_template": "按天统计最近 30 天订单金额趋势。",
  "confidence": 0.82
}
```

规则：

1. 不确定表或口径时，`need_clarification=true`，不要硬猜。
2. SQL 必须匹配当前引擎方言：StarRocks 或 Doris。
3. 不允许生成写入语句。
4. 不允许编造不存在的表字段。
5. `confidence < 0.6` 时前端默认不展示“执行”主按钮，只展示“继续澄清”。

---

## 9. 图表设计

MVP 不让 LLM 直接生成复杂图表配置，只让它建议图表类型和字段。前端用确定性规则渲染：

| 数据形态 | 图表 |
| --- | --- |
| 时间列 + 数值列 | 折线图 |
| 分类列 + 数值列 | 柱状图 |
| 分类数量 <= 8 + 占比含义 | 饼图 |
| 多维度或无法判断 | 表格 |

这样能减少 Prompt 成本，也避免 LLM 生成不可用的 ECharts 配置。

---

## 10. 数据模型

新增表使用 `ask_` 前缀。所有迁移必须同步维护：

- `backend/migrations/sqlite/`
- `backend/migrations/mysql/`
- `backend/migrations/postgres/`

JSON 字段统一用 `TEXT` 存序列化 JSON，避免三方言差异。

### 10.1 `ask_settings`

每个组织 / 集群的问数配置。

| 字段 | 说明 |
| --- | --- |
| `id` | 主键 |
| `organization_id` | 组织 |
| `cluster_id` | 集群 |
| `enabled` | 是否启用 |
| `auto_execute` | 是否允许低风险自动执行 |
| `default_limit` | 默认 LIMIT，例如 1000 |
| `max_rows` | 最大返回行数 |
| `query_timeout_secs` | 查询超时 |
| `allowed_catalogs_json` | 可问数 Catalog 白名单 |
| `blocked_schemas_json` | 禁止访问库 |
| `created_at`, `updated_at` | 时间 |

### 10.2 `ask_metadata_items`

统一保存可检索元数据。

| 字段 | 说明 |
| --- | --- |
| `id` | 主键 |
| `organization_id`, `cluster_id` | 隔离键 |
| `catalog`, `database_name`, `table_name`, `column_name` | 定位 |
| `kind` | `database` / `table` / `column` / `materialized_view` |
| `data_type` | 字段类型 |
| `comment` | 注释 |
| `ddl` | 表 DDL，仅表级记录存 |
| `table_type` | `internal` / `external` |
| `stats_json` | 行数、分区、Tablet 等 |
| `search_text` | 检索文本 |
| `synced_at` | 同步时间 |

索引：`(cluster_id, catalog, database_name)`、`(cluster_id, table_name)`。

### 10.3 `ask_terms`

业务术语库。

| 字段 | 说明 |
| --- | --- |
| `id` | 主键 |
| `organization_id`, `cluster_id` | 隔离键 |
| `term` | 术语，例如 GMV |
| `definition` | 定义 |
| `expression` | 推荐表达式，例如 `SUM(pay_amount)` |
| `scope_json` | 生效库表范围 |
| `enabled` | 是否启用 |

### 10.4 `ask_sql_examples`

Few-shot SQL 示例。

| 字段 | 说明 |
| --- | --- |
| `id` | 主键 |
| `organization_id`, `cluster_id` | 隔离键 |
| `question` | 示例问题 |
| `sql` | 标准 SQL |
| `catalog`, `database_name` | 范围 |
| `tables_json` | 涉及表 |
| `enabled` | 是否启用 |
| `source` | `manual` / `feedback` / `audit` |

### 10.5 会话存储（复用共享表 `ai_sessions`，channel='ask'）

> 本设计与智能运维助手（ops_agent）共享会话基建，**不单独建表**。共享表结构与读写见 `docs/agent/ai-common-design.md`（`services/ai/session.rs` `AiSessionStore`）。

| 字段（共享表中与 ask 相关部分） | 说明 |
| --- | --- |
| `channel` | `'ask'`（与 agent 会话隔离） |
| `organization_id` / `user_id` | 隔离与归属（ask 必填，agent 可空） |
| `cluster_id` | 集群隔离 |
| `title` | 会话标题 |
| `catalog` / `database_name` | 默认上下文（仅 ask 使用） |
| `created_at` / `last_active_at` | 时间 |

### 10.6 问答消息（复用共享表 `ai_messages`）

| 字段（ask 扩展列） | 说明 |
| --- | --- |
| `role` | `user` / `assistant` |
| `content` | 用户问题或助手回答 |
| `generated_sql` | 生成 SQL |
| `guard_status` | `pending` / `passed` / `blocked` / `failed` |
| `guard_reason` | 阻断原因 |
| `explain_text` | EXPLAIN 结果，截断保存 |
| `chart_json` | 图表建议 |
| `context_json` | 命中的上下文摘要 |
| `llm_session_id` | 关联 LLM 分析记录 |
| `execution_history_id` | 关联现有 SQL 执行历史 |
| `created_at` | 时间 |

### 10.7 `ask_feedback`

用户反馈。

| 字段 | 说明 |
| --- | --- |
| `id` | 主键 |
| `message_id` | 消息 |
| `user_id` | 用户 |
| `rating` | `up` / `down` |
| `reason` | 原因 |
| `corrected_sql` | 用户修正 SQL |
| `created_at` | 时间 |

---

## 11. 后端 API

### 11.1 配置与元数据

```http
GET  /api/clusters/{cluster_id}/ask/settings
PUT  /api/clusters/{cluster_id}/ask/settings
POST /api/clusters/{cluster_id}/ask/metadata/sync
GET  /api/clusters/{cluster_id}/ask/metadata/status
```

### 11.2 术语与示例

```http
GET    /api/clusters/{cluster_id}/ask/terms
POST   /api/clusters/{cluster_id}/ask/terms
PUT    /api/clusters/{cluster_id}/ask/terms/{id}
DELETE /api/clusters/{cluster_id}/ask/terms/{id}

GET    /api/clusters/{cluster_id}/ask/examples
POST   /api/clusters/{cluster_id}/ask/examples
PUT    /api/clusters/{cluster_id}/ask/examples/{id}
DELETE /api/clusters/{cluster_id}/ask/examples/{id}
```

### 11.3 问数会话

```http
POST /api/clusters/{cluster_id}/ask/sessions
GET  /api/clusters/{cluster_id}/ask/sessions
GET  /api/clusters/{cluster_id}/ask/sessions/{session_id}
POST /api/clusters/{cluster_id}/ask/sessions/{session_id}/messages
POST /api/clusters/{cluster_id}/ask/messages/{message_id}/execute
POST /api/clusters/{cluster_id}/ask/messages/{message_id}/feedback
```

### 11.4 权限码

新增权限建议：

| 权限码 | 说明 |
| --- | --- |
| `ask:view` | 查看智能问数页与历史 |
| `ask:query` | 发起问数 |
| `ask:execute` | 执行生成 SQL |
| `ask:manage_terms` | 管理术语库 |
| `ask:manage_examples` | 管理 SQL 示例 |
| `ask:manage_settings` | 管理问数配置 |

---

## 12. 前端设计

### 12.1 页面入口

推荐放在：

```text
/pages/starrocks/queries/ask-data
```

菜单名称：**智能问数**。

理由：它属于查询管理能力，不是系统配置，也不是集群拓扑管理。

### 12.2 页面布局

```
┌────────────────────────────────────────────────────────────┐
│ 集群 / Catalog / Database 选择       元数据状态：已同步     │
├───────────────┬────────────────────────────────────────────┤
│ 历史会话       │ 问答区                                      │
│ 推荐问题       │  用户：最近30天订单金额趋势                  │
│ 术语提示       │  AI：生成 SQL + 表格 + 图表 + 解释            │
│                │  [查看 SQL] [执行] [诊断] [👍] [👎]          │
├───────────────┴────────────────────────────────────────────┤
│ 结果表格 / ECharts 图表 / EXPLAIN 风险提示                   │
└────────────────────────────────────────────────────────────┘
```

### 12.3 管理入口

系统管理或页面右上角进入：

- 问数配置。
- 术语库。
- SQL 示例。
- 元数据同步状态。

---

## 13. 与 Stellar 现有特色的结合

这是 Stellar 能区别于通用 ChatBI 的地方：

1. **Query Profile 联动**  
   问数 SQL 执行慢时，自动拉取 Profile 并调用现有规则引擎诊断。

2. **物化视图推荐**  
   高频问法或慢查询命中相同聚合模式时，提示创建物化视图。

3. **集群负载保护**  
   查询前读取当前运行查询数、BE 存活、磁盘、Compaction Score。集群压力大时提示延迟执行。

4. **StarRocks / Doris 方言优化**  
   Prompt 和 SQL 校验按 `ClusterType` 区分，避免把 StarRocks 独有语法用于 Doris。

5. **审计日志反哺**  
   从历史成功 SQL 和常见慢 SQL 中推荐 Few-shot 示例。

---

## 14. 分期计划

### M0：最小可用版本

目标：能安全问数。

范围：

- 新增 `ask_data` 后端模块。
- 元数据同步：Catalog / Database / Table / Column / DDL。
- 关键词检索上下文。
- LLM 生成 SQL。
- SQL Guard：只读、多语句、LIMIT、黑名单。
- EXPLAIN 预检。
- 用户确认执行。
- 表格结果、SQL 展示、问答历史。

验收：

- 对 StarRocks 和 Doris 各跑 10 个典型问题。
- 非 SELECT 全部被拦截。
- 生成 SQL 不存在表字段时能提示澄清或修复。
- 默认不会返回超过配置上限的行数。

### M1：可运营版本

目标：越问越准。

范围：

- 术语库管理。
- SQL 示例管理。
- 用户反馈。
- 一次 SQL 修复。
- 图表预览。
- 常见问题推荐。
- 从审计日志生成候选示例，人工确认后启用。

验收：

- 同一类问题经示例校准后，生成 SQL 稳定命中正确表。
- 错误 SQL 可通过一次修复成功率提升。
- 图表类型选择准确率可人工验收。

### M2：可信语义层版本

目标：企业级可信问数。

范围：

- 指标口径管理。
- 表关系管理。
- 敏感字段禁用。
- 行列权限联动。
- Profile 自动诊断。
- 物化视图推荐。
- 大查询风险评分。

验收：

- “收入 / GMV / 订单数”等指标口径统一。
- 无权限字段不会进入 Prompt。
- 慢问数 SQL 能自动产出 Profile 诊断建议。

---

## 15. 风险与对策

| 风险 | 对策 |
| --- | --- |
| LLM 编造字段 | Prompt 只给检索命中的 schema；后端执行前 EXPLAIN；失败最多修复一次 |
| 误执行写语句 | 后端 SqlGuard 白名单；只读账号；禁多语句 |
| 大查询拖垮集群 | 默认 LIMIT；EXPLAIN 风险判断；超时；用户确认执行 |
| 多租户越权 | 元数据同步、检索、执行全部带 `organization_id` 和 Casbin 校验 |
| Prompt 泄露敏感字段 | 敏感字段不进入 `ask_metadata_items.search_text`，也不进入 Prompt |
| SQLBot 许可证风险 | 只参考设计，不复制代码、Prompt、前端资源 |
| 结果不可信 | 展示 SQL、表字段、假设、置信度；低置信度要求澄清 |
| 部署复杂度上升 | 首期不引入向量数据库，不新增独立服务 |

---

## 16. 设计自检

- KISS：首期只做“检索上下文 → 生成 SQL → 校验 → 用户确认执行”，不做完整 BI 平台。
- YAGNI：不引入向量库、不做指标市场、不自动发布 Dashboard。
- DRY：复用现有 LLM、ClusterAdapter、MySQLClient、QueryExecutionHistory、权限体系。
- 安全：LLM 没有执行权，所有 SQL 必须经过后端校验和用户权限检查。
- 性能：不默认采样业务数据；元数据可按库同步；查询默认 LIMIT 和超时。
- 可演进：M0 可用，M1 可运营，M2 进入语义层和 Profile 联动。

---

## 17. 推荐落地结论

Stellar 应该优先参考 SQLBot 的产品形态，但不要引入 SQLBot 代码。最小实现应作为 Stellar 后端内的 `ask_data` 模块，直接复用现有集群连接、权限、LLM 配置和 SQL 执行链路。

首期只解决一个问题：**让用户安全地用中文问 StarRocks / Doris 数据，并能看到生成 SQL、表格结果和基础图表。**

等问数链路跑通后，再引入 WrenAI 式语义层，把“能问”升级为“可信地问”。
