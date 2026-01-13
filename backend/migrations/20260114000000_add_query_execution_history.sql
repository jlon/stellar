-- ==============================================
-- Query Execution History Table
-- 用户在实时查询页面执行的 SQL 历史记录
-- ==============================================
CREATE TABLE IF NOT EXISTS query_execution_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    cluster_id INTEGER NOT NULL,
    catalog VARCHAR(100),
    database_name VARCHAR(100),
    sql_statement TEXT NOT NULL,
    sql_hash VARCHAR(64) NOT NULL,
    execution_time_ms INTEGER,
    row_count INTEGER,
    success BOOLEAN NOT NULL DEFAULT 1,
    error_message TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (cluster_id) REFERENCES clusters(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_query_history_user_cluster 
    ON query_execution_history(user_id, cluster_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_query_history_sql_hash 
    ON query_execution_history(user_id, cluster_id, sql_hash);

-- ==============================================
-- 权限配置
-- 查询历史 API 权限绑定到实时查询菜单权限
-- URI: /api/clusters/queries/execution-history
-- 权限提取: resource=clusters, action=queries:execution:history
-- ==============================================
INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:clusters:queries:execution:history', '查询执行历史', 'api', 'clusters', 'queries:execution:history', 'GET/DELETE /api/clusters/queries/execution-history');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:queries:execution')
WHERE code = 'api:clusters:queries:execution:history';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='admin'), id FROM permissions
WHERE code = 'api:clusters:queries:execution:history';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT (SELECT id FROM roles WHERE code='super_admin'), id FROM permissions
WHERE code = 'api:clusters:queries:execution:history';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r, permissions p
WHERE r.code = 'cluster_admin' 
AND p.code = 'api:clusters:queries:execution:history';
