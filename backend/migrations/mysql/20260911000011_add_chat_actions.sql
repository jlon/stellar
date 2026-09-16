CREATE TABLE IF NOT EXISTS agent_chat_actions (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    session_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    kind VARCHAR(30) NOT NULL,
    title TEXT NOT NULL,
    params_json TEXT NOT NULL,
    reason TEXT,
    status VARCHAR(12) NOT NULL DEFAULT 'pending',
    action_uuid VARCHAR(36) NOT NULL UNIQUE,
    created_by TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    confirmed_at TIMESTAMP NULL,
    confirmed_by TEXT,
    executed_at TIMESTAMP NULL,
    result_json TEXT
);
CREATE INDEX idx_chat_actions_session ON agent_chat_actions(session_id, status);
