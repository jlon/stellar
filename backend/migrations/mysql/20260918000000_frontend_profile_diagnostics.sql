CREATE TABLE IF NOT EXISTS frontend_profile_access_logs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    user_id BIGINT NOT NULL,
    username VARCHAR(100) NOT NULL,
    organization_id BIGINT,
    cluster_id BIGINT NOT NULL,
    cluster_name VARCHAR(200) NOT NULL,
    frontend_name VARCHAR(200) NOT NULL,
    frontend_host VARCHAR(255) NOT NULL,
    http_port INTEGER NOT NULL,
    action VARCHAR(16) NOT NULL,
    profile_filename VARCHAR(255),
    outcome VARCHAR(16) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    KEY idx_frontend_profile_access_cluster_time (cluster_id, created_at),
    KEY idx_frontend_profile_access_user_time (user_id, created_at)
);

INSERT IGNORE INTO permissions (code, name, type, resource, action, description)
VALUES (
    'api:clusters:frontends:diagnose',
    'Frontend节点诊断',
    'api',
    'clusters',
    'frontends:diagnose',
    'GET /api/clusters/frontends/profiles'
);
UPDATE permissions
SET parent_id = (SELECT _t.id FROM (SELECT id FROM permissions WHERE code = 'menu:nodes:frontends') AS _t)
WHERE code = 'api:clusters:frontends:diagnose';
INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'admin'), id FROM permissions
WHERE code = 'api:clusters:frontends:diagnose';
INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'super_admin'), id FROM permissions
WHERE code = 'api:clusters:frontends:diagnose';
INSERT IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'api:clusters:frontends:diagnose'
WHERE r.code LIKE 'org_admin_%';
