-- 对话内动作确认权限（挂 menu:agent，admin/super_admin）
INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:chat-actions:list', '查看对话内动作', 'api', 'agent', 'chat-actions:list', 'GET /api/agent/chat-actions'),
('api:agent:chat-actions:confirm', '确认对话内动作', 'api', 'agent', 'chat-actions:confirm', 'POST /api/agent/chat-actions/:id/confirm'),
('api:agent:chat-actions:cancel', '拒绝对话内动作', 'api', 'agent', 'chat-actions:cancel', 'POST /api/agent/chat-actions/:id/cancel');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:chat-actions:confirm', 'api:agent:chat-actions:cancel');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:agent:chat-actions:confirm', 'api:agent:chat-actions:cancel');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:agent:chat-actions:confirm', 'api:agent:chat-actions:cancel');
