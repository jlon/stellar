-- Production closure for physical deployment operations:
-- - task_type gains scale_out / config_change / decommission
-- - sr_managed_clusters gains status 'removed' and a bootstrap_credential_id
--   so production clusters that secure the root account can still deploy
-- CHECK constraints require SQLite table rebuilds; every row and index is
-- preserved through backup tables.

CREATE TABLE sr_config_revisions_p0_backup AS
SELECT id, managed_cluster_id, node_id, revision, content, content_sha256, task_id, created_by, created_at
FROM sr_config_revisions;

CREATE TABLE sr_operation_events_p0_backup AS
SELECT id, task_id, step, status, node_id, message, created_at FROM sr_operation_events;

CREATE TABLE sr_operation_tasks_p0_backup AS
SELECT id, organization_id, managed_cluster_id, task_type, payload_json, status,
       current_step, error_message, result_json, created_by, created_at, started_at, finished_at
FROM sr_operation_tasks;

CREATE TABLE sr_observed_nodes_p0_backup AS
SELECT id, managed_cluster_id, node_type, address, raw_json, created_at FROM sr_observed_nodes;

CREATE TABLE sr_host_port_allocations_p0_backup AS
SELECT host_id, port, managed_cluster_id FROM sr_host_port_allocations;

CREATE TABLE sr_host_allocations_p0_backup AS
SELECT host_id, managed_cluster_id, created_at FROM sr_host_allocations;

CREATE TABLE sr_cluster_nodes_p0_backup AS
SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port,
       query_port, rpc_port, brpc_port, webserver_port, starlet_port, meta_dir, storage_dir,
       status, created_at, updated_at
FROM sr_cluster_nodes;

DROP TABLE sr_config_revisions;
DROP TABLE sr_operation_events;
DROP TABLE sr_operation_tasks;
DROP TABLE sr_observed_nodes;
DROP TABLE sr_host_port_allocations;
DROP TABLE sr_host_allocations;
DROP TABLE sr_cluster_nodes;

CREATE TABLE sr_managed_clusters_p0_mig (
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
    bootstrap_credential_id INTEGER REFERENCES sr_database_credentials(id),
    status TEXT NOT NULL DEFAULT 'planning'
        CHECK (status IN ('planning', 'deploying', 'running', 'failed', 'adopted_read_only', 'removed')),
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO sr_managed_clusters_p0_mig
    (id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir,
     ssh_credential_id, package_id, operator_credential_id, bootstrap_credential_id,
     status, created_by, created_at, updated_at)
SELECT id, organization_id, name, deployment_mode, sr_version, cluster_id, install_dir,
       ssh_credential_id, package_id, operator_credential_id, NULL, status, created_by,
       created_at, updated_at
FROM sr_managed_clusters;

DROP TABLE sr_managed_clusters;
ALTER TABLE sr_managed_clusters_p0_mig RENAME TO sr_managed_clusters;

CREATE TABLE sr_operation_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id INTEGER NOT NULL REFERENCES organizations(id),
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    task_type TEXT NOT NULL
        CHECK (task_type IN ('deploy', 'adopt_read_only', 'node_command', 'import_cluster',
                             'scale_out', 'config_change', 'decommission')),
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

INSERT INTO sr_operation_tasks
    (id, organization_id, managed_cluster_id, task_type, payload_json, status,
     current_step, error_message, result_json, created_by, created_at, started_at, finished_at)
SELECT id, organization_id, managed_cluster_id, task_type, payload_json, status,
       current_step, error_message, result_json, created_by, created_at, started_at, finished_at
FROM sr_operation_tasks_p0_backup;

CREATE UNIQUE INDEX idx_sr_one_running_task
    ON sr_operation_tasks(managed_cluster_id)
    WHERE status = 'running';

CREATE INDEX idx_sr_tasks_organization_id ON sr_operation_tasks(organization_id, created_at DESC);

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
    starlet_port INTEGER,
    meta_dir TEXT,
    storage_dir TEXT,
    status TEXT NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned', 'installed', 'running', 'stopped', 'failed', 'removed')),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(managed_cluster_id, host_id, role, service_port)
);

INSERT INTO sr_cluster_nodes
    (id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port,
     query_port, rpc_port, brpc_port, webserver_port, starlet_port, meta_dir, storage_dir,
     status, created_at, updated_at)
SELECT id, managed_cluster_id, host_id, role, fe_role, advertise_host, service_port, http_port,
       query_port, rpc_port, brpc_port, webserver_port, starlet_port, meta_dir, storage_dir,
       status, created_at, updated_at
FROM sr_cluster_nodes_p0_backup;

CREATE TABLE sr_host_allocations (
    host_id INTEGER PRIMARY KEY REFERENCES physical_hosts(id) ON DELETE CASCADE,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO sr_host_allocations (host_id, managed_cluster_id, created_at)
SELECT host_id, managed_cluster_id, created_at FROM sr_host_allocations_p0_backup;

CREATE TABLE sr_host_port_allocations (
    host_id INTEGER NOT NULL REFERENCES physical_hosts(id) ON DELETE CASCADE,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    PRIMARY KEY (host_id, port)
);

INSERT INTO sr_host_port_allocations (host_id, port, managed_cluster_id)
SELECT host_id, port, managed_cluster_id FROM sr_host_port_allocations_p0_backup;

CREATE INDEX idx_sr_host_port_allocations_cluster
    ON sr_host_port_allocations(managed_cluster_id);

CREATE TABLE sr_observed_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    node_type TEXT NOT NULL CHECK (node_type IN ('fe', 'be')),
    address TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO sr_observed_nodes (id, managed_cluster_id, node_type, address, raw_json, created_at)
SELECT id, managed_cluster_id, node_type, address, raw_json, created_at FROM sr_observed_nodes_p0_backup;

CREATE INDEX idx_sr_observed_nodes_cluster_id ON sr_observed_nodes(managed_cluster_id, id);

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

CREATE INDEX idx_sr_events_task_id ON sr_operation_events(task_id, id);

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

INSERT INTO sr_config_revisions
    (id, managed_cluster_id, node_id, revision, content, content_sha256, task_id, created_by, created_at)
SELECT id, managed_cluster_id, node_id, revision, content, content_sha256, task_id, created_by, created_at
FROM sr_config_revisions_p0_backup;

CREATE INDEX idx_sr_config_revisions_cluster
    ON sr_config_revisions(managed_cluster_id, node_id, revision DESC);

DROP TABLE sr_config_revisions_p0_backup;
DROP TABLE sr_operation_events_p0_backup;
DROP TABLE sr_operation_tasks_p0_backup;
DROP TABLE sr_observed_nodes_p0_backup;
DROP TABLE sr_host_port_allocations_p0_backup;
DROP TABLE sr_host_allocations_p0_backup;
DROP TABLE sr_cluster_nodes_p0_backup;

INSERT OR IGNORE INTO permissions (code, name, type, resource, action, description) VALUES
('api:sr-ops:clusters:delete', '删除托管集群', 'api', 'sr-ops', 'clusters:delete', '退役托管集群并释放端口预占');

UPDATE permissions
SET parent_id = (SELECT id FROM permissions WHERE code = 'menu:deployment')
WHERE code = 'api:sr-ops:clusters:delete';

INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE (r.code = 'admin' OR r.code = 'super_admin' OR r.code LIKE 'org_admin_%')
  AND p.code = 'api:sr-ops:clusters:delete';
