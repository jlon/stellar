-- BE binds an implicit starlet grpc port (default 9070) in addition to its
-- heartbeat/thrift/http/brpc ports. Same-host multi-cluster deployments must
-- reserve and check it explicitly, or the second BE crashes on startup.
ALTER TABLE sr_cluster_nodes ADD COLUMN starlet_port INTEGER;

UPDATE sr_cluster_nodes SET starlet_port = 9070 WHERE role = 'be' AND starlet_port IS NULL;

INSERT OR IGNORE INTO sr_host_port_allocations (host_id, port, managed_cluster_id)
SELECT host_id, starlet_port, managed_cluster_id
FROM sr_cluster_nodes
WHERE role = 'be' AND starlet_port IS NOT NULL;
