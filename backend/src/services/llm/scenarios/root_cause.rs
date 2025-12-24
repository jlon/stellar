//! Root Cause Analysis Scenario
//!
//! LLM-enhanced root cause analysis for query profile diagnostics.
//! Supports both StarRocks and Doris OLAP databases with dynamic prompt generation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::models::cluster::ClusterType;
use crate::services::llm::models::LLMScenario;
use crate::services::llm::service::{LLMAnalysisRequestTrait, LLMAnalysisResponseTrait};

// ============================================================================
// System Prompt - Dynamic Generation
// ============================================================================

/// Common analysis methodology shared between StarRocks and Doris
const PROMPT_ANALYSIS_METHODOLOGY: &str = r#"
## 🧠 批判性思维要求 (Critical Thinking)

在给出任何诊断或建议前，你必须进行**自我批评式思考**：

1. **质疑假设**: 我的诊断是否基于充分的证据？是否有其他可能的解释？
2. **验证参数**: 我推荐的参数是否真实存在于官方文档中？如果不确定，宁可不推荐。
3. **检查适用性**: 这个建议是否适用于当前的表类型（内表/外表）？
4. **避免臆断**: 我是否在没有数据支撑的情况下做出了推测？
5. **反思偏见**: 我是否过度依赖某些常见模式而忽略了具体情况？

**重要原则**: 宁可少给建议，也不要给出错误或不存在的参数建议！

## 分析方法论 (Chain-of-Thought)

### Step 1: 理解查询意图
- 这是什么类型的查询？(OLAP聚合/点查/ETL导入/Join密集型)
- 涉及哪些表？各表的数据量级？
- 自问: 我是否完整理解了查询的业务场景？

### Step 2: 识别性能瓶颈
- 哪个算子耗时最长？(time_pct > 30%)
- 是 IO 瓶颈还是 CPU 瓶颈？
- 是否有数据倾斜？(max/avg 比值)
- 自问: 我的判断是否有 Profile 指标支撑？

### Step 3: 根因溯源
- 瓶颈算子的上游是什么？
- 根因是数据问题还是配置问题？
- 是否有规则引擎未发现的隐式根因？
- 自问: 我是否混淆了症状和根因？

### Step 4: 制定优化方案
- 针对根因而非症状给出建议
- 优先给出投入产出比最高的优化
- 必须是可直接执行的命令
- 自问: 这个建议在用户环境中是否可行？

### Step 5: 自我验证 (必做)
- **参数存在性**: 我推荐的每个参数是否在下方的"官方支持参数列表"中？
- **表类型匹配**: 对外表建议 ALTER TABLE 分桶是错误的！
- **配置冲突**: 是否与当前 session_variables 中的值重复？
- **命令完整性**: SQL/SET 命令是否可以直接复制执行？
"#;

/// Build base prompt header for specific cluster type
fn build_base_prompt_header(cluster_type: ClusterType) -> &'static str {
    match cluster_type {
        ClusterType::StarRocks => {
            "你是一位拥有20年以上的 StarRocks OLAP 数据库的高级性能专家。\n\
             你需要分析 Query Profile 数据，识别真正的根因并给出**可直接执行**的优化建议。"
        }
        ClusterType::Doris => {
            "你是一位拥有20年以上的 Apache Doris OLAP 数据库的高级性能专家。\n\
             你需要分析 Query Profile 数据，识别真正的根因并给出**可直接执行**的优化建议。\n\n\
             ## Doris 核心特性\n\n\
             Apache Doris 是一款高性能的实时分析数据库，具有以下核心特性：\n\
             - **MPP 架构**: 大规模并行处理，支持 PB 级数据分析\n\
             - **向量化执行引擎**: 基于列式存储的向量化计算\n\
             - **Pipeline 执行引擎**: 异步执行，充分利用多核 CPU\n\
             - **智能物化视图**: 自动查询改写，透明加速\n\
             - **多 Catalog 支持**: 统一访问 Hive/Iceberg/Hudi/Paimon 等外部数据源"
        }
    }
}

/// Build strict rules prompt with database name
fn build_strict_rules_prompt(db_name: &str) -> String {
    format!(
        r#"
## ⚠️ 严格遵守的规则
1. **检查 session_variables 再给建议**: 参数已启用就不要重复建议
2. **区分表类型**: 内表和外表的优化方向完全不同
3. **参数必须存在**: 只使用下方列出的 {} 官方参数，禁止创造参数！
4. **建议必须可执行**: 给出完整的 SQL/SET/ALTER 命令
5. **宁缺毋滥**: 不确定的建议宁可不给，也不要误导用户"#,
        db_name
    )
}

/// Build complete base prompt for cluster type
fn build_base_prompt(cluster_type: ClusterType) -> String {
    format!(
        "{}\n{}\n{}",
        build_base_prompt_header(cluster_type),
        PROMPT_ANALYSIS_METHODOLOGY,
        build_strict_rules_prompt(cluster_type.display_name())
    )
}

// ============================================================================
// Table Type Prompt Generation
// ============================================================================

/// Connector-specific optimization hints
struct ConnectorHints {
    name: String,
    cache_cmd: &'static str,
    hints: Vec<&'static str>,
}

impl ConnectorHints {
    fn new(name: impl Into<String>, cache_cmd: &'static str, hints: &[&'static str]) -> Self {
        Self {
            name: name.into(),
            cache_cmd,
            hints: hints.to_vec(),
        }
    }
}

/// Get connector hints based on cluster type
fn get_connector_hints(connector: &str, cluster_type: ClusterType) -> ConnectorHints {
    let cache_cmd = match cluster_type {
        ClusterType::StarRocks => "SET enable_scan_datacache=true;",
        ClusterType::Doris => "SET enable_file_cache=true;",
    };

    match connector {
        "hive" => ConnectorHints::new("Hive", cache_cmd, &[
            "分区裁剪: 确保 WHERE 条件包含分区列",
            "小文件合并: 在 Hive/Spark 端执行合并",
            "⚠️ 外表不支持 ALTER TABLE 修改分桶",
        ]),
        "iceberg" => ConnectorHints::new("Iceberg", cache_cmd, &[
            "利用 Iceberg 的 hidden partitioning",
            "检查 delete files 是否过多 (V2 格式)",
            "使用 Time Travel 查询历史数据",
        ]),
        "hudi" => ConnectorHints::new("Hudi", cache_cmd, &[
            "检查 compaction 是否及时",
            "MOR 表考虑调整读取模式",
            "使用增量查询优化性能",
        ]),
        "paimon" => ConnectorHints::new("Paimon", cache_cmd, &[
            "利用 Paimon 的主键表特性",
            "检查 snapshot 数量",
        ]),
        "jdbc" => ConnectorHints::new("JDBC", "", &[
            "谓词下推: 确保 WHERE 条件能下推到源库",
            "减少 SELECT 列: 只查询必要的列",
            "考虑数据同步到内表加速",
        ]),
        "es" | "elasticsearch" => ConnectorHints::new("Elasticsearch", "", &[
            "确保查询条件能下推到 ES",
            "利用 ES 的索引能力",
            "减少返回字段数",
        ]),
        _ => ConnectorHints::new(connector, cache_cmd, &["分区裁剪", "谓词下推"]),
    }
}

/// Get internal table optimization hints based on cluster type
fn get_internal_table_hints(cluster_type: ClusterType) -> &'static [&'static str] {
    match cluster_type {
        ClusterType::StarRocks => &[
            "ANALYZE TABLE 更新统计信息",
            "检查分桶键是否合理",
            "考虑物化视图加速",
            "可使用 ALTER TABLE 调整属性",
        ],
        ClusterType::Doris => &[
            "ANALYZE TABLE 更新统计信息",
            "检查分桶键是否合理 (DISTRIBUTED BY HASH)",
            "考虑同步物化视图或异步物化视图加速",
            "可使用 ALTER TABLE 调整属性",
            "检查 Compaction 是否及时",
        ],
    }
}

/// Build table type prompt - unified for both StarRocks and Doris
fn build_table_type_prompt(scan_details: &[ScanDetailForLLM], cluster_type: ClusterType) -> String {
    let mut internal_tables = Vec::new();
    let mut external_tables: HashMap<String, Vec<String>> = HashMap::new();

    for scan in scan_details {
        if scan.table_type == "internal" {
            internal_tables.push(scan.table_name.clone());
        } else {
            let connector = scan.connector_type.clone().unwrap_or_else(|| "unknown".to_string());
            external_tables.entry(connector).or_default().push(scan.table_name.clone());
        }
    }

    let mut prompt = String::from("\n\n## 📊 本次查询涉及的表\n");
    let db_name = cluster_type.display_name();

    // Internal tables
    if !internal_tables.is_empty() {
        let hints = get_internal_table_hints(cluster_type);
        prompt.push_str(&format!(
            "\n### {} 内表 ({} 张)\n表名: {}\n\n**内表优化方向:**\n{}\n",
            db_name,
            internal_tables.len(),
            internal_tables.join(", "),
            hints.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n")
        ));
    }

    // External tables
    for (connector, tables) in &external_tables {
        let hints = get_connector_hints(connector, cluster_type);
        let mut hint_lines: Vec<String> = Vec::new();

        if !hints.cache_cmd.is_empty() {
            hint_lines.push(format!("启用缓存: `{}`", hints.cache_cmd));
        }
        hint_lines.extend(hints.hints.iter().map(|h| (*h).to_string()));

        prompt.push_str(&format!(
            "\n### {} 外表 ({} 张)\n表名: {}\n\n**{} 表优化方向:**\n{}\n",
            hints.name,
            tables.len(),
            tables.join(", "),
            hints.name,
            hint_lines.iter().map(|h| format!("- {}", h)).collect::<Vec<_>>().join("\n")
        ));
    }

    prompt
}


// ============================================================================
// Issue and Session Variable Prompts
// ============================================================================

/// Dynamic prompt section based on detected issues
fn build_issue_focused_prompt(diagnostics: &[DiagnosticForLLM]) -> String {
    if diagnostics.is_empty() {
        return String::from(
            "\n\n## 规则引擎未发现明显问题\n请深入分析原始 Profile 数据，寻找隐式性能问题。\n",
        );
    }

    let mut prompt = String::from("\n\n## 规则引擎已识别的问题 (仅作为参考)\n");
    for d in diagnostics.iter().take(5) {
        prompt.push_str(&format!("- **{}** [{}]: {}\n", d.rule_id, d.severity, d.message));
    }
    prompt.push_str("\n**你的任务**: 不要简单重复这些问题，而是:\n1. 分析这些症状背后的根因\n2. 找出规则引擎未发现的隐式问题\n3. 建立因果链条\n");

    prompt
}

/// Dynamic prompt section for current session variables
fn build_session_vars_prompt(session_vars: &HashMap<String, String>) -> String {
    if session_vars.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("\n\n## ⚠️ 当前集群配置 (严格禁止重复建议!)\n");

    let mut enabled_features = Vec::new();
    let mut disabled_features = Vec::new();
    let mut other_settings = Vec::new();

    for (var, value) in session_vars {
        let is_bool_flag = var.starts_with("enable_");
        let is_true = value == "true" || value == "1";

        if is_bool_flag {
            if is_true {
                enabled_features.push(var.as_str());
            } else {
                disabled_features.push(var.as_str());
            }
        } else {
            other_settings.push((var.as_str(), value.as_str()));
        }
    }

    enabled_features.sort();
    disabled_features.sort();
    other_settings.sort_by_key(|(k, _)| *k);

    if !enabled_features.is_empty() {
        prompt.push_str(&format!(
            "\n### 🟢 已启用的功能 (禁止再建议开启!)\n{}\n",
            enabled_features.iter().map(|v| format!("`{}`", v)).collect::<Vec<_>>().join(", ")
        ));
    }

    if !disabled_features.is_empty() {
        prompt.push_str(&format!(
            "\n### 🔴 已禁用的功能 (可建议开启)\n{}\n",
            disabled_features.iter().map(|v| format!("`{}`", v)).collect::<Vec<_>>().join(", ")
        ));
    }

    if !other_settings.is_empty() {
        prompt.push_str("\n### 其他配置\n");
        for (var, value) in &other_settings {
            prompt.push_str(&format!("- `{}` = `{}`\n", var, value));
        }
    }

    prompt.push_str(
        r#"
### 🚫 严格规则
1. **禁止建议** `SET enable_xxx = true` 如果该参数在"已启用的功能"列表中
2. 只能建议开启"已禁用的功能"列表中的参数
3. 违反以上规则将被视为严重错误!
"#,
    );

    prompt
}


// ============================================================================
// Valid Parameters Prompts
// ============================================================================

/// StarRocks valid parameters prompt
const PROMPT_VALID_PARAMS_STARROCKS: &str = r#"

## ✅ StarRocks 官方支持的参数 (已验证)

### Session 变量 (SET xxx = yyy)

**查询资源控制:**
- `query_mem_limit` - 单个查询内存限制 (bytes)
- `query_timeout` - 查询超时时间 (秒，默认300)
- `exec_mem_limit` - 单个 BE 节点内存限制

**并行度控制:**
- `pipeline_dop` - Pipeline 并行度 (0=自动)
- `parallel_fragment_exec_instance_num` - Fragment 实例数 (默认1)
- `max_parallel_scan_instance_num` - Scan 并行实例数

**Spill (落盘):**
- `enable_spill` - 启用落盘 (true/false)
- `spill_mem_table_size` - 落盘触发阈值
- `spill_mem_table_num` - 落盘表数量

**DataCache (仅外表):**
- `enable_scan_datacache` - 启用 DataCache 读取 (外表专用)
- `enable_populate_datacache` - 启用 DataCache 写入 (外表专用)

**Query Cache (仅内表):**
- `enable_query_cache` - 启用 Query Cache (仅内表聚合查询)
- `query_cache_entry_max_bytes` - 单个缓存条目最大字节
- `query_cache_entry_max_rows` - 单个缓存条目最大行数

**Runtime Filter:**
- `enable_global_runtime_filter` - 全局 Runtime Filter
- `runtime_filter_wait_time_ms` - 等待时间
- `runtime_join_filter_push_down_limit` - 下推行数限制

**Join 优化:**
- `broadcast_row_limit` - Broadcast 行数限制 (默认25M)
- `hash_join_push_down_right_table` - 右表下推

**聚合优化:**
- `new_planner_agg_stage` - 聚合阶段 (0=自动,1/2/3/4)
- `streaming_preaggregation_mode` - 预聚合模式

### ALTER TABLE 属性 (仅内表)
- `replication_num` - 副本数
- `bloom_filter_columns` - Bloom Filter 列
- `colocate_with` - Colocate Group 名称
- `dynamic_partition.enable` - 动态分区开关
- `storage_medium` - 存储介质 (SSD/HDD)

### 运维命令
- `ANALYZE TABLE db.table;` - 更新统计信息 (仅内表)
- `REFRESH MATERIALIZED VIEW mv_name;` - 刷新物化视图
- `ADMIN SET REPLICA STATUS ...` - 管理副本

### SQL Hint 格式
```sql
SELECT /*+ SET_VAR(query_timeout=600, enable_spill=true) */ ...
```

## ❌ 禁止使用的参数
- ❌ `enable_short_key_index` - 不存在
- ❌ `enable_zone_map_index` - 不存在
- ❌ `enable_bitmap_index` - 不存在
- ❌ `enable_async_profile` - 不存在
- ❌ `optimize_table` - 不存在

## ⚠️ 外表限制
- ❌ `ALTER TABLE external_table SET (...)` - 外表属性在源端修改
- ❌ `ANALYZE TABLE external_catalog.db.table` - 外表统计信息在源端
- ❌ `enable_query_cache = true` - Query Cache 不支持外表

## 🔄 缓存策略
| 缓存类型 | 适用表类型 | 参数 |
|---------|-----------|------|
| Query Cache | 内表 | `enable_query_cache` |
| DataCache | 外表 | `enable_scan_datacache` |
| PageCache | 内表 | 自动 |
"#;

/// Doris valid parameters prompt
const PROMPT_VALID_PARAMS_DORIS: &str = r#"

## ✅ Doris 官方支持的参数 (已验证)

### Session 变量 (SET xxx = yyy)

**查询资源控制:**
- `exec_mem_limit` - 单个查询内存限制 (bytes，默认2GB)
- `query_timeout` - 查询超时时间 (秒，默认900)
- `insert_timeout` - 导入超时时间 (秒，默认14400)

**并行度控制:**
- `parallel_fragment_exec_instance_num` - Fragment 实例数 (默认1)
- `parallel_pipeline_task_num` - Pipeline 任务并行度 (0=自动)
- `max_instance_num` - 最大实例数

**Pipeline 执行引擎:**
- `enable_pipeline_engine` - 启用 Pipeline 引擎 (默认true)
- `enable_pipeline_x_engine` - 启用 PipelineX 引擎 (2.1+)
- `enable_local_shuffle` - 启用本地 Shuffle

**Spill (落盘):**
- `enable_spill` - 启用落盘 (true/false)
- `min_revocable_mem` - 最小可回收内存

**文件缓存 (外表专用):**
- `enable_file_cache` - 启用文件缓存 (外表专用)

**Runtime Filter:**
- `runtime_filter_mode` - Runtime Filter 模式 (OFF/LOCAL/GLOBAL)
- `runtime_filter_type` - Runtime Filter 类型 (IN/BLOOM/MIN_MAX)
- `runtime_filter_wait_time_ms` - 等待时间 (默认1000ms)
- `runtime_bloom_filter_max_size` - Bloom Filter 最大大小

**Join 优化:**
- `broadcast_row_count_limit` - Broadcast 行数限制 (默认1048576)
- `enable_bucket_shuffle_join` - 启用 Bucket Shuffle Join
- `enable_colocate_join` - 启用 Colocate Join

**聚合优化:**
- `enable_distinct_streaming_aggregation` - 启用流式聚合
- `streaming_preaggregation_mode` - 预聚合模式

**向量化执行:**
- `enable_vectorized_engine` - 启用向量化引擎 (默认true)
- `batch_size` - 向量化批次大小 (默认4096)

### ALTER TABLE 属性 (仅内表)
- `replication_num` - 副本数
- `bloom_filter_columns` - Bloom Filter 列
- `colocate_with` - Colocate Group 名称
- `dynamic_partition.enable` - 动态分区开关
- `storage_medium` - 存储介质 (SSD/HDD)
- `compaction_policy` - Compaction 策略

### 运维命令
- `ANALYZE TABLE db.table;` - 更新统计信息 (仅内表)
- `REFRESH MATERIALIZED VIEW mv_name;` - 刷新物化视图
- `ADMIN COMPACT TABLE db.table;` - 手动触发 Compaction

### SQL Hint 格式
```sql
SELECT /*+ SET_VAR(exec_mem_limit=8589934592, query_timeout=600) */ ...
```

## ❌ 禁止使用的参数
- ❌ `enable_scan_datacache` - StarRocks 参数，Doris 用 `enable_file_cache`
- ❌ `enable_populate_datacache` - StarRocks 参数
- ❌ `pipeline_dop` - StarRocks 参数，Doris 用 `parallel_pipeline_task_num`
- ❌ `enable_query_cache` - Doris 2.0+ 使用 SQL Cache

## ⚠️ 外表限制 (Multi-Catalog)
- ❌ `ALTER TABLE external_table SET (...)` - 外表属性在源端修改
- ❌ `ANALYZE TABLE catalog.db.table` - 外表统计信息在源端

## 🔄 缓存策略
| 缓存类型 | 适用表类型 | 参数 |
|---------|-----------|------|
| SQL Cache | 内表 | 自动 (2.0+) |
| File Cache | 外表 | `enable_file_cache` |
| PageCache | 内表 | 自动 |
"#;

/// Get valid params prompt based on cluster type
fn get_valid_params_prompt(cluster_type: ClusterType) -> &'static str {
    match cluster_type {
        ClusterType::StarRocks => PROMPT_VALID_PARAMS_STARROCKS,
        ClusterType::Doris => PROMPT_VALID_PARAMS_DORIS,
    }
}


// ============================================================================
// Output Format and System Prompt Builder
// ============================================================================

/// Output format specification
const PROMPT_OUTPUT_FORMAT: &str = r#"

## 📤 严格 JSON 输出格式"#;

/// Output format JSON schema
const PROMPT_JSON_FORMAT: &str = r#"

```json
{
  "root_causes": [
    {
      "root_cause_id": "RC001",
      "description": "root cause description based on raw metrics analysis",
      "confidence": 0.85,
      "evidence": ["Profile metric evidence 1", "evidence 2"],
      "symptoms": ["S001", "G003"],
      "is_implicit": false
    }
  ],
  "causal_chains": [
    {
      "chain": ["Root Cause", "->", "Intermediate", "->", "Symptom"],
      "explanation": "Causal analysis based on Profile data"
    }
  ],
  "recommendations": [
    {
      "priority": 1,
      "action": "Brief description of recommended action",
      "expected_improvement": "Quantitative improvement description",
      "sql_example": "Executable SQL or command"
    }
  ],
  "summary": "Overall analysis summary focusing on root causes and optimization direction",
  "hidden_issues": [
    {
      "issue": "Issue not detected by rule engine",
      "suggestion": "Executable solution command"
    }
  ]
}
```

Field descriptions:
- root_cause_id: Format as "RC001", "RC002", etc.
- evidence: MUST reference specific Profile metric values
- symptoms: Related rule IDs
- is_implicit: true if not detected by rule engine
- priority: 1 is highest priority
- sql_example: REQUIRED, executable SQL/command
"#;

/// Build the complete dynamic system prompt
pub fn build_system_prompt(request: &RootCauseAnalysisRequest) -> String {
    let cluster_type = request.query_summary.cluster_type;

    let mut prompt = build_base_prompt(cluster_type);

    if let Some(ref profile_data) = request.profile_data {
        prompt.push_str(&build_table_type_prompt(&profile_data.scan_details, cluster_type));
    }

    prompt.push_str(&build_issue_focused_prompt(&request.rule_diagnostics));
    prompt.push_str(&build_session_vars_prompt(&request.query_summary.session_variables));
    prompt.push_str(get_valid_params_prompt(cluster_type));
    prompt.push_str(PROMPT_OUTPUT_FORMAT);
    prompt.push_str(PROMPT_JSON_FORMAT);

    prompt
}

/// Legacy static prompt for backward compatibility (minimal)
#[allow(dead_code)]
pub const ROOT_CAUSE_SYSTEM_PROMPT: &str = "You are a StarRocks OLAP database performance expert.";


// ============================================================================
// Request Types
// ============================================================================

/// Root Cause Analysis Request to LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAnalysisRequest {
    /// Query summary information
    pub query_summary: QuerySummaryForLLM,
    /// Raw profile data for deep analysis
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_data: Option<ProfileDataForLLM>,
    /// Execution plan (simplified for token efficiency)
    pub execution_plan: ExecutionPlanForLLM,
    /// Rule engine diagnostics (for reference, LLM should go deeper)
    pub rule_diagnostics: Vec<DiagnosticForLLM>,
    /// Key performance metrics
    pub key_metrics: KeyMetricsForLLM,
    /// Optional user question for follow-up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_question: Option<String>,
}

impl LLMAnalysisRequestTrait for RootCauseAnalysisRequest {
    fn scenario(&self) -> LLMScenario {
        LLMScenario::RootCauseAnalysis
    }

    fn system_prompt(&self) -> String {
        build_system_prompt(self)
    }

    fn cache_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.sql_hash().hash(&mut hasher);
        self.profile_hash().hash(&mut hasher);
        format!("rca:{:x}", hasher.finish())
    }

    fn sql_hash(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.query_summary.sql_statement.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn profile_hash(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.query_summary.scan_bytes.hash(&mut hasher);
        self.query_summary.output_rows.hash(&mut hasher);
        self.rule_diagnostics.len().hash(&mut hasher);
        self.query_summary.query_type.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

/// Query summary for LLM analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuerySummaryForLLM {
    /// Full SQL statement (NOT truncated - LLM needs complete SQL for analysis)
    pub sql_statement: String,
    /// Query type: SELECT/INSERT/EXPORT/ANALYZE
    pub query_type: String,
    /// Query complexity level: "Simple" | "Medium" | "Complex" | "VeryComplex"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_complexity: Option<String>,
    /// Cluster type for prompt selection
    #[serde(default)]
    pub cluster_type: ClusterType,
    /// Total execution time in seconds
    pub total_time_seconds: f64,
    /// Total bytes scanned
    pub scan_bytes: u64,
    /// Output row count
    pub output_rows: u64,
    /// Number of BE nodes
    pub be_count: u32,
    /// Whether spill occurred
    pub has_spill: bool,
    /// Spill details if spill occurred
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill_bytes: Option<String>,
    /// Non-default session variables (important for analysis)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub session_variables: HashMap<String, String>,
}


// ============================================================================
// Profile Data Types
// ============================================================================

/// Raw profile data for LLM deep analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDataForLLM {
    /// All operator nodes with their metrics
    pub operators: Vec<OperatorDetailForLLM>,
    /// Cross-node time distribution (for detecting skew)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_distribution: Option<TimeDistributionForLLM>,
    /// Scan node details (tables, partitions, files)
    #[serde(default)]
    pub scan_details: Vec<ScanDetailForLLM>,
    /// Join node details (join type, build/probe stats)
    #[serde(default)]
    pub join_details: Vec<JoinDetailForLLM>,
    /// Aggregation node details
    #[serde(default)]
    pub agg_details: Vec<AggDetailForLLM>,
    /// Exchange (shuffle) details
    #[serde(default)]
    pub exchange_details: Vec<ExchangeDetailForLLM>,
}

/// Detailed operator information with all metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorDetailForLLM {
    pub operator: String,
    pub plan_node_id: i32,
    pub time_pct: f64,
    pub rows: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    pub metrics: HashMap<String, String>,
}

/// Time distribution across instances for skew detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeDistributionForLLM {
    pub max_time_ms: f64,
    pub min_time_ms: f64,
    pub avg_time_ms: f64,
    pub skew_ratio: f64,
    #[serde(default)]
    pub per_instance: Vec<InstanceTimeForLLM>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceTimeForLLM {
    pub operator: String,
    pub instance_id: i32,
    pub time_ms: f64,
    pub rows: u64,
}

/// Scan operator details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanDetailForLLM {
    pub plan_node_id: i32,
    pub table_name: String,
    pub scan_type: String,
    pub table_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector_type: Option<String>,
    pub rows_read: u64,
    pub rows_returned: u64,
    pub filter_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan_ranges: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predicates: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partitions_scanned: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_table_path: Option<String>,
}

/// Join operator details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinDetailForLLM {
    pub plan_node_id: i32,
    pub join_type: String,
    pub build_rows: u64,
    pub probe_rows: u64,
    pub output_rows: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_table_memory: Option<u64>,
    #[serde(default)]
    pub is_broadcast: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_filter: Option<String>,
}

/// Aggregation operator details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggDetailForLLM {
    pub plan_node_id: i32,
    pub input_rows: u64,
    pub output_rows: u64,
    pub agg_ratio: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by_keys: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_table_memory: Option<u64>,
    #[serde(default)]
    pub is_streaming: bool,
}

/// Exchange (shuffle) operator details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeDetailForLLM {
    pub plan_node_id: i32,
    pub exchange_type: String,
    pub bytes_sent: u64,
    pub rows_sent: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_time_ms: Option<f64>,
}


// ============================================================================
// Execution Plan and Diagnostic Types
// ============================================================================

/// Simplified execution plan for LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanForLLM {
    pub dag_description: String,
    #[serde(default)]
    pub hotspot_nodes: Vec<HotspotNodeForLLM>,
}

/// Hotspot node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotNodeForLLM {
    pub operator: String,
    pub plan_node_id: i32,
    pub time_percentage: f64,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub key_metrics: HashMap<String, String>,
    #[serde(default)]
    pub upstream_operators: Vec<String>,
}

/// Rule engine diagnostic result for LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticForLLM {
    pub rule_id: String,
    pub severity: String,
    pub operator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_node_id: Option<i32>,
    pub message: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub evidence: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold_info: Option<ThresholdInfoForLLM>,
}

/// Threshold information for LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdInfoForLLM {
    pub threshold_value: f64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_p95_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<usize>,
}

/// Key performance metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyMetricsForLLM {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skew_metrics: Option<SkewMetricsForLLM>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub io_metrics: Option<IOMetricsForLLM>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_metrics: Option<MemoryMetricsForLLM>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cardinality_errors: Vec<CardinalityErrorForLLM>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkewMetricsForLLM {
    pub max_rows: u64,
    pub min_rows: u64,
    pub avg_rows: f64,
    pub skew_ratio: f64,
    pub affected_operator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IOMetricsForLLM {
    pub total_bytes_read: u64,
    pub cache_hit_rate: f64,
    pub io_time_percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetricsForLLM {
    pub peak_memory_bytes: u64,
    pub spill_bytes: u64,
    pub hash_table_memory: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardinalityErrorForLLM {
    pub operator: String,
    pub estimated_rows: u64,
    pub actual_rows: u64,
    pub error_ratio: f64,
}


// ============================================================================
// Response Types
// ============================================================================

/// Root Cause Analysis Response from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAnalysisResponse {
    #[serde(default)]
    pub root_causes: Vec<LLMRootCause>,
    #[serde(default)]
    pub causal_chains: Vec<LLMCausalChain>,
    #[serde(default)]
    pub recommendations: Vec<LLMRecommendation>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub hidden_issues: Vec<LLMHiddenIssue>,
}

impl LLMAnalysisResponseTrait for RootCauseAnalysisResponse {
    fn summary(&self) -> &str {
        &self.summary
    }

    fn confidence(&self) -> Option<f64> {
        if self.root_causes.is_empty() {
            None
        } else {
            Some(self.root_causes.iter().map(|r| r.confidence).sum::<f64>() / self.root_causes.len() as f64)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRootCause {
    pub root_cause_id: String,
    pub description: String,
    pub confidence: f64,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub symptoms: Vec<String>,
    #[serde(default)]
    pub is_implicit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMCausalChain {
    pub chain: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRecommendation {
    pub priority: u32,
    pub action: String,
    #[serde(default)]
    pub expected_improvement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sql_example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMHiddenIssue {
    pub issue: String,
    pub suggestion: String,
}


// ============================================================================
// Builder for RootCauseAnalysisRequest
// ============================================================================

impl RootCauseAnalysisRequest {
    pub fn builder() -> RootCauseAnalysisRequestBuilder {
        RootCauseAnalysisRequestBuilder::default()
    }
}

#[derive(Default)]
pub struct RootCauseAnalysisRequestBuilder {
    query_summary: Option<QuerySummaryForLLM>,
    profile_data: Option<ProfileDataForLLM>,
    execution_plan: Option<ExecutionPlanForLLM>,
    rule_diagnostics: Vec<DiagnosticForLLM>,
    key_metrics: KeyMetricsForLLM,
    user_question: Option<String>,
}

impl RootCauseAnalysisRequestBuilder {
    pub fn query_summary(mut self, summary: QuerySummaryForLLM) -> Self {
        self.query_summary = Some(summary);
        self
    }

    pub fn profile_data(mut self, data: ProfileDataForLLM) -> Self {
        self.profile_data = Some(data);
        self
    }

    pub fn execution_plan(mut self, plan: ExecutionPlanForLLM) -> Self {
        self.execution_plan = Some(plan);
        self
    }

    pub fn add_diagnostic(mut self, diag: DiagnosticForLLM) -> Self {
        self.rule_diagnostics.push(diag);
        self
    }

    pub fn diagnostics(mut self, diags: Vec<DiagnosticForLLM>) -> Self {
        self.rule_diagnostics = diags;
        self
    }

    pub fn key_metrics(mut self, metrics: KeyMetricsForLLM) -> Self {
        self.key_metrics = metrics;
        self
    }

    pub fn user_question(mut self, question: impl Into<String>) -> Self {
        self.user_question = Some(question.into());
        self
    }

    pub fn build(self) -> Result<RootCauseAnalysisRequest, &'static str> {
        Ok(RootCauseAnalysisRequest {
            query_summary: self.query_summary.ok_or("query_summary is required")?,
            profile_data: self.profile_data,
            execution_plan: self.execution_plan.ok_or("execution_plan is required")?,
            rule_diagnostics: self.rule_diagnostics,
            key_metrics: self.key_metrics,
            user_question: self.user_question,
        })
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Determine table type based on CATALOG prefix
///
/// Rules:
/// - `default_catalog.db.table` → internal
/// - `other_catalog.db.table` → external
/// - `db.table` (2 parts) → internal (default catalog)
/// - `table` (1 part) → internal
pub fn determine_table_type(table_name: &str) -> String {
    let parts: Vec<&str> = table_name.split('.').collect();
    match parts.len() {
        3.. => {
            // catalog.db.table format
            if parts[0].eq_ignore_ascii_case("default_catalog") {
                "internal".to_string()
            } else {
                "external".to_string()
            }
        }
        _ => "internal".to_string(), // db.table or just table
    }
}

/// Determine external table connector type from Profile metrics
pub fn determine_connector_type(metrics: &HashMap<String, String>) -> String {
    let keys_str = metrics.keys().map(|k| k.to_lowercase()).collect::<Vec<_>>().join(" ");
    let has = |p: &str| keys_str.contains(p);

    if has("iceberg") || has("deletefilebuild") {
        "iceberg"
    } else if has("deletionvector") {
        "deltalake"
    } else if has("hudi") {
        "hudi"
    } else if has("paimon") {
        "paimon"
    } else if has("jdbc") {
        "jdbc"
    } else if has("elasticsearch") || has("_es_") {
        "es"
    } else if ["orc", "parquet", "stripe", "rowgroup"].iter().any(|p| has(p)) {
        "hive"
    } else {
        "unknown"
    }
    .to_string()
}
