-- Add LLM root-cause analysis permission
INSERT INTO permissions (code, name, type, resource, action, description) VALUES
('api:agent:incidents:analyze', 'LLM 根因分析', 'api', 'agent', 'incidents:analyze', 'POST /api/agent/incidents/:id/analyze') ON CONFLICT DO NOTHING;

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:agent')
WHERE code = 'api:agent:incidents:analyze';

INSERT INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions WHERE code = 'api:agent:incidents:analyze';
INSERT INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions WHERE code = 'api:agent:incidents:analyze';
