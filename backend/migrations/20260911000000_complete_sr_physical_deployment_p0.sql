-- StarRocks physical deployment P0: credentials, managed topology and task history.

CREATE TABLE ssh_credentials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    name TEXT NOT NULL,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL CHECK (auth_type = 'key'),
    algorithm TEXT NOT NULL DEFAULT 'aes-256-gcm',
    key_version INTEGER NOT NULL DEFAULT 1,
    secret_nonce BLOB NOT NULL,
    secret_ciphertext BLOB NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(organization_id, name)
);

CREATE TABLE sr_database_credentials (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    name TEXT NOT NULL,
    username TEXT NOT NULL,
    algorithm TEXT NOT NULL DEFAULT 'aes-256-gcm',
    key_version INTEGER NOT NULL DEFAULT 1,
    secret_nonce BLOB NOT NULL,
    secret_ciphertext BLOB NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(organization_id, name)
);

ALTER TABLE sr_packages ADD COLUMN cached_path TEXT;
ALTER TABLE sr_packages ADD COLUMN size_bytes INTEGER;

CREATE TABLE sr_managed_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    name TEXT NOT NULL UNIQUE,
    deployment_mode TEXT NOT NULL DEFAULT 'shared_nothing'
        CHECK (deployment_mode = 'shared_nothing'),
    sr_version TEXT NOT NULL,
    cluster_id INTEGER REFERENCES clusters(id) ON DELETE SET NULL,
    install_dir TEXT,
    ssh_credential_id INTEGER REFERENCES ssh_credentials(id),
    package_id INTEGER REFERENCES sr_packages(id),
    operator_credential_id INTEGER REFERENCES sr_database_credentials(id),
    status TEXT NOT NULL DEFAULT 'planning'
        CHECK (status IN ('planning', 'deploying', 'running', 'failed', 'adopted_read_only')),
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sr_host_allocations (
    host_id INTEGER PRIMARY KEY REFERENCES physical_hosts(id) ON DELETE CASCADE,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sr_cluster_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    host_id INTEGER NOT NULL REFERENCES physical_hosts(id),
    role TEXT NOT NULL CHECK (role IN ('fe', 'be')),
    fe_role TEXT CHECK (fe_role IN ('leader', 'follower')),
    advertise_host TEXT NOT NULL,
    service_port INTEGER NOT NULL CHECK (service_port BETWEEN 1 AND 65535),
    http_port INTEGER,
    query_port INTEGER,
    rpc_port INTEGER,
    brpc_port INTEGER,
    webserver_port INTEGER,
    meta_dir TEXT,
    storage_dir TEXT,
    status TEXT NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned', 'installed', 'running', 'stopped', 'failed', 'removed')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(managed_cluster_id, host_id, role, service_port)
);

CREATE TABLE sr_operation_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    task_type TEXT NOT NULL CHECK (task_type IN ('deploy', 'adopt_read_only')),
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'interrupted', 'cancelled')),
    current_step TEXT,
    error_message TEXT,
    result_json TEXT,
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TIMESTAMP,
    finished_at TIMESTAMP
);

CREATE UNIQUE INDEX idx_sr_one_running_task
    ON sr_operation_tasks(managed_cluster_id)
    WHERE status = 'running';

CREATE TABLE sr_operation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES sr_operation_tasks(id) ON DELETE CASCADE,
    step TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'skipped')),
    node_id INTEGER REFERENCES sr_cluster_nodes(id),
    message TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_sr_tasks_organization_id ON sr_operation_tasks(organization_id, created_at DESC);
CREATE INDEX idx_sr_events_task_id ON sr_operation_events(task_id, id);

CREATE TABLE sr_observed_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    node_type TEXT NOT NULL CHECK (node_type IN ('fe', 'be')),
    address TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_sr_observed_nodes_cluster_id ON sr_observed_nodes(managed_cluster_id, id);

INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:sr-ops:credentials:list', '查询 SSH 凭据', 'api', 'sr-ops', 'credentials:list', 'GET /api/sr-ops/credentials'),
('api:sr-ops:credentials:manage', '管理 SSH 凭据', 'api', 'sr-ops', 'credentials:manage', 'POST /api/sr-ops/credentials'),
('api:sr-ops:credentials:delete', '删除 SSH 凭据', 'api', 'sr-ops', 'credentials:delete', 'DELETE /api/sr-ops/credentials/:id'),
('api:sr-ops:db-credentials:list', '查询数据库凭据', 'api', 'sr-ops', 'db-credentials:list', 'GET /api/sr-ops/database-credentials'),
('api:sr-ops:db-credentials:manage', '管理数据库凭据', 'api', 'sr-ops', 'db-credentials:manage', 'POST /api/sr-ops/database-credentials'),
('api:sr-ops:db-credentials:delete', '删除数据库凭据', 'api', 'sr-ops', 'db-credentials:delete', 'DELETE /api/sr-ops/database-credentials/:id'),
('api:sr-ops:clusters:list', '查询托管集群', 'api', 'sr-ops', 'clusters:list', 'GET /api/sr-ops/clusters'),
('api:sr-ops:clusters:get', '查看托管集群', 'api', 'sr-ops', 'clusters:get', 'GET /api/sr-ops/clusters/:id'),
('api:sr-ops:deployments:create', '创建部署任务', 'api', 'sr-ops', 'deployments:create', 'POST /api/sr-ops/deployments'),
('api:sr-ops:tasks:list', '查询部署任务', 'api', 'sr-ops', 'tasks:list', 'GET /api/sr-ops/tasks'),
('api:sr-ops:tasks:get', '查看部署任务', 'api', 'sr-ops', 'tasks:get', 'GET /api/sr-ops/tasks/:id'),
('api:sr-ops:tasks:cancel', '取消部署任务', 'api', 'sr-ops', 'tasks:cancel', 'POST /api/sr-ops/tasks/:id/cancel');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:deployment')
WHERE code LIKE 'api:sr-ops:%';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE (r.code = 'admin' OR r.code = 'super_admin' OR r.code LIKE 'org_admin_%')
  AND p.code LIKE 'api:sr-ops:%';
