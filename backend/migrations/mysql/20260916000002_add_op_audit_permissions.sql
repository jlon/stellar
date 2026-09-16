-- 操作审计权限（系统管理下操作日志页；播种 admin/super_admin）
INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('menu:system:op-audit', '操作日志', 'menu', 'system:op-audit', 'view', '查看操作日志'),
('api:op-audit-logs:logs:list', '查询操作审计', 'api', 'op-audit-logs', 'logs:list', 'GET /api/op-audit-logs');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:system')
WHERE code IN ('menu:system:op-audit', 'api:op-audit-logs:logs:list');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('menu:system:op-audit', 'api:op-audit-logs:logs:list');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('menu:system:op-audit', 'api:op-audit-logs:logs:list');
