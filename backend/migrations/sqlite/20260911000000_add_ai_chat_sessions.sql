-- ==============================================
-- AI 应用共享会话（智能问数 ask × 运维助手 agent 共用，channel 隔离）
-- 设计见 docs/agent/ai-common-design.md
-- ==============================================
CREATE TABLE IF NOT EXISTS ai_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel VARCHAR(20) NOT NULL DEFAULT 'agent',
    organization_id INTEGER,
    user_id INTEGER,
    cluster_id INTEGER NOT NULL,
    title TEXT,
    catalog TEXT,
    database_name TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    last_active_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ai_sessions_scope
    ON ai_sessions(channel, cluster_id, last_active_at DESC);

-- ==============================================
-- AI 消息：角色 + 内容 + 各自业务扩展列（steps_json=agent 审计链，
-- generated_sql/guard/explain/chart 等=ask 扩展，互不写入对方列）
-- ==============================================
CREATE TABLE IF NOT EXISTS ai_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    role VARCHAR(10) NOT NULL,
    content TEXT NOT NULL,
    steps_json TEXT,
    generated_sql TEXT,
    guard_status VARCHAR(20),
    guard_reason TEXT,
    explain_text TEXT,
    chart_json TEXT,
    context_json TEXT,
    llm_session_id INTEGER,
    execution_history_id INTEGER,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (session_id) REFERENCES ai_sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ai_messages_session
    ON ai_messages(session_id, id);
