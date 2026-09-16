-- ===========================================
-- Add AI Agent (智能运维助手) Permissions
-- ===========================================
-- Date: 2026-09-11
-- Purpose: Menu + API permissions for the read-only ops agent chat & sessions

-- ============ Menu Permissions ============
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('menu:agent', '智能运维助手', 'menu', 'agent', 'view', 'AI 运维助手入口');

-- ============ API Permissions ============
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:chat', '发起 Agent 对话', 'api', 'agent', 'chat', 'POST /api/agent/chat'),
('api:agent:sessions:list', '查看会话列表', 'api', 'agent', 'sessions', 'GET /api/agent/sessions'),
('api:agent:sessions:get', '查看会话详情', 'api', 'agent', 'sessions:get', 'GET /api/agent/sessions/:id'),
('api:agent:sessions:delete', '删除会话', 'api', 'agent', 'sessions:delete', 'DELETE /api/agent/sessions/:id');

-- ============ Set API parents ============
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code IN ('api:agent:chat', 'api:agent:sessions:list', 'api:agent:sessions:get', 'api:agent:sessions:delete');

-- ============ Grant to Admin Roles ============
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('menu:agent', 'api:agent:chat', 'api:agent:sessions:list', 'api:agent:sessions:get', 'api:agent:sessions:delete');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('menu:agent', 'api:agent:chat', 'api:agent:sessions:list', 'api:agent:sessions:get', 'api:agent:sessions:delete');
