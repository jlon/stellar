# 智能运维 Agent 产品闭环规划

> 版本: v0.1 · 2026-09-11
> 视角: 产品经理 · 闭环框架: **发现 → 通知 → 诊断 → 处置 → 验证 → 复盘**

## 一、闭环现状盘点（断点即机会）

| 环节 | 现状 | 状态 |
|---|---|---|
| 发现 | 30s 采集 → 收敛 → Incident 升级（规则确定性优先） | ✅ 已闭环 |
| 通知 | Incident 产生**无任何触达**，用户只能主动打开工作台 | ❌ **断点 1** |
| 诊断 | 规则剧本 + 证据板 + LLM 根因假设（证据 ID 校验护栏） | ✅ 已闭环 |
| 处置 | 两阶段确认动作（kill_query / update_variable，UUID/TTL/单次执行） | ⚠️ 可用但摩擦高：建议是自然语言，动作需人工建表单；无移动端入口 |
| 验证 | 动作执行后**无人跟进**：kill 后慢查询消失了吗？变量生效了吗？ | ❌ **断点 2** |
| 复盘 | 无周报、无 LLM 假设命中率回填 → 飞轮不转 | ❌ **断点 3** |

## 二、下一步 TODO（按用户价值排序）

### P0 — 闭环最后一公里（做完才能叫"闭环产品"）

1. **Incident 升级通知触达**（断点 1）
   - Incident 新建/升级 critical → 飞书 Webhook 推送（标题/影响/置信度/工作台链接）
   - 配置开关 + 目标 URL（按集群可配置）
   - 验收：真实磁盘临界 → 飞书收到消息 → 点击链接直达 Incident 详情
2. **动作验证闭环**（断点 2）
   - 确认执行后后台验证器轮询恢复指标：kill_query → 目标 query 不再出现；update_variable → 变量值生效
   - 验证结果写入 action.result_json（verified/failed_to_verify）；Incident 提供「验证并关闭」
   - 验收：真实执行动作后留档验证结论；假动作（指向不存在 query）诚实标 failed
3. **结构化动作建议**（处置摩擦，接驳回动）
   - 诊断 actions 从自然语言升级为结构化 {kind, params 建议值, risk}（kill_query 直接带证据中的 query_id）
   - Incident 页「一键创建待确认动作」→ 人工只做确认
   - 验收：容量 Incident 一键生成 kill/变量动作，确认弹窗展示全部参数
4. **飞书审批卡做两阶段确认**（移动端 MTTR）
   - 动作 pending → 飞书互动卡片（确认/拒绝按钮）→ 回写 agent_actions 状态
   - 与 Web 确认共用同一原子状态翻转（单次执行语义不变）
   - 验收：飞书卡片确认后动作执行、结果回推卡片

### P1 — 价值增量

5. **容量自适应**（原六阶段）
   - metrics_snapshots 磁盘趋势线性回归 → 满盘 ETA（天）→ capacity 诊断输出 ETA 与建议
   - 验收：真实趋势数据给出 ETA 与扩容/清理建议，进入 Incident 详情
6. **阈值集群化**：DISK 80/90、SLOW_QUERY_MS 等从硬编码改为集群配置（默认值不变）
7. **AIOps 运营周报**：事件/Incident 统计、处置动作清单、LLM 假设命中率回填（验证环节反馈）→ 飞书推送

### P2 — 体验与效率

8. Agent 自身可观测性：采集成功率 / 流水线耗时 / LLM token 成本仪表盘
9. SSE token 级流式（模型思考逐字呈现，前端打字机最后一公里）
10. 多集群告警分级路由（不同集群 → 不同通知对象/通道）

## 三、建议实施顺序与理由

```
P0-1（通知）→ P0-2（验证）→ P0-3（建议结构化）→ P0-4（飞书确认）
                                        ↓
               P1-5（容量）→ P1-6（阈值）→ P1-7（周报）
                                        ↓
                        P2-8/9/10 按运营数据驱动
```

理由：1→2 让「发现-处置-验证」先形成数据闭环（用户能证明 Agent 有用）；3 吃掉最大人工摩擦；
4 借力飞书（项目所在生态）把确认带到移动端；随后容量/周报让产品从"工具"变成"运营助手"。
---

## 通知体系全盘设计（v0.2 增补）

### 目标
单一通知模型服务所有异步场景：对话回合、事件闭环、动作闭环、系统消息；
未来任何新异步能力（报表、容量建议、飞书桥接）只需加 kind，不重造轮子。

### 数据模型（已落地 + 扩展）
```
notifications(
  id, user_id, kind, severity(info/warning/critical),
  title, body, link, meta_json(关联对象 ids，供深链/去重/扩展),
  read, created_at)
```

### kind 矩阵（类型 × 触发时机 × 内容 × 跳转）

| kind | 触发 | 内容 | link | severity |
|---|---|---|---|---|
| agent_chat_done | 对话回合成功完成（用户不在前台） | 答案摘要 ≤120 | 会话页?session= | info |
| agent_chat_error | 回合失败/异常（用户不在前台） | 错误摘要 | 会话页 | critical |
| incident_created | 新 Incident（含 critical 事件） | 标题/根因摘要/置信度 | 事件中心?incident= | critical |
| incident_reopened | 30 天内复开 | 同上 | 同上 | warning |
| action_pending | 运维动作创建待确认 | 动作类型/参数摘要/有效期 | 事件中心?incident= | warning |
| action_result | 动作确认执行后 | 成功/失败 + 结果摘要 | 事件中心?incident= | success→info / failed→critical |
| system | 平台消息（预留） | 任意 | 可选 | info |

### 通知策略（免打扰）
- 对话类：仅"无会话 UI 在前台"时通知（前端 AgentChatService 判定）
- 事件/动作类：后端直接落库（后台产生，工作台页面与铃铛同步可见）；动作确认动作由用户触发不通知 pending 取消

### 扩展路线（todo）
1. 已读全部 / 分页 / 历史清理（TTL 30 天）
2. EventSource 推送替换 60s 轮询
3. 按 kind 的偏好开关（每用户）
4. 飞书桥接：通知流出层（同 kind → 飞书消息/审批卡），站内通知不变
5. 事件聚合去重（同一 incident 多次 notification 的合并展示）
