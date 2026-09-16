ALTER TABLE notifications ADD COLUMN severity VARCHAR(10) NOT NULL DEFAULT 'info';
ALTER TABLE notifications ADD COLUMN meta_json TEXT;
CREATE INDEX idx_notifications_user_created ON notifications(user_id, created_at);
