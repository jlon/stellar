INSERT IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:clusters:schema:list', '浏览Schema对象', 'api', 'clusters', 'schema:list', 'GET /api/clusters/:cluster_id/schema/objects'),
('api:clusters:schema:get', '查看Schema对象详情', 'api', 'clusters', 'schema:get', 'GET /api/clusters/:cluster_id/schema/objects/:object_ref'),
('api:clusters:schema:refresh', '刷新Schema对象', 'api', 'clusters', 'schema:refresh', 'POST /api/clusters/:cluster_id/schema/objects/:object_ref/refresh'),
('api:clusters:schema:dependencies', '查看Schema对象依赖', 'api', 'clusters', 'schema:dependencies', 'GET /api/clusters/:cluster_id/schema/objects/:object_ref/dependencies');

UPDATE permissions
SET parent_id = (SELECT _t.id FROM (SELECT id FROM permissions WHERE code = 'menu:queries:execution') AS _t)
WHERE code IN ('api:clusters:schema:list', 'api:clusters:schema:get', 'api:clusters:schema:refresh', 'api:clusters:schema:dependencies');

INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'admin'), id FROM permissions
WHERE code IN ('api:clusters:schema:list', 'api:clusters:schema:get', 'api:clusters:schema:refresh', 'api:clusters:schema:dependencies');
INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'super_admin'), id FROM permissions
WHERE code IN ('api:clusters:schema:list', 'api:clusters:schema:get', 'api:clusters:schema:refresh', 'api:clusters:schema:dependencies');
INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('api:clusters:schema:list', 'api:clusters:schema:get', 'api:clusters:schema:refresh', 'api:clusters:schema:dependencies')
WHERE r.code LIKE 'org_admin_%';
