CREATE TABLE IF NOT EXISTS op_audit_logs (
    id BIGINT PRIMARY KEY AUTO_INCREMENT,
    user_id BIGINT NOT NULL,
    username VARCHAR(100) NOT NULL DEFAULT '',
    organization_id BIGINT,
    action VARCHAR(20) NOT NULL,
    target_type VARCHAR(30) NOT NULL,
    target_id BIGINT,
    target_name VARCHAR(200) NOT NULL DEFAULT '',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_op_audit_user ON op_audit_logs(user_id, created_at);
CREATE INDEX idx_op_audit_target ON op_audit_logs(target_type, target_id);
