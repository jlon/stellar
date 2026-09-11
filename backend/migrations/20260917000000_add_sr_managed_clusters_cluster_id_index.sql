-- 集群"自托管 vs 外部导入"判定依赖 sr_managed_clusters.cluster_id 反向关联
-- （clusters 表不新增冗余列，避免与部署模块双写漂移）。
-- 删除守卫与列表标注按 cluster_id 反查，必须走索引。
CREATE INDEX IF NOT EXISTS idx_sr_managed_clusters_cluster_id
    ON sr_managed_clusters(cluster_id);