# Doris Profile 解析器改进总结

## 概述

根据 Doris 官方文档和源码（`ProfileDagParser.java`），对 Stellar 平台的 Profile 解析器进行了完善，确保能正确解析 Doris Profile 格式，同时保持对 StarRocks Profile 的完全兼容。

## 测试结果

✅ **StarRocks Profile**: 83 个测试全部通过  
✅ **Doris Profile**: 2 个测试全部通过

## 关键改进

### 1. MetricsParser 改进

**文件**: `backend/src/services/profile_analyzer/parser/core/metrics_parser.rs`

#### 改进内容

1. **支持 Doris Counter 格式**：
   - 添加了对 `CommonCounters:` 和 `CustomCounters:` 的支持
   - 保持对 StarRocks `CommonMetrics:` 和 `UniqueMetrics:` 的兼容
   - 优先尝试 Doris 格式，如果为空则回退到 StarRocks 格式

2. **改进 Counter 块提取逻辑**：
   - 根据 Doris `ProfileDagParser.java` 的实现，改进了 `extract_section_block` 的结束条件
   - 正确处理 `CommonCounters:` 和 `CustomCounters:` 标记
   - 在遇到下一个 Operator、Pipeline 或 Fragment 时正确停止

#### 关键代码

```rust
pub fn extract_common_metrics_block(text: &str) -> String {
    // Try Doris format first (CommonCounters:), then fallback to StarRocks format (CommonMetrics:)
    let doris_result = Self::extract_section_block(text, "CommonCounters:");
    if !doris_result.trim().is_empty() {
        return doris_result;
    }
    Self::extract_section_block(text, "CommonMetrics:")
}
```

### 2. FragmentParser 改进

**文件**: `backend/src/services/profile_analyzer/parser/core/fragment_parser.rs`

#### 改进内容

1. **改进 plan_node_id 提取逻辑**：
   - 正确处理 Doris 的复杂格式：`(id=0. nereids_id=74. table name = xxx)`
   - 提取 `id=` 后面的数字，忽略后续的点号和额外信息
   - 保持对 StarRocks `(plan_node_id=0)` 格式的兼容

2. **Pipeline 正则表达式**：
   - 已验证能正确匹配 `Pipeline 0(instance_num=1):` 格式
   - 同时支持 StarRocks 的 `Pipeline (id=0):` 格式

#### 关键代码

```rust
let plan_node_id = if full_header.contains("plan_node_id=") {
    // StarRocks format: (plan_node_id=0)
    // ... existing logic ...
} else if let Some(id_start) = full_header.find("(id=") {
    // Doris format: (id=0) or (id=0. nereids_id=32...) or (id=0. nereids_id=74. table name = xxx)
    // Extract the number immediately after "id=", before any dot or closing paren
    let after_id = &full_header[id_start + 4..]; // Skip "(id="
    let id_str = after_id
        .split(|c: char| c == '.' || c == ')')
        .next()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .map(|n| n.to_string());
    id_str
} else {
    None
};
```

### 3. ProfileComposer 改进

**文件**: `backend/src/services/profile_analyzer/parser/composer.rs`

#### 改进内容

1. **改进 MergedProfile 提取**：
   - 支持多种 MergedProfile 格式：
     - `MergedProfile:` (带冒号)
     - `MergedProfile ` (带空格)
     - `MergedProfile\n` (带换行)
   - 正确识别结束标记：`DetailProfile:` 或 `Execution Profile`
   - 添加详细的调试日志

2. **错误处理**：
   - 当 MergedProfile 块为空时输出警告日志
   - 包含 MergedProfile 块的前 500 字符预览，便于调试

#### 关键代码

```rust
fn extract_fragments_from_merged_profile(text: &str) -> ParseResult<Vec<Fragment>> {
    // Try different formats in order of likelihood
    let (marker, start) = if let Some(pos) = text.find("MergedProfile:") {
        ("MergedProfile:", pos)
    } else if let Some(pos) = text.find("MergedProfile ") {
        ("MergedProfile ", pos)
    } else if let Some(pos) = text.find("MergedProfile\n") {
        ("MergedProfile\n", pos)
    } else {
        tracing::warn!("[Doris] MergedProfile section not found in profile text");
        return Ok(Vec::new());
    };
    // ... rest of the logic ...
}
```

### 4. OperatorParser 改进

**文件**: `backend/src/services/profile_analyzer/parser/core/operator_parser.rs`

#### 改进内容

1. **改进 Operator 匹配逻辑**：
   - 正确处理 `OPERATOR_NAME(id=X):` 格式（无空格）
   - 正确处理 `OPERATOR_NAME (id=X. nereids_id=Y):` 格式（有空格和额外信息）
   - 确保匹配时检查是否为 Operator Header（以 `:` 结尾）

2. **Operator 名称提取**：
   - 从 `OPERATOR_NAME (id=0. nereids_id=74. table name = xxx)` 中正确提取 `OPERATOR_NAME`
   - 处理有空格和无空格两种情况

## 格式支持对比

### Pipeline 格式

| 格式 | StarRocks | Doris | 支持状态 |
|------|-----------|-------|----------|
| `Pipeline (id=0):` | ✅ | ❌ | ✅ 已支持 |
| `Pipeline 0(instance_num=1):` | ❌ | ✅ | ✅ 已支持 |
| `Pipeline : 0(instance_num=1):` | ❌ | ✅ | ✅ 已支持 |

### Operator 格式

| 格式 | StarRocks | Doris | 支持状态 |
|------|-----------|-------|----------|
| `OPERATOR_NAME (plan_node_id=0):` | ✅ | ❌ | ✅ 已支持 |
| `OPERATOR_NAME(id=0):` | ❌ | ✅ | ✅ 已支持 |
| `OPERATOR_NAME (id=0):` | ❌ | ✅ | ✅ 已支持 |
| `OPERATOR_NAME (id=0. nereids_id=74):` | ❌ | ✅ | ✅ 已支持 |
| `OPERATOR_NAME (id=0. nereids_id=74. table name = xxx):` | ❌ | ✅ | ✅ 已支持 |

### Counter 格式

| 格式 | StarRocks | Doris | 支持状态 |
|------|-----------|-------|----------|
| `CommonMetrics:` | ✅ | ❌ | ✅ 已支持 |
| `UniqueMetrics:` | ✅ | ❌ | ✅ 已支持 |
| `CommonCounters:` | ❌ | ✅ | ✅ 已支持 |
| `CustomCounters:` | ❌ | ✅ | ✅ 已支持 |

## 解析流程

根据 Doris `ProfileDagParser.java` 的实现，解析流程如下：

1. **定位 MergedProfile 部分**：查找 `MergedProfile:` 标记
2. **查找 Fragments 部分**：在 MergedProfile 中查找 `Fragments:` 标记
3. **解析 Fragment**：使用正则表达式 `^Fragment (\d+):` 匹配
4. **解析 Pipeline**：使用正则表达式 `^Pipeline (\d+)\(instance_num=(\d+)\):` 匹配
5. **解析 Operator**：使用正则表达式 `^([A-Z_]+_OPERATOR)(?:\([^)]+\))?\(id=(\d+)\):` 匹配
6. **解析 Counter**：
   - 遇到 `CommonCounters:` 时设置 `inCommonCounters = true`
   - 遇到 `CustomCounters:` 时设置 `inCustomCounters = true`
   - 解析以 `- ` 开头的 Counter 行
7. **构建 DAG**：根据层级关系建立父子节点关系

## 兼容性保证

### StarRocks Profile 兼容性

- ✅ 所有 83 个 StarRocks Profile 测试通过
- ✅ `CommonMetrics:` 和 `UniqueMetrics:` 格式正常解析
- ✅ `Pipeline (id=0):` 格式正常解析
- ✅ `OPERATOR_NAME (plan_node_id=0):` 格式正常解析

### Doris Profile 兼容性

- ✅ 2 个 Doris Profile 测试通过
- ✅ `CommonCounters:` 和 `CustomCounters:` 格式正常解析
- ✅ `Pipeline 0(instance_num=1):` 格式正常解析
- ✅ `OPERATOR_NAME(id=0):` 和 `OPERATOR_NAME (id=0. nereids_id=74):` 格式正常解析
- ✅ MergedProfile 提取正常

## 参考文档

- [Doris Profile 生成机制文档](./DORIS_PROFILE_GENERATION.md)
- [Doris Profile DAG Parser 文档](../../doris/docs/profile-dag-parser.md)
- [Doris ProfileDagParser.java 源码](../../doris/fe/fe-core/src/main/java/org/apache/doris/common/profile/ProfileDagParser.java)

## 下一步

1. ✅ 完成 MetricsParser 对 Doris Counter 格式的支持
2. ✅ 完成 FragmentParser 对复杂 Operator ID 格式的支持
3. ✅ 完成 ProfileComposer 对 MergedProfile 多种格式的支持
4. ⏳ 验证实际 live Doris Profile 的解析（需要测试集群）
5. ⏳ 根据实际使用情况进一步优化解析逻辑

## 注意事项

1. **向后兼容**：所有改进都确保不破坏 StarRocks Profile 的解析
2. **格式检测**：通过检查 Profile 文本开头是否为 `Summary:` 来区分 Doris 和 StarRocks 格式
3. **错误处理**：添加了详细的日志和错误信息，便于调试
4. **测试覆盖**：确保所有格式都有对应的测试用例

