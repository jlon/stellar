-- ===========================================
-- Add missing database authentication API permissions
-- ===========================================
-- Date: 2025-12-22
-- Purpose: Add permissions for my-permissions and role-permissions APIs

-- Insert new API permissions
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:db-auth:my-permissions', '查询我的数据库权限', 'api', 'db-auth', 'my-permissions', 'GET /api/clusters/db-auth/my-permissions'),
('api:db-auth:role-permissions', '查询角色权限详情', 'api', 'db-auth', 'role-permissions', 'GET /api/clusters/db-auth/role-permissions/:role_name');

-- Set parent_id for new permissions (under 权限申请 menu)
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:cluster-ops:auth:requests')
WHERE code IN ('api:db-auth:my-permissions', 'api:db-auth:role-permissions');

-- Grant new permissions to admin role
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code IN ('api:db-auth:my-permissions', 'api:db-auth:role-permissions');

-- Grant new permissions to super_admin role
INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code IN ('api:db-auth:my-permissions', 'api:db-auth:role-permissions');
