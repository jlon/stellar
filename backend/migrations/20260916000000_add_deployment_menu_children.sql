-- Deployment menu reorganization: seven child menu codes replace the flat
-- eight-entry list, so the sidebar can hide sections per permission and every
-- route is guarded consistently.
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('menu:deployment:hosts', '主机管理', 'menu', 'deployment:hosts', 'view', '查看部署主机资产'),
('menu:deployment:credentials', 'SSH 凭据', 'menu', 'deployment:credentials', 'view', '查看部署凭据资产'),
('menu:deployment:packages', '安装包', 'menu', 'deployment:packages', 'view', '查看受控安装包资产'),
('menu:deployment:clusters', '托管集群', 'menu', 'deployment:clusters', 'view', '查看受管集群'),
('menu:deployment:adopt', '集群接管', 'menu', 'deployment:adopt', 'view', '只读接管现有集群'),
('menu:deployment:deploy', '新建部署', 'menu', 'deployment:deploy', 'view', '创建物理机部署'),
('menu:deployment:tasks', '部署任务', 'menu', 'deployment:tasks', 'view', '查看部署任务');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:deployment')
WHERE code IN (
    'menu:deployment:hosts',
    'menu:deployment:credentials',
    'menu:deployment:packages',
    'menu:deployment:clusters',
    'menu:deployment:adopt',
    'menu:deployment:deploy',
    'menu:deployment:tasks'
);

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code = 'super_admin'
  AND p.code IN (
      'menu:deployment:hosts',
      'menu:deployment:credentials',
      'menu:deployment:packages',
      'menu:deployment:clusters',
      'menu:deployment:adopt',
      'menu:deployment:deploy',
      'menu:deployment:tasks'
  );

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code LIKE 'org_admin_%'
  AND p.code IN (
      'menu:deployment:hosts',
      'menu:deployment:credentials',
      'menu:deployment:packages',
      'menu:deployment:clusters',
      'menu:deployment:adopt',
      'menu:deployment:deploy',
      'menu:deployment:tasks'
  );

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.code = 'admin'
  AND p.code IN (
      'menu:deployment:hosts',
      'menu:deployment:credentials',
      'menu:deployment:packages',
      'menu:deployment:clusters',
      'menu:deployment:adopt',
      'menu:deployment:deploy',
      'menu:deployment:tasks'
  );
