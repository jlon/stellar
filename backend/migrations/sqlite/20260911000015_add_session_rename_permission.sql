-- 会话重命名权限（挂 menu:agent，admin/super_admin）
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:sessions:rename', '重命名会话', 'api', 'agent', 'sessions:rename', 'PATCH /api/agent/sessions/:id');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:sessions:rename');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:agent:sessions:rename');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:agent:sessions:rename');
