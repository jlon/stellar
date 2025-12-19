# Doris Profile Parser 对比分析

## 概述

本文档对比了 Doris 官方文档 `profile-dag-parser.md` 中定义的解析逻辑与 Stellar 当前实现的差异，并识别出需要改进的地方。

## 关键差异分析

### 1. Pipeline instance_num 提取

**文档要求**：
- 正则表达式：`^Pipeline (\\d+)\\(instance_num=(\\d+)\\):`
- 需要捕获两个组：Pipeline ID 和 instance_num

**当前实现**：
```rust
static PIPELINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*Pipeline\s*:?\s*(?:\(id=(\d+)\)|(\d+)\(instance_num=)").unwrap());
```

**问题**：
- ✅ 可以匹配 Doris 格式 `Pipeline 0(instance_num=1):`
- ❌ 没有提取 `instance_num` 的值
- ❌ 正则表达式没有完整匹配 `instance_num=(\d+)` 部分

**影响**：
- `instance_num` 信息丢失，虽然不影响 DAG 构建，但可能影响性能分析

**建议修复**：
```rust
// 修改正则表达式以捕获 instance_num
static PIPELINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*Pipeline\s*:?\s*(?:\(id=(\d+)\)|(\d+)\(instance_num=(\d+)\))").unwrap());

// 在 parse_pipelines 中提取 instance_num
if let Some(caps) = PIPELINE_REGEX.captures(line.trim()) {
    let id = caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str().to_string()).unwrap_or_else(|| "0".to_string());
    let instance_num = caps.get(3).and_then(|m| m.as_str().parse::<u32>().ok());
    // 可以将 instance_num 存储到 Pipeline 结构体中（如果模型支持）
}
```

### 2. Counter 正则表达式过于严格

**文档要求**：
- 正则表达式：`^- ([^:]+): (.+)`
- 匹配格式：`- CounterName: value`
- Counter 名称可以是任何非冒号字符

**当前实现**：
```rust
static METRIC_LINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*-\s+([A-Za-z_][A-Za-z0-9_]*)(?::\s+(.+))?$").unwrap());
```

**问题**：
- ✅ 可以匹配标准的 counter 名称（如 `ExecTime`, `RowsProduced`）
- ❌ 要求 counter 名称必须是标识符格式（字母开头，只能包含字母数字下划线）
- ❌ 无法匹配包含特殊字符的 counter 名称（如 `Counter-Name`, `Counter.Name`）

**影响**：
- 如果 Doris Profile 中有包含特殊字符的 counter 名称，可能无法解析

**建议修复**：
```rust
// 放宽正则表达式以匹配文档要求
static METRIC_LINE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*-\s+([^:]+):\s*(.+)$").unwrap());
```

### 3. Operator 正则表达式差异

**文档要求**：
- 正则表达式：`^([A-Z_]+_OPERATOR)(?:\\([^)]+\\))?\\(id=(\\d+)\\):`
- 要求必须以 `_OPERATOR` 结尾

**当前实现**：
```rust
static OPERATOR_HEADER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Z_]+(?:\s*\((?:(?:plan_node_id|id)=\d+[^)]*)\))?:$").unwrap());
```

**问题**：
- ✅ 可以匹配 StarRocks 格式 `OPERATOR_NAME (plan_node_id=0):`
- ✅ 可以匹配 Doris 格式 `OPERATOR_NAME(id=0):`
- ❌ 不要求必须以 `_OPERATOR` 结尾（更宽松）
- ⚠️ 文档要求更严格，但当前实现更灵活，可能匹配到非 Operator 的节点

**影响**：
- 当前实现可能匹配到非 Operator 的节点（虽然不太可能，因为 Profile 格式规范）
- 但更宽松的实现可能更健壮

**建议**：
- 保持当前实现，因为：
  1. 实际 Profile 中所有 Operator 都以 `_OPERATOR` 结尾
  2. 更宽松的正则表达式可以处理格式变化
  3. 有额外的检查逻辑（`is_operator_header`）过滤非 Operator

### 4. DataCache 命中率提取位置

**文档要求**：
- 从 ExecutionSummary 中提取 DataCache 命中率

**当前实现**：
- 从整个 Profile 文本中提取 DataCache 相关指标（`DataCacheReadDiskBytes`, `DataCacheReadMemBytes`, `FSIOBytesRead`）
- 通过正则表达式在整个文本中搜索

**问题**：
- ⚠️ 文档提到从 ExecutionSummary 提取，但实际 Profile 中 DataCache 指标在 Operator 的 CustomCounters 中
- ✅ 当前实现更准确，因为 DataCache 指标确实在 Operator 级别

**建议**：
- 保持当前实现，因为：
  1. DataCache 指标实际在 Operator 的 CustomCounters 中，不在 ExecutionSummary
  2. 从整个文本提取更可靠
  3. 文档可能描述不准确或过时

### 5. execTimeNs（纳秒）用于排序

**文档要求**：
- DagNode 包含 `execTimeNs`（纳秒），用于排序

**当前实现**：
- `ExecutionTreeNode` 包含 `metrics.operator_total_time`（纳秒）
- `compute_top_time_consuming_nodes` 使用 `time_percentage` 排序，而不是 `execTimeNs`

**问题**：
- ✅ 有纳秒级别的时间信息（`operator_total_time`）
- ⚠️ 排序使用 `time_percentage` 而不是 `execTimeNs`

**影响**：
- 使用 `time_percentage` 排序更合理，因为它是相对于总执行时间的百分比
- 文档要求使用 `execTimeNs` 排序，但实际场景中百分比排序更有意义

**建议**：
- 保持当前实现（使用 `time_percentage` 排序），因为：
  1. 百分比排序更能反映节点的相对重要性
  2. 文档可能只是示例，实际使用中百分比排序更合理

## 总结

### 需要修复的问题

1. **Pipeline instance_num 提取**（优先级：低）
   - 当前可以匹配但未提取值
   - 不影响核心功能，但丢失了有用信息

2. **Counter 正则表达式过于严格**（优先级：中）
   - 可能无法解析包含特殊字符的 counter 名称
   - 建议放宽正则表达式

### 不需要修改的部分

1. **Operator 正则表达式**：当前实现更灵活，保持现状
2. **DataCache 提取位置**：当前实现更准确，保持现状
3. **排序方式**：使用 `time_percentage` 排序更合理，保持现状

## 建议的修复优先级

1. **高优先级**：无
2. **中优先级**：修复 Counter 正则表达式
3. **低优先级**：提取 Pipeline instance_num

## 测试建议

修复后需要测试：
1. 包含特殊字符的 counter 名称是否能正确解析
2. Pipeline instance_num 是否能正确提取和存储
3. 所有现有测试用例仍然通过

