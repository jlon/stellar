CREATE TABLE IF NOT EXISTS frontend_profile_access_logs (
    id BIGSERIAL PRIMARY KEY,
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_frontend_profile_access_cluster_time
    ON frontend_profile_access_logs(cluster_id, created_at);
CREATE INDEX IF NOT EXISTS idx_frontend_profile_access_user_time
    ON frontend_profile_access_logs(user_id, created_at);

INSERT INTO permissions (code, name, type, resource, action, description)
VALUES (
    'api:clusters:frontends:diagnose',
    'Frontend节点诊断',
    'api',
    'clusters',
    'frontends:diagnose',
    'GET /api/clusters/frontends/profiles'
)
ON CONFLICT (code) DO NOTHING;
UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:nodes:frontends')
WHERE code = 'api:clusters:frontends:diagnose';
INSERT INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'admin'), id FROM permissions
WHERE code = 'api:clusters:frontends:diagnose'
ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code = 'super_admin'), id FROM permissions
WHERE code = 'api:clusters:frontends:diagnose'
ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code = 'api:clusters:frontends:diagnose'
WHERE r.code LIKE 'org_admin_%'
ON CONFLICT DO NOTHING;
