-- 独立的数据导入菜单/API 权限，并保持已具备查询读取权限的角色升级后可访问。
INSERT INTO permissions (code, name, type, resource, action, description) VALUES
('menu:loads', '数据导入', 'menu', 'loads', 'view', '查看数据导入管理'),
('api:clusters:loads', '查询导入任务', 'api', 'clusters', 'loads', 'GET /api/clusters/loads and /api/clusters/loads/:job_id')
ON CONFLICT DO NOTHING;

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:loads')
WHERE code = 'api:clusters:loads';

INSERT INTO role_permissions (role_id, permission_id)
SELECT DISTINCT rp.role_id, load_menu.id
FROM role_permissions rp
JOIN permissions previous_permission ON previous_permission.id = rp.permission_id
JOIN permissions load_menu ON load_menu.code = 'menu:loads'
WHERE previous_permission.code IN ('menu:queries:execution', 'api:clusters:queries')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT DISTINCT rp.role_id, load_api.id
FROM role_permissions rp
JOIN permissions previous_permission ON previous_permission.id = rp.permission_id
JOIN permissions load_api ON load_api.code = 'api:clusters:loads'
WHERE previous_permission.code = 'api:clusters:queries'
ON CONFLICT DO NOTHING;
