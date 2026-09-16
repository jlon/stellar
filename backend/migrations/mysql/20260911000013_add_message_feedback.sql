-- 回答点赞/点踩：用户对单条 assistant 消息的反馈（GPT 同款）
CREATE TABLE IF NOT EXISTS agent_message_feedback (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    message_id BIGINT NOT NULL,
    session_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    rating VARCHAR(8) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE KEY uq_msg_feedback_user (message_id, user_id)
);
CREATE INDEX idx_msg_feedback_session ON agent_message_feedback(session_id);
