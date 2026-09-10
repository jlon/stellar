-- A physical host can serve multiple shared-nothing clusters when every service port
-- and installation directory is isolated. The previous host-wide reservation remains
-- as historical data only; new deployments reserve their concrete service ports.
CREATE TABLE sr_host_port_allocations (
    host_id INTEGER NOT NULL REFERENCES physical_hosts(id) ON DELETE CASCADE,
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    managed_cluster_id INTEGER NOT NULL REFERENCES sr_managed_clusters(id) ON DELETE CASCADE,
    PRIMARY KEY (host_id, port)
);

INSERT OR IGNORE INTO sr_host_port_allocations (host_id, port, managed_cluster_id)
SELECT host_id, service_port, managed_cluster_id FROM sr_cluster_nodes
UNION ALL SELECT host_id, http_port, managed_cluster_id FROM sr_cluster_nodes WHERE http_port IS NOT NULL
UNION ALL SELECT host_id, query_port, managed_cluster_id FROM sr_cluster_nodes WHERE query_port IS NOT NULL
UNION ALL SELECT host_id, rpc_port, managed_cluster_id FROM sr_cluster_nodes WHERE rpc_port IS NOT NULL
UNION ALL SELECT host_id, brpc_port, managed_cluster_id FROM sr_cluster_nodes WHERE brpc_port IS NOT NULL
UNION ALL SELECT host_id, webserver_port, managed_cluster_id FROM sr_cluster_nodes WHERE webserver_port IS NOT NULL;

CREATE INDEX idx_sr_host_port_allocations_cluster
    ON sr_host_port_allocations(managed_cluster_id);
