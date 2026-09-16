-- 回答点赞/点踩：用户对单条 assistant 消息的反馈（GPT 同款）
CREATE TABLE IF NOT EXISTS agent_message_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    session_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    rating VARCHAR(8) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(message_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_msg_feedback_session ON agent_message_feedback(session_id);
