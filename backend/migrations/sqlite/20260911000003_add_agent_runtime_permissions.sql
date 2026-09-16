-- ===========================================
-- Add Agent Runtime (事件闭环) Permissions
-- ===========================================
-- Date: 2026-09-11
-- Purpose: Incident/Event 只读 + 手动调查/关闭权限

INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:incidents:list', '查看故障列表', 'api', 'agent', 'incidents', 'GET /api/agent/incidents'),
('api:agent:incidents:get', '查看故障详情', 'api', 'agent', 'incidents:get', 'GET /api/agent/incidents/:id'),
('api:agent:incidents:investigate', '手动调查故障', 'api', 'agent', 'incidents:investigate', 'POST /api/agent/incidents/:id/investigate'),
('api:agent:incidents:close', '关闭故障', 'api', 'agent', 'incidents:close', 'POST /api/agent/incidents/:id/close'),
('api:agent:events:list', '查看事件列表', 'api', 'agent', 'events', 'GET /api/agent/events');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:incidents:list', 'api:agent:incidents:get', 'api:agent:incidents:investigate', 'api:agent:incidents:close', 'api:agent:events:list');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:agent:incidents:list', 'api:agent:incidents:get', 'api:agent:incidents:investigate', 'api:agent:incidents:close', 'api:agent:events:list');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:agent:incidents:list', 'api:agent:incidents:get', 'api:agent:incidents:investigate', 'api:agent:incidents:close', 'api:agent:events:list');
