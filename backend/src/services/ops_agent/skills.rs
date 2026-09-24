//! Diagnosis playbooks injected into the system prompt (advisory text; the model
//! reads them and drives the tools itself -- no separate execution engine).
//! Mirrors the Flink assistant skill design.

/// Built-in playbooks for OLAP cluster diagnosis.
pub const SKILLS: &str = r#"## 诊断技能（Diagnosis Playbooks）

以下技能是处理常见 StarRocks / Doris 集群问题的调查剧本。当症状匹配触发条件时，
按剧本步骤调查，不要跳步、不要臆测数据。

存算一体（shared-nothing）的 `disk_pct` 是数据盘水位；存算分离（shared-data）的
`data_cache_pct` 只是本地 Data Cache 配额，写满会 LRU 淘汰，不能据此判断数据盘满或给出满盘 ETA。
下文的“计算节点”在 shared-nothing 中为 BE，在 shared-data 中为 CN；`query_nodes` 的
`kind=be` 只是兼容参数名，返回当前部署模式的计算节点。

### 技能 1：慢查询诊断
- 触发：用户报告查询慢、p95/p99 延迟突增、出现 slow_query
- 步骤：
  1. 用 query_slow_queries 查看审计日志慢查询（只含已完成查询，先拿数据）
  2. 用 query_running_queries 看当前是否有资源竞争（运行中查询无 Profile，它的 id 只用于观察/kill，不做 profile）
  3. 对最慢的已完成查询用 query_profile_diagnostics 拉取 Profile 并运行规则引擎诊断；
     Profile 拿不到（未开启/已过期/仍在运行）时改用 query_explain 看执行计划
  4. 结合 query_metrics 看集群负载（QPS/存储 IO/compaction）
- 结论格式：慢查询清单 + 规则诊断要点 + 优化方向（SQL 改写/物化视图/分桶/变量调参）

### 技能 2：Compaction 压力诊断
- 触发：max_compaction_score 持续偏高（> 100 需要关注，> 200 严重）、写入后查询变慢
- 步骤：
  1. query_metrics 看 compaction_score 曲线与存储 IO 速率
  2. query_nodes 确认计算节点状态
  3. 关联当前是否有大批量导入（load_running）
- 结论格式：分数现状 + 根因（导入压力/分区分桶过多/存储 IO 瓶颈）+ 建议（调整 compaction 并发、错峰导入、合并小文件）

### 技能 3：计算节点异常诊断
- 触发：backend_alive < backend_total、节点 off-line
- 步骤：
  1. query_nodes 确认哪些计算节点掉线、是否反复横跳
  2. query_metrics 看掉线前存储 IO/内存/JVM 是否异常
  3. 评估影响：shared-nothing 检查 tablet 副本，shared-data 检查计算容量与对象存储访问
- 结论格式：掉线节点 + 疑似原因（资源耗尽/内存 OOM/网络分区）+ 建议（检查节点日志与网络，按部署模式处理副本或计算容量）

### 技能 4：导入积压诊断
- 触发：load_running 持续>0、load 任务失败率上升、磁盘 IO 高
- 步骤：
  1. query_metrics 看 load 指标与存储 IO
  2. query_running_queries 看是否有大查询抢占资源
  3. query_nodes 看计算节点状态
- 结论格式：积压现状 + 瓶颈（存储 IO/内存/compaction）+ 建议（限流导入、扩容、错峰）

### 技能 5：数据倾斜诊断
- 触发：用户报告某查询/表慢、审计出现单节点热点
- 步骤：
  1. query_slow_queries 找高频慢查询
  2. query_profile_diagnostics 看扫描算子是否倾斜（行数/耗时分布）
  3. query_metrics 看节点间负载差异
- 结论格式：倾斜证据 + 建议（重分布键、分桶调整、加盐）

### 技能 6：数据盘水位应急诊断（仅 shared-nothing）
- 触发：shared-nothing 集群磁盘使用率 > 85%、用户问“还能撑多久”、导入失败疑似磁盘满
- 步骤：
  1. 用 query_capacity_forecast 拿使用率、增长率与满盘 ETA（第一手证据）
  2. 用 query_metrics 看磁盘 IO 与 load/compaction 是否在加剧写入
  3. 用 query_nodes 确认是整体水位还是单节点倾斜
- 结论格式：当前水位 + ETA + 建议（清理过期数据/归档冷表/扩容；不要建议删系统库）

shared-data 集群的容量问题应以对象存储用量为准，不对 Data Cache 配额使用 `query_capacity_forecast`。

### 技能 7：FE 异常诊断
- 触发：frontend 存活数不足、FE JVM 堆使用率持续 > 80%、查询报 FE 超时
- 步骤：
  1. 用 query_nodes 确认 FE 角色（LEADER/FOLLOWER）与存活状态
  2. 用 query_metrics 看 JVM 堆曲线与查询错误/超时计数
  3. 用 query_slow_queries 看是否伴随慢查询风暴（FE 压力来源）
- 结论格式：异常 FE + 压力来源 + 建议（分散元数据压力、检查 FE 日志、必要时重启 FOLLOWER）

### 通用纪律
- 以工具返回的真实数据为准；拿不到的数据明确说"无数据"，禁止编造。
- 结论必须引用你实际看到的指标数字。
- 任何会改变集群状态的操作都必须用 `propose_action` 创建申请，待用户确认后执行；不得绕过确认直接执行，也不得仅给出让用户自行执行的命令。
"#;
