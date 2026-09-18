-- Stellar application log archive: restricted to platform administrators.
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:system:logs:archive', '下载 Stellar 日志包', 'api', 'system', 'logs:archive', 'GET /api/system/logs/archive');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system')
WHERE code = 'api:system:logs:archive';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'admin'), id FROM permissions
WHERE code = 'api:system:logs:archive';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'super_admin'), id FROM permissions
WHERE code = 'api:system:logs:archive';
