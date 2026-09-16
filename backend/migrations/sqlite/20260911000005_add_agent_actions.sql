-- ==============================================
-- Agent 动作闭环：两阶段确认写动作（对齐 Flink platform write actions：
-- UUID 高熵不可猜测 / TTL 过期 / 单次执行 / 结果留档审计）
-- 设计见 docs/agent/ops-agent-design.md 动作闭环
-- ==============================================

CREATE TABLE IF NOT EXISTS agent_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_id INTEGER NOT NULL,
    cluster_id INTEGER NOT NULL,
    kind VARCHAR(30) NOT NULL,
    title TEXT NOT NULL,
    params_json TEXT NOT NULL,
    status VARCHAR(12) NOT NULL DEFAULT 'pending',
    action_uuid VARCHAR(36) NOT NULL UNIQUE,
    created_by TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    confirmed_at TIMESTAMP,
    confirmed_by TEXT,
    executed_at TIMESTAMP,
    result_json TEXT,
    FOREIGN KEY (incident_id) REFERENCES agent_incidents(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_agent_actions_incident ON agent_actions(incident_id, status);
