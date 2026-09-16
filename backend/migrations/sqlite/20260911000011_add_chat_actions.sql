-- 对话内授权执行：LLM 受控申请 → 用户确认 → MySQLClient 通道执行
-- 审计链：申请用户/LLM 理由/确认人/参数/结果/时间 全量留档
CREATE TABLE IF NOT EXISTS agent_chat_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    kind VARCHAR(30) NOT NULL,
    title TEXT NOT NULL,
    params_json TEXT NOT NULL,
    reason TEXT,
    status VARCHAR(12) NOT NULL DEFAULT 'pending',
    action_uuid VARCHAR(36) NOT NULL UNIQUE,
    created_by TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    confirmed_at TIMESTAMP,
    confirmed_by TEXT,
    executed_at TIMESTAMP,
    result_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_chat_actions_session ON agent_chat_actions(session_id, status);
