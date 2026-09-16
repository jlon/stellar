-- 通知权限（铃铛对所有登录用户可见；播种 admin/super_admin）
INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:notifications', '查看我的通知', 'api', 'notifications', 'list', 'GET /api/notifications'),
('api:notifications:create', '创建通知', 'api', 'notifications', 'create', 'POST /api/notifications'),
('api:notifications:read', '标记通知已读', 'api', 'notifications', 'read', 'POST /api/notifications/:id/read');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system')
WHERE code IN ('api:notifications', 'api:notifications:create', 'api:notifications:read');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:notifications', 'api:notifications:create', 'api:notifications:read');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:notifications', 'api:notifications:create', 'api:notifications:read');
