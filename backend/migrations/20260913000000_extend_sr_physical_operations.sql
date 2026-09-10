-- Physical deployment P1: pre-provisioned packages, config revisions, and
-- SSH-managed cluster operations (node commands, explicit cluster import).
ALTER TABLE sr_packages ADD COLUMN local_path TEXT;

-- sr_operation_tasks.task_type gains 'node_command' and 'import_cluster'.
-- SQLite cannot alter a CHECK constraint, so the table is rebuilt while
-- preserving every row and index for audit history.
CREATE TABLE sr_operation_events_p0_backup AS
SELECT id, task_id, step, status, node_id, message, created_at FROM sr_operation_events;

DROP TABLE sr_operation_events;

CREATE TABLE sr_operation_tasks_p0_mig (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    task_type TEXT NOT NULL
        CHECK (task_type IN ('deploy', 'adopt_read_only', 'node_command', 'import_cluster')),
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

INSERT INTO sr_operation_tasks_p0_mig
    (id, organization_id, managed_cluster_id, task_type, payload_json, status,
     current_step, error_message, result_json, created_by, created_at, started_at, finished_at)
SELECT id, organization_id, managed_cluster_id, task_type, payload_json, status,
       current_step, error_message, result_json, created_by, created_at, started_at, finished_at
FROM sr_operation_tasks;

DROP TABLE sr_operation_tasks;

ALTER TABLE sr_operation_tasks_p0_mig RENAME TO sr_operation_tasks;

CREATE UNIQUE INDEX idx_sr_one_running_task
    ON sr_operation_tasks(managed_cluster_id)
    WHERE status = 'running';

CREATE INDEX idx_sr_tasks_organization_id ON sr_operation_tasks(organization_id, created_at DESC);

CREATE TABLE sr_operation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL REFERENCES sr_operation_tasks(id) ON DELETE CASCADE,
    step TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'skipped')),
    node_id INTEGER REFERENCES sr_cluster_nodes(id),
    message TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO sr_operation_events
    (id, task_id, step, status, node_id, message, created_at)
SELECT id, task_id, step, status, node_id, message, created_at
FROM sr_operation_events_p0_backup;

DROP TABLE sr_operation_events_p0_backup;

CREATE INDEX idx_sr_events_task_id ON sr_operation_events(task_id, id);

-- Every controlled FE/BE configuration distribution is versioned so the UI can
-- show history and diffs without granting arbitrary config writes.
CREATE TABLE sr_config_revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL REFERENCES sr_cluster_nodes(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    content TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    task_id INTEGER REFERENCES sr_operation_tasks(id) ON DELETE SET NULL,
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (node_id, revision)
);

CREATE INDEX idx_sr_config_revisions_cluster
    ON sr_config_revisions(managed_cluster_id, node_id, revision DESC);

INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:sr-ops:clusters:manage', '管理托管集群', 'api', 'sr-ops', 'clusters:manage', '一键导入托管集群并执行节点启停命令'),
('api:sr-ops:clusters:logs', '查看节点日志', 'api', 'sr-ops', 'clusters:logs', 'GET /api/sr-ops/clusters/:id/nodes/:node/logs'),
('api:sr-ops:configs:read', '查看配置版本', 'api', 'sr-ops', 'configs:read', 'GET /api/sr-ops/clusters/:id/configs');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:deployment')
WHERE code IN ('api:sr-ops:clusters:manage', 'api:sr-ops:clusters:logs', 'api:sr-ops:configs:read');

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE (r.code = 'admin' OR r.code = 'super_admin' OR r.code LIKE 'org_admin_%')
  AND p.code IN ('api:sr-ops:clusters:manage', 'api:sr-ops:clusters:logs', 'api:sr-ops:configs:read');
