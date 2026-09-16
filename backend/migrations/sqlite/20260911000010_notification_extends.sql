-- 通知扩展：级别与关联对象元数据（产品化 kind 矩阵）
ALTER TABLE notifications ADD COLUMN severity VARCHAR(10) NOT NULL DEFAULT 'info';
ALTER TABLE notifications ADD COLUMN meta_json TEXT;
CREATE INDEX IF NOT EXISTS idx_notifications_user_created ON notifications(user_id, created_at);
