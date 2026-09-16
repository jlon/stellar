-- Agent 动作闭环权限（挂 menu:agent 下，admin/super_admin 授予）
INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:incidents:actions', '创建运维动作', 'api', 'agent', 'incidents:actions', 'POST /api/agent/incidents/:id/actions'),
('api:agent:incidents:actions:get', '查看运维动作', 'api', 'agent', 'incidents:actions:get', 'GET /api/agent/incidents/:id/actions'),
('api:agent:incidents:actions:confirm', '确认运维动作', 'api', 'agent', 'incidents:actions:confirm', 'POST /api/agent/incidents/:id/actions/:aid/confirm'),
('api:agent:incidents:actions:cancel', '取消运维动作', 'api', 'agent', 'incidents:actions:cancel', 'POST /api/agent/incidents/:id/actions/:aid/cancel');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:incidents:actions', 'api:agent:incidents:actions:get', 'api:agent:incidents:actions:confirm', 'api:agent:incidents:actions:cancel');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:agent:incidents:actions', 'api:agent:incidents:actions:get', 'api:agent:incidents:actions:confirm', 'api:agent:incidents:actions:cancel');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:agent:incidents:actions', 'api:agent:incidents:actions:get', 'api:agent:incidents:actions:confirm', 'api:agent:incidents:actions:cancel');
