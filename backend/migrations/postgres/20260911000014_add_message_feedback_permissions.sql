-- 回答点赞/点踩权限（挂 menu:agent，admin/super_admin）
INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:messages:feedback', '评价助手的回答', 'api', 'agent', 'messages:feedback', 'POST /api/agent/messages/:id/feedback');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:messages:feedback');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:agent:messages:feedback');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:agent:messages:feedback');
