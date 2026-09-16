use crate::db::query as db_query;

/// 磁盘水位阈值（告警口径）：与 overview_service::generate_alerts 的 80/90 分档一致。
/// 事件闭环（agent_runtime/collectors.rs）与概览告警共用同一常量，避免口径漂移。
pub const DISK_WARNING_PCT: f64 = 80.0;
pub const DISK_CRITICAL_PCT: f64 = 90.0;
// Metrics Collector Service
// Purpose: Periodically collect metrics from StarRocks clusters and store them in SQLite
// Design Ref: ARCHITECTURE_ANALYSIS_AND_INTEGRATION.md

use crate::db::AppDb;
use crate::db::SqlDialect;
use crate::db::dialect::RowsAffected;
use crate::models::{Backend, Cluster, Frontend, RuntimeInfo};
use crate::services::mysql_pool_manager::MySQLPoolManager;
use crate::services::{ClusterService, StarRocksClient};
use crate::utils::{ApiError, ApiResult, ScheduledTask};
use chrono::Utc;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use stellar_macros::app_impl;
use utoipa::ToSchema;

/// 审计日志延迟百分位查询代价高（percentile_approx 扫全窗口），
/// 而该指标是低频缓变值——30s 采集周期内复用缓存即可。
const LATENCY_PERCENTILES_CACHE_TTL: Duration = Duration::from_secs(600);

struct CachedNodeList<T> {
    nodes: Vec<T>,
    fetched_at: Instant,
}

/// Aggregated metrics from database queries
#[derive(Debug, sqlx::FromRow)]
struct MetricsAggregation {
    avg_qps: Option<f64>,
    max_qps: Option<f64>,
    min_qps: Option<f64>,
    avg_latency_p99: Option<f64>,
    max_latency_p99: Option<f64>,
    total_queries: Option<i64>,
    total_errors: Option<i64>,
    avg_cpu_usage: Option<f64>,
    max_cpu_usage: Option<f64>,
    avg_memory_usage: Option<f64>,
    max_memory_usage: Option<f64>,
    avg_disk_usage_pct: Option<f64>,
    max_disk_usage_pct: Option<f64>,
    max_disk_used_bytes: Option<f64>,
}

/// Metrics snapshot stored in database
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MetricsSnapshot {
    pub cluster_id: i64,
    pub collected_at: chrono::DateTime<Utc>,

    pub qps: f64,
    pub rps: f64,
    pub query_latency_p50: f64,
    pub query_latency_p95: f64,
    pub query_latency_p99: f64,
    pub query_total: i64,
    pub query_success: i64,
    pub query_error: i64,
    pub query_timeout: i64,

    pub backend_total: i32,
    pub backend_alive: i32,
    pub frontend_total: i32,
    pub frontend_alive: i32,

    pub total_cpu_usage: f64,
    pub avg_cpu_usage: f64,
    pub total_memory_usage: f64,
    pub avg_memory_usage: f64,
    pub disk_total_bytes: i64,
    pub disk_used_bytes: i64,
    pub disk_usage_pct: f64,

    pub tablet_count: i64,
    pub max_compaction_score: f64,

    pub txn_running: i32,
    pub txn_success_total: i64,
    pub txn_failed_total: i64,

    pub load_running: i32,
    pub load_finished_total: i64,

    pub jvm_heap_total: i64,
    pub jvm_heap_used: i64,
    pub jvm_heap_usage_pct: f64,
    pub jvm_thread_count: i32,

    pub network_bytes_sent_total: i64,
    pub network_bytes_received_total: i64,
    pub network_send_rate: f64,
    pub network_receive_rate: f64,

    pub io_read_bytes_total: i64,
    pub io_write_bytes_total: i64,
    pub io_read_rate: f64,
    pub io_write_rate: f64,

    pub meta_log_count: i64,
    pub unfinished_query: i64,
    pub safe_mode: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct ClusterResourceSummary {
    pub cluster_id: i64,
    pub cpu_usage_pct: f64,
    pub memory_usage_pct: f64,
    pub disk_usage_pct: Option<f64>,
}

#[derive(Clone)]
pub struct MetricsCollectorService<DB: AppDb> {
    db: Pool<DB>,
    cluster_service: Arc<ClusterService<DB>>,
    mysql_pool_manager: Arc<MySQLPoolManager>,
    retention_days: i64,
    backend_list_cache: Arc<DashMap<i64, CachedNodeList<Backend>>>,
    frontend_list_cache: Arc<DashMap<i64, CachedNodeList<Frontend>>>,
    /// (p50, p95, p99, fetched_at) per cluster；审计 percentile 查询代价高，低频刷新。
    latency_percentiles_cache: Arc<DashMap<i64, (f64, f64, f64, Instant)>>,
}

#[app_impl]
impl<DB: AppDb> MetricsCollectorService<DB> {
    /// Create a new MetricsCollectorService
    pub fn new(
        db: Pool<DB>,
        cluster_service: Arc<ClusterService<DB>>,
        mysql_pool_manager: Arc<MySQLPoolManager>,
        retention_days: i64,
    ) -> Self {
        Self {
            db,
            cluster_service,
            mysql_pool_manager,
            retention_days,
            backend_list_cache: Arc::new(DashMap::new()),
            frontend_list_cache: Arc::new(DashMap::new()),
            latency_percentiles_cache: Arc::new(DashMap::new()),
        }
    }

    pub fn cached_backends(&self, cluster_id: i64, max_age: Duration) -> Option<Vec<Backend>> {
        let entry = self.backend_list_cache.get(&cluster_id)?;
        if entry.fetched_at.elapsed() <= max_age {
            Some(entry.nodes.clone())
        } else {
            None
        }
    }

    pub fn stale_backends(&self, cluster_id: i64) -> Option<Vec<Backend>> {
        self.backend_list_cache
            .get(&cluster_id)
            .map(|entry| entry.nodes.clone())
    }

    pub fn store_backends(&self, cluster_id: i64, nodes: Vec<Backend>) {
        self.backend_list_cache
            .insert(cluster_id, CachedNodeList { nodes, fetched_at: Instant::now() });
    }

    pub fn cached_frontends(&self, cluster_id: i64, max_age: Duration) -> Option<Vec<Frontend>> {
        let entry = self.frontend_list_cache.get(&cluster_id)?;
        if entry.fetched_at.elapsed() <= max_age {
            Some(entry.nodes.clone())
        } else {
            None
        }
    }

    pub fn stale_frontends(&self, cluster_id: i64) -> Option<Vec<Frontend>> {
        self.frontend_list_cache
            .get(&cluster_id)
            .map(|entry| entry.nodes.clone())
    }

    pub fn store_frontends(&self, cluster_id: i64, nodes: Vec<Frontend>) {
        self.frontend_list_cache
            .insert(cluster_id, CachedNodeList { nodes, fetched_at: Instant::now() });
    }

    pub fn invalidate_backends(&self, cluster_id: i64) {
        self.backend_list_cache.remove(&cluster_id);
    }

    /// Execute one collection cycle
    /// This is called periodically by the ScheduledExecutor
    pub async fn collect_once(&self) -> Result<(), anyhow::Error> {
        self.collect_all_clusters().await?;

        self.check_and_run_daily_aggregation().await?;

        Ok(())
    }

    /// Check if daily aggregation is needed and run it
    async fn check_and_run_daily_aggregation(&self) -> Result<(), anyhow::Error> {
        let today = Utc::now().date_naive();
        let yesterday = today - chrono::Duration::days(1);

        let count: (i64,) = db_query::query_as(
            "SELECT COUNT(*) as count FROM daily_snapshots WHERE snapshot_date = ?",
        )
        .bind(yesterday)
        .fetch_one(&self.db)
        .await?;

        if count.0 == 0 {
            tracing::info!("Running daily aggregation for date: {}", yesterday);
            self.run_daily_aggregation_all_clusters().await?;
        }

        Ok(())
    }

    /// Collect metrics from all clusters
    async fn collect_all_clusters(&self) -> Result<(), anyhow::Error> {
        let clusters = self.cluster_service.list_clusters().await?;

        tracing::debug!("Collecting metrics from {} clusters", clusters.len());

        for cluster in clusters {
            if let Err(e) = self.collect_cluster_metrics(&cluster).await {
                tracing::error!(
                    "Failed to collect metrics for cluster {} ({}): {}",
                    cluster.id,
                    cluster.name,
                    e
                );
            }
        }

        if let Err(e) = self.cleanup_old_metrics().await {
            tracing::error!("Failed to cleanup old metrics: {}", e);
        }

        Ok(())
    }

    /// Collect metrics from a single cluster
    async fn collect_cluster_metrics(&self, cluster: &Cluster) -> ApiResult<()> {
        tracing::debug!("Collecting metrics for cluster: {} ({})", cluster.id, cluster.name);

        let client = StarRocksClient::new(cluster.clone(), self.mysql_pool_manager.clone());

        let mysql_timeout = Duration::from_secs(cluster.connection_timeout.max(1) as u64);
        let (metrics_result, backends_result, frontends_result, runtime_result) = tokio::join!(
            client.get_metrics(),
            tokio::time::timeout(mysql_timeout, client.get_backends()),
            tokio::time::timeout(mysql_timeout, client.get_frontends()),
            client.get_runtime_info(),
        );

        let backends = match backends_result {
            Ok(result) => result?,
            Err(_) => {
                return Err(ApiError::cluster_connection_failed(format!(
                    "Timed out collecting node list for cluster {}",
                    cluster.name
                )));
            },
        };
        if backends.is_empty() {
            // 节点列表为空（集群重启/切换中）：本轮数据不可靠，跳过快照，
            // 避免全零快照覆盖正常值导致 dashboard 显示 0%/空白。
            // 注意：空列表也不写入节点缓存，否则节点管理页会被清空。
            tracing::warn!(
                "No backends/compute-nodes returned for cluster {} ({}), skipping snapshot",
                cluster.id,
                cluster.name
            );
            return Ok(());
        }
        self.store_backends(cluster.id, backends.clone());
        let frontends = match frontends_result {
            Ok(Ok(value)) => {
                self.store_frontends(cluster.id, value.clone());
                value
            },
            Ok(Err(e)) => {
                tracing::warn!(
                    "Failed to collect frontends for cluster {} ({}): {}",
                    cluster.id,
                    cluster.name,
                    e
                );
                Vec::new()
            },
            Err(_) => {
                tracing::warn!(
                    "Timed out collecting frontends for cluster {} ({})",
                    cluster.id,
                    cluster.name
                );
                Vec::new()
            },
        };
        let metrics_text = match metrics_result {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!(
                    "Failed to collect HTTP metrics for cluster {} ({}): {}",
                    cluster.id,
                    cluster.name,
                    e
                );
                String::new()
            },
        };
        let runtime_info = match runtime_result {
            Ok(value) => value,
            Err(e) => {
                tracing::warn!(
                    "Failed to collect runtime info for cluster {} ({}): {}",
                    cluster.id,
                    cluster.name,
                    e
                );
                RuntimeInfo::default()
            },
        };

        let metrics_map = client.parse_prometheus_metrics(&metrics_text)?;

        let backend_total = backends.len() as i32;
        let backend_alive = backends.iter().filter(|b| b.alive == "true").count() as i32;

        let frontend_total = frontends.len() as i32;
        let frontend_alive = frontends.iter().filter(|f| f.alive == "true").count() as i32;

        let tablet_count: i64 = backends
            .iter()
            .filter_map(|b| b.tablet_num.parse::<i64>().ok())
            .sum();

        let cpu_values: Vec<f64> =
            backends.iter().filter_map(|b| parse_pct(&b.cpu_used_pct)).collect();

        let total_cpu_usage: f64 = cpu_values.iter().sum();

        let avg_cpu_usage = if !cpu_values.is_empty() {
            total_cpu_usage / cpu_values.len() as f64
        } else {
            0.0
        };

        tracing::debug!(
            "CPU parsing: parsed {}/{} compute nodes (BE/CN), total={}, avg={}",
            cpu_values.len(),
            backend_total,
            total_cpu_usage,
            avg_cpu_usage
        );

        let memory_values: Vec<f64> =
            backends.iter().filter_map(|b| parse_pct(&b.mem_used_pct)).collect();
        let total_memory_usage: f64 = memory_values.iter().sum();
        let avg_memory_usage = if !memory_values.is_empty() {
            total_memory_usage / memory_values.len() as f64
        } else {
            0.0
        };

        let capacity_samples: Vec<(f64, i64, i64)> = backends
            .iter()
            .filter_map(|b| {
                let total = parse_storage_size(&b.total_capacity)?;
                let pct = parse_pct(&b.max_disk_used_pct)?;
                let used = (total as f64 * pct / 100.0) as i64;
                Some((pct, total, used))
            })
            .collect();
        let cache_samples: Vec<(f64, i64, i64)> = backends
            .iter()
            .filter_map(|b| {
                let (used, total, pct) = parse_data_cache_disk(&b.data_cache_metrics)?;
                Some((pct, total, used))
            })
            .collect();
        let disk_samples = if capacity_samples.is_empty() { cache_samples } else { capacity_samples };
        let (disk_usage_pct, disk_used_bytes, disk_total_bytes) =
            cluster_disk_usage(&disk_samples);

        tracing::debug!(
            "Disk usage (cluster): {}% ({} / {} bytes, cluster mode: {})",
            disk_usage_pct,
            disk_used_bytes,
            disk_total_bytes,
            cluster.deployment_mode
        );

        let jvm_heap_used = runtime_info.total_mem - runtime_info.free_mem;
        let jvm_heap_usage_pct = if runtime_info.total_mem > 0 {
            (jvm_heap_used as f64 / runtime_info.total_mem as f64) * 100.0
        } else {
            0.0
        };

        let network_bytes_sent_total = metrics_map
            .get("starrocks_be_network_send_bytes")
            .copied()
            .unwrap_or(0.0) as i64;
        let network_bytes_received_total = metrics_map
            .get("starrocks_be_network_receive_bytes")
            .copied()
            .unwrap_or(0.0) as i64;
        let network_send_rate = metrics_map
            .get("starrocks_be_network_send_rate")
            .copied()
            .unwrap_or(0.0);
        let network_receive_rate = metrics_map
            .get("starrocks_be_network_receive_rate")
            .copied()
            .unwrap_or(0.0);

        let io_read_bytes_total = metrics_map
            .get("starrocks_be_disk_read_bytes")
            .copied()
            .unwrap_or(0.0) as i64;
        let io_write_bytes_total = metrics_map
            .get("starrocks_be_disk_write_bytes")
            .copied()
            .unwrap_or(0.0) as i64;
        let io_read_rate = metrics_map
            .get("starrocks_be_disk_read_rate")
            .copied()
            .unwrap_or(0.0);
        let io_write_rate = metrics_map
            .get("starrocks_be_disk_write_rate")
            .copied()
            .unwrap_or(0.0);

        let (real_p50, real_p95, real_p99) = self
            .get_real_latency_percentiles(cluster)
            .await
            .unwrap_or((0.0, 0.0, 0.0));

        let snapshot = MetricsSnapshot {
            cluster_id: cluster.id,
            collected_at: Utc::now(),

            qps: metrics_map.get("starrocks_fe_qps").copied().unwrap_or(0.0),
            rps: metrics_map.get("starrocks_fe_rps").copied().unwrap_or(0.0),
            query_latency_p50: if real_p50 > 0.0 {
                real_p50
            } else {
                metrics_map
                    .get("starrocks_fe_query_latency_p50")
                    .copied()
                    .unwrap_or(0.0)
            },
            query_latency_p95: if real_p95 > 0.0 {
                real_p95
            } else {
                metrics_map
                    .get("starrocks_fe_query_latency_p95")
                    .copied()
                    .unwrap_or(0.0)
            },
            query_latency_p99: if real_p99 > 0.0 {
                real_p99
            } else {
                metrics_map
                    .get("starrocks_fe_query_latency_p99")
                    .copied()
                    .unwrap_or(0.0)
            },
            query_total: metrics_map
                .get("starrocks_fe_query_total")
                .copied()
                .unwrap_or(0.0) as i64,
            query_success: metrics_map
                .get("starrocks_fe_query_success")
                .copied()
                .unwrap_or(0.0) as i64,
            query_error: metrics_map
                .get("starrocks_fe_query_err")
                .copied()
                .unwrap_or(0.0) as i64,
            query_timeout: metrics_map
                .get("starrocks_fe_query_timeout")
                .copied()
                .unwrap_or(0.0) as i64,

            backend_total,
            backend_alive,
            frontend_total,
            frontend_alive,

            total_cpu_usage,
            avg_cpu_usage,
            total_memory_usage,
            avg_memory_usage,
            disk_total_bytes,
            disk_used_bytes,
            disk_usage_pct,

            tablet_count,
            max_compaction_score: metrics_map
                .get("starrocks_fe_max_tablet_compaction_score")
                .copied()
                .unwrap_or(0.0),

            txn_running: 0,
            txn_success_total: metrics_map
                .get("starrocks_fe_txn_success")
                .copied()
                .unwrap_or(0.0) as i64,
            txn_failed_total: metrics_map
                .get("starrocks_fe_txn_failed")
                .copied()
                .unwrap_or(0.0) as i64,

            load_running: 0,
            load_finished_total: metrics_map
                .get("starrocks_fe_load_finished")
                .copied()
                .unwrap_or(0.0) as i64,

            jvm_heap_total: runtime_info.total_mem,
            jvm_heap_used,
            jvm_heap_usage_pct,
            jvm_thread_count: runtime_info.thread_cnt,

            network_bytes_sent_total,
            network_bytes_received_total,
            network_send_rate,
            network_receive_rate,

            io_read_bytes_total,
            io_write_bytes_total,
            io_read_rate,
            io_write_rate,

            meta_log_count: metrics_map
                .get("starrocks_fe_meta_log_count")
                .copied()
                .unwrap_or(0.0) as i64,
            unfinished_query: metrics_map
                .get("starrocks_fe_unfinished_query")
                .copied()
                .unwrap_or(0.0) as i64,
            safe_mode: if metrics_map.get("starrocks_fe_safe_mode").copied().unwrap_or(0.0) > 0.0 {
                1
            } else {
                0
            },
        };

        self.save_snapshot(&snapshot).await?;

        tracing::debug!(
            "Metrics collected for cluster {} ({}): QPS={:.2}, CPU={:.1}%, Disk={:.1}%",
            cluster.id,
            cluster.name,
            snapshot.qps,
            snapshot.avg_cpu_usage,
            snapshot.disk_usage_pct
        );

        Ok(())
    }

    /// Save metrics snapshot to database
    async fn save_snapshot(&self, snapshot: &MetricsSnapshot) -> ApiResult<()> {
        db_query::query(
            r#"
            INSERT INTO metrics_snapshots (
                cluster_id, collected_at,
                qps, rps, query_latency_p50, query_latency_p95, query_latency_p99,
                query_total, query_success, query_error, query_timeout,
                backend_total, backend_alive, frontend_total, frontend_alive,
                total_cpu_usage, avg_cpu_usage, total_memory_usage, avg_memory_usage,
                disk_total_bytes, disk_used_bytes, disk_usage_pct,
                tablet_count, max_compaction_score,
                txn_running, txn_success_total, txn_failed_total,
                load_running, load_finished_total,
                jvm_heap_total, jvm_heap_used, jvm_heap_usage_pct, jvm_thread_count,
                network_bytes_sent_total, network_bytes_received_total, network_send_rate, network_receive_rate,
                io_read_bytes_total, io_write_bytes_total, io_read_rate, io_write_rate,
                meta_log_count, unfinished_query, safe_mode,
                raw_metrics
            ) VALUES (
                ?, ?,
                ?, ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?,
                ?, ?,
                ?, ?, ?,
                ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?,
                ?
            )
            "#
        )
        .bind(snapshot.cluster_id)
        .bind(snapshot.collected_at)
        .bind(snapshot.qps)
        .bind(snapshot.rps)
        .bind(snapshot.query_latency_p50)
        .bind(snapshot.query_latency_p95)
        .bind(snapshot.query_latency_p99)
        .bind(snapshot.query_total)
        .bind(snapshot.query_success)
        .bind(snapshot.query_error)
        .bind(snapshot.query_timeout)
        .bind(snapshot.backend_total)
        .bind(snapshot.backend_alive)
        .bind(snapshot.frontend_total)
        .bind(snapshot.frontend_alive)
        .bind(snapshot.total_cpu_usage)
        .bind(snapshot.avg_cpu_usage)
        .bind(snapshot.total_memory_usage)
        .bind(snapshot.avg_memory_usage)
        .bind(snapshot.disk_total_bytes)
        .bind(snapshot.disk_used_bytes)
        .bind(snapshot.disk_usage_pct)
        .bind(snapshot.tablet_count)
        .bind(snapshot.max_compaction_score)
        .bind(snapshot.txn_running)
        .bind(snapshot.txn_success_total)
        .bind(snapshot.txn_failed_total)
        .bind(snapshot.load_running)
        .bind(snapshot.load_finished_total)
        .bind(snapshot.jvm_heap_total)
        .bind(snapshot.jvm_heap_used)
        .bind(snapshot.jvm_heap_usage_pct)
        .bind(snapshot.jvm_thread_count)
        .bind(snapshot.network_bytes_sent_total)
        .bind(snapshot.network_bytes_received_total)
        .bind(snapshot.network_send_rate)
        .bind(snapshot.network_receive_rate)
        .bind(snapshot.io_read_bytes_total)
        .bind(snapshot.io_write_bytes_total)
        .bind(snapshot.io_read_rate)
        .bind(snapshot.io_write_rate)
        .bind(snapshot.meta_log_count)
        .bind(snapshot.unfinished_query)
        .bind(snapshot.safe_mode)
        .bind(None::<String>)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Cleanup old metrics data based on retention policy
    async fn cleanup_old_metrics(&self) -> Result<(), sqlx::Error> {
        let cutoff_date = Utc::now() - chrono::Duration::days(self.retention_days);

        let result = db_query::query("DELETE FROM metrics_snapshots WHERE collected_at < ?")
            .bind(cutoff_date)
            .execute(&self.db)
            .await?;

        if result.rows_affected() > 0 {
            tracing::info!(
                "Cleaned up {} old metric snapshots (older than {} days)",
                result.rows_affected(),
                self.retention_days
            );
        }

        Ok(())
    }

    /// Get the latest snapshot for a cluster
    pub async fn get_latest_snapshot(&self, cluster_id: i64) -> ApiResult<Option<MetricsSnapshot>> {
        #[derive(sqlx::FromRow)]
        struct SnapshotRow {
            cluster_id: i64,
            collected_at: chrono::DateTime<Utc>,
            qps: f64,
            rps: f64,
            query_latency_p50: f64,
            query_latency_p95: f64,
            query_latency_p99: f64,
            query_total: i64,
            query_success: i64,
            query_error: i64,
            query_timeout: i64,
            backend_total: i64,
            backend_alive: i64,
            frontend_total: i64,
            frontend_alive: i64,
            total_cpu_usage: f64,
            avg_cpu_usage: f64,
            total_memory_usage: f64,
            avg_memory_usage: f64,
            disk_total_bytes: i64,
            disk_used_bytes: i64,
            disk_usage_pct: f64,
            tablet_count: i64,
            max_compaction_score: f64,
            txn_running: i64,
            txn_success_total: i64,
            txn_failed_total: i64,
            load_running: i64,
            load_finished_total: i64,
            jvm_heap_total: i64,
            jvm_heap_used: i64,
            jvm_heap_usage_pct: f64,
            jvm_thread_count: i64,
            network_bytes_sent_total: i64,
            network_bytes_received_total: i64,
            network_send_rate: f64,
            network_receive_rate: f64,
            io_read_bytes_total: i64,
            io_write_bytes_total: i64,
            io_read_rate: f64,
            io_write_rate: f64,
            meta_log_count: i64,
            unfinished_query: i64,
            safe_mode: i64,
        }

        let row: Option<SnapshotRow> = db_query::query_as(
            r#"
            SELECT * FROM metrics_snapshots
            WHERE cluster_id = ?
            ORDER BY collected_at DESC
            LIMIT 1
            "#,
        )
        .bind(cluster_id)
        .fetch_optional(&self.db)
        .await?;

        if let Some(r) = row {
            Ok(Some(MetricsSnapshot {
                cluster_id: r.cluster_id,
                collected_at: r.collected_at,
                qps: r.qps,
                rps: r.rps,
                query_latency_p50: r.query_latency_p50,
                query_latency_p95: r.query_latency_p95,
                query_latency_p99: r.query_latency_p99,
                query_total: r.query_total,
                query_success: r.query_success,
                query_error: r.query_error,
                query_timeout: r.query_timeout,
                backend_total: r.backend_total as i32,
                backend_alive: r.backend_alive as i32,
                frontend_total: r.frontend_total as i32,
                frontend_alive: r.frontend_alive as i32,
                total_cpu_usage: r.total_cpu_usage,
                avg_cpu_usage: r.avg_cpu_usage,
                total_memory_usage: r.total_memory_usage,
                avg_memory_usage: r.avg_memory_usage,
                disk_total_bytes: r.disk_total_bytes,
                disk_used_bytes: r.disk_used_bytes,
                disk_usage_pct: r.disk_usage_pct,
                tablet_count: r.tablet_count,
                max_compaction_score: r.max_compaction_score,
                txn_running: r.txn_running as i32,
                txn_success_total: r.txn_success_total,
                txn_failed_total: r.txn_failed_total,
                load_running: r.load_running as i32,
                load_finished_total: r.load_finished_total,
                jvm_heap_total: r.jvm_heap_total,
                jvm_heap_used: r.jvm_heap_used,
                jvm_heap_usage_pct: r.jvm_heap_usage_pct,
                jvm_thread_count: r.jvm_thread_count as i32,
                network_bytes_sent_total: r.network_bytes_sent_total,
                network_bytes_received_total: r.network_bytes_received_total,
                network_send_rate: r.network_send_rate,
                network_receive_rate: r.network_receive_rate,
                io_read_bytes_total: r.io_read_bytes_total,
                io_write_bytes_total: r.io_write_bytes_total,
                io_read_rate: r.io_read_rate,
                io_write_rate: r.io_write_rate,
                meta_log_count: r.meta_log_count,
                unfinished_query: r.unfinished_query,
                safe_mode: r.safe_mode as i32,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_latest_resource_summaries(
        &self,
        cluster_ids: &[i64],
    ) -> ApiResult<Vec<ClusterResourceSummary>> {
        let mut summaries = Vec::with_capacity(cluster_ids.len());
        for cluster_id in cluster_ids {
            let Some(snapshot) = self.get_latest_snapshot(*cluster_id).await? else {
                continue;
            };
            summaries.push(ClusterResourceSummary {
                cluster_id: snapshot.cluster_id,
                cpu_usage_pct: snapshot.avg_cpu_usage,
                memory_usage_pct: snapshot.avg_memory_usage,
                disk_usage_pct: if snapshot.disk_total_bytes > 0 {
                    Some(snapshot.disk_usage_pct)
                } else {
                    None
                },
            });
        }
        Ok(summaries)
    }
}

fn parse_pct(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().trim_end_matches('%').trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse().ok()
}

fn parse_storage_size(size_str: &str) -> Option<i64> {
    let s = size_str.trim();
    if s.is_empty() {
        return None;
    }
    let unit_at = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(unit_at);
    let value: f64 = num.trim().parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let multiplier = match unit.trim().to_ascii_uppercase().as_str() {
        "B" | "BYTES" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0_f64.powi(4),
        "PB" => 1024.0_f64.powi(5),
        _ => return None,
    };
    Some((value * multiplier) as i64)
}

fn parse_data_cache_disk(metrics: &str) -> Option<(i64, i64, f64)> {
    let rest = metrics.split("DiskUsage:").nth(1)?;
    let disk_part = rest.split("MemUsage:").next().unwrap_or(rest);
    let mut used_sum = 0_i64;
    let mut total_sum = 0_i64;
    let mut found = false;
    for token in disk_part.split(',') {
        let Some((used_s, total_s)) = token.split_once('/') else {
            continue;
        };
        let Some(used) = parse_storage_size(used_s) else {
            continue;
        };
        let Some(total) = parse_storage_size(total_s) else {
            continue;
        };
        if total <= 0 {
            continue;
        }
        used_sum += used;
        total_sum += total;
        found = true;
    }
    if !found || total_sum <= 0 {
        return None;
    }
    Some((used_sum, total_sum, used_sum as f64 / total_sum as f64 * 100.0))
}

fn cluster_disk_usage(samples: &[(f64, i64, i64)]) -> (f64, i64, i64) {
    let disk_total_bytes: i64 = samples.iter().map(|sample| sample.1).sum();
    let disk_used_bytes: i64 = samples.iter().map(|sample| sample.2).sum();
    let disk_usage_pct = if disk_total_bytes > 0 {
        disk_used_bytes as f64 / disk_total_bytes as f64 * 100.0
    } else {
        0.0
    };
    (disk_usage_pct, disk_used_bytes, disk_total_bytes)
}

fn format_storage_size(bytes: i64) -> String {
    let tb = 1024.0_f64.powi(4);
    let gb = 1024.0_f64.powi(3);
    let value = bytes as f64;
    if value >= tb {
        format!("{:.1} TB", value / tb)
    } else if value >= gb {
        format!("{:.1} GB", value / gb)
    } else {
        format!("{} B", bytes)
    }
}

pub(crate) fn fill_backend_cache_capacity(backend: &mut crate::models::Backend) {
    if !backend.data_used_capacity.trim().is_empty() || !backend.total_capacity.trim().is_empty() {
        return;
    }
    let Some((used, total, pct)) = parse_data_cache_disk(&backend.data_cache_metrics) else {
        return;
    };
    backend.data_used_capacity = format_storage_size(used);
    backend.total_capacity = format_storage_size(total);
    backend.used_pct = format!("{:.1}%", pct);
}

#[app_impl]
impl<DB: AppDb> MetricsCollectorService<DB> {
    /// Run daily aggregation for all clusters
    async fn run_daily_aggregation_all_clusters(&self) -> Result<(), anyhow::Error> {
        let clusters = self.cluster_service.list_clusters().await?;

        tracing::info!("Starting daily aggregation for {} clusters", clusters.len());

        for cluster in clusters {
            if let Err(e) = self.run_daily_aggregation_for_cluster(cluster.id).await {
                tracing::error!(
                    "Failed to run daily aggregation for cluster {} ({}): {}",
                    cluster.id,
                    cluster.name,
                    e
                );
            }
        }

        self.cleanup_old_daily_snapshots().await?;

        Ok(())
    }

    /// Run daily aggregation for a single cluster
    /// Aggregates yesterday's metrics_snapshots into daily_snapshots
    async fn run_daily_aggregation_for_cluster(&self, cluster_id: i64) -> ApiResult<()> {
        let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
        let yesterday_start = yesterday.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let yesterday_end = yesterday.and_hms_opt(23, 59, 59).unwrap().and_utc();

        tracing::debug!(
            "Aggregating metrics for cluster {} from {} to {}",
            cluster_id,
            yesterday_start,
            yesterday_end
        );

        let snapshots = db_query::query_as::<_, MetricsAggregation>(
            r#"
            SELECT
                AVG(qps) as avg_qps,
                MAX(qps) as max_qps,
                MIN(qps) as min_qps,
                AVG(query_latency_p99) as avg_latency_p99,
                MAX(query_latency_p99) as max_latency_p99,
                SUM(query_total) as total_queries,
                SUM(query_error) as total_errors,
                AVG(avg_cpu_usage) as avg_cpu_usage,
                MAX(avg_cpu_usage) as max_cpu_usage,
                AVG(avg_memory_usage) as avg_memory_usage,
                MAX(avg_memory_usage) as max_memory_usage,
                AVG(disk_usage_pct) as avg_disk_usage_pct,
                MAX(disk_usage_pct) as max_disk_usage_pct,
                CAST(MAX(disk_used_bytes) AS REAL) as max_disk_used_bytes
            FROM metrics_snapshots
            WHERE cluster_id = ?
                AND collected_at >= ?
                AND collected_at <= ?
            "#,
        )
        .bind(cluster_id)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .fetch_one(&self.db)
        .await?;

        let total_queries = snapshots.total_queries.unwrap_or(0);
        let total_errors = snapshots.total_errors.unwrap_or(0);
        let error_rate = if total_queries > 0 {
            (total_errors as f64 / total_queries as f64) * 100.0
        } else {
            0.0
        };

        let avg_qps = snapshots.avg_qps.unwrap_or(0.0);
        let max_qps = snapshots.max_qps.unwrap_or(0.0);
        let min_qps = snapshots.min_qps.unwrap_or(0.0);
        let avg_latency_p99 = snapshots.avg_latency_p99.unwrap_or(0.0);
        let max_latency_p99 = snapshots.max_latency_p99.unwrap_or(0.0);
        let avg_cpu_usage = snapshots.avg_cpu_usage.unwrap_or(0.0);
        let max_cpu_usage = snapshots.max_cpu_usage.unwrap_or(0.0);
        let avg_memory_usage = snapshots.avg_memory_usage.unwrap_or(0.0);
        let max_memory_usage = snapshots.max_memory_usage.unwrap_or(0.0);
        let avg_disk_usage_pct = snapshots.avg_disk_usage_pct.unwrap_or(0.0);
        let max_disk_usage_pct = snapshots.max_disk_usage_pct.unwrap_or(0.0);
        let data_size_end = snapshots.max_disk_used_bytes.unwrap_or(0.0) as i64;
        let data_growth_bytes = 0i64;

        let upsert_sql = format!(
            "INSERT INTO daily_snapshots (\
              cluster_id, snapshot_date,\
              avg_qps, max_qps, min_qps,\
              avg_latency_p99, max_latency_p99,\
              total_queries, total_errors, error_rate,\
              avg_cpu_usage, max_cpu_usage,\
              avg_memory_usage, max_memory_usage,\
              avg_disk_usage_pct, max_disk_usage_pct,\
              data_size_end, data_growth_bytes\
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {}",
            <DB as SqlDialect>::upsert_suffix(
                &["cluster_id", "snapshot_date"],
                &[
                    "avg_qps",
                    "max_qps",
                    "min_qps",
                    "avg_latency_p99",
                    "max_latency_p99",
                    "total_queries",
                    "total_errors",
                    "error_rate",
                    "avg_cpu_usage",
                    "max_cpu_usage",
                    "avg_memory_usage",
                    "max_memory_usage",
                    "avg_disk_usage_pct",
                    "max_disk_usage_pct",
                    "data_size_end",
                    "data_growth_bytes",
                ],
                &[],
            ),
        );

        db_query::query(&upsert_sql)
            .bind(cluster_id)
            .bind(yesterday)
            .bind(avg_qps)
            .bind(max_qps)
            .bind(min_qps)
            .bind(avg_latency_p99)
            .bind(max_latency_p99)
            .bind(total_queries)
            .bind(total_errors)
            .bind(error_rate)
            .bind(avg_cpu_usage)
            .bind(max_cpu_usage)
            .bind(avg_memory_usage)
            .bind(max_memory_usage)
            .bind(avg_disk_usage_pct)
            .bind(max_disk_usage_pct)
            .bind(data_size_end)
            .bind(data_growth_bytes)
            .execute(&self.db)
            .await?;

        tracing::info!(
            "Daily aggregation completed for cluster {} (date: {})",
            cluster_id,
            yesterday
        );

        Ok(())
    }

    /// Detect the available query time column name in audit log table
    /// Different StarRocks versions may use different column names
    async fn detect_query_time_column(
        &self,
        mysql_client: &crate::services::mysql_client::MySQLClient,
        cluster: &Cluster,
    ) -> Option<String> {
        use crate::models::cluster::ClusterType;

        let audit_table = match cluster.cluster_type {
            ClusterType::StarRocks => "starrocks_audit_db__.starrocks_audit_tbl__",
            ClusterType::Doris => "__internal_schema.audit_log",
        };

        let show_columns_query = format!("SHOW COLUMNS FROM {}", audit_table);

        match mysql_client.query_raw(&show_columns_query).await {
            Ok((columns, rows)) => {
                let field_idx = columns
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case("Field"))?;

                let possible_columns =
                    vec!["queryTime", "query_time", "duration", "QueryTime", "queryDuration"];

                for row in rows {
                    if let Some(col_name) = row.get(field_idx) {
                        let col_name_lower = col_name.to_lowercase();
                        for possible_col in &possible_columns {
                            if col_name_lower == possible_col.to_lowercase() {
                                tracing::debug!("Detected query time column: {}", col_name);
                                return Some(col_name.clone());
                            }
                        }
                    }
                }
            },
            Err(e) => {
                tracing::debug!("Failed to get column list, trying fallback method: {}", e);

                let possible_columns =
                    vec!["queryTime", "query_time", "duration", "QueryTime", "queryDuration"];

                for col_name in possible_columns {
                    let test_query = format!(
                        "SELECT COUNT(*) as cnt FROM {} WHERE {} > 0 LIMIT 1",
                        audit_table, col_name
                    );

                    if mysql_client.query(&test_query).await.is_ok() {
                        tracing::debug!("Detected query time column (fallback): {}", col_name);
                        return Some(col_name.to_string());
                    }
                }
            },
        }

        tracing::warn!(
            "Could not detect query time column in audit log table. Available columns may differ by StarRocks version."
        );
        None
    }

    /// Get real latency percentiles from audit logs using StarRocks percentile functions
    /// Reference: https://docs.starrocks.io/zh/docs/category/percentile/
    async fn get_real_latency_percentiles(&self, cluster: &Cluster) -> ApiResult<(f64, f64, f64)> {
        if let Some(entry) = self.latency_percentiles_cache.get(&cluster.id)
            && entry.3.elapsed() < LATENCY_PERCENTILES_CACHE_TTL
        {
            return Ok((entry.0, entry.1, entry.2));
        }

        use crate::services::mysql_client::MySQLClient;

        let pool = self.mysql_pool_manager.get_pool(cluster).await?;
        let mysql_client = MySQLClient::from_pool(pool);

        let query_time_col = match self.detect_query_time_column(&mysql_client, cluster).await {
            Some(col) => col,
            None => {
                tracing::debug!(
                    "Query time column not found, skipping audit log percentile calculation"
                );
                return Ok((0.0, 0.0, 0.0));
            },
        };

        use crate::models::cluster::ClusterType;

        let (audit_table, time_field, is_query_field) = match cluster.cluster_type {
            ClusterType::StarRocks => {
                ("starrocks_audit_db__.starrocks_audit_tbl__", "timestamp", "isQuery")
            },
            ClusterType::Doris => ("__internal_schema.audit_log", "time", "is_query"),
        };

        let query = format!(
            r#"
            SELECT 
                COALESCE(percentile_approx({}, 0.50), 0) as p50,
                COALESCE(percentile_approx({}, 0.95), 0) as p95,
                COALESCE(percentile_approx({}, 0.99), 0) as p99
            FROM {}
            WHERE {} >= DATE_SUB(NOW(), INTERVAL 1 HOUR)
                AND {} > 0
                AND state = 'EOF'
                AND {} = 1
        "#,
            query_time_col,
            query_time_col,
            query_time_col,
            audit_table,
            time_field,
            query_time_col,
            is_query_field
        );

        match mysql_client.query(&query).await {
            Ok(results) => {
                if let Some(row) = results.first() {
                    let p50 = row
                        .get("p50")
                        .and_then(|v| v.as_f64())
                        .or_else(|| row.get("p50").and_then(|v| v.as_i64()).map(|i| i as f64))
                        .or_else(|| {
                            row.get("p50")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                        .unwrap_or(0.0);

                    let p95 = row
                        .get("p95")
                        .and_then(|v| v.as_f64())
                        .or_else(|| row.get("p95").and_then(|v| v.as_i64()).map(|i| i as f64))
                        .or_else(|| {
                            row.get("p95")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                        .unwrap_or(0.0);

                    let p99 = row
                        .get("p99")
                        .and_then(|v| v.as_f64())
                        .or_else(|| row.get("p99").and_then(|v| v.as_i64()).map(|i| i as f64))
                        .or_else(|| {
                            row.get("p99")
                                .and_then(|v| v.as_str())
                                .and_then(|s| s.parse::<f64>().ok())
                        })
                        .unwrap_or(0.0);

                    tracing::debug!(
                        "Real latency percentiles from audit logs: P50={:.2}ms, P95={:.2}ms, P99={:.2}ms",
                        p50,
                        p95,
                        p99
                    );
                    self.latency_percentiles_cache
                        .insert(cluster.id, (p50, p95, p99, Instant::now()));

                    Ok((p50, p95, p99))
                } else {
                    tracing::debug!("No audit log data available for percentile calculation");
                    Ok((0.0, 0.0, 0.0))
                }
            },
            Err(e) => {
                tracing::warn!(
                    "Failed to query audit logs for percentiles: {}. Falling back to Prometheus metrics.",
                    e
                );
                Ok((0.0, 0.0, 0.0))
            },
        }
    }

    /// Cleanup old daily snapshots (keep 90 days)
    async fn cleanup_old_daily_snapshots(&self) -> Result<(), sqlx::Error> {
        let cutoff_date = Utc::now().date_naive() - chrono::Duration::days(90);

        let result = db_query::query("DELETE FROM daily_snapshots WHERE snapshot_date < ?")
            .bind(cutoff_date)
            .execute(&self.db)
            .await?;

        if result.rows_affected() > 0 {
            tracing::info!(
                "Cleaned up {} old daily snapshots (older than 90 days)",
                result.rows_affected()
            );
        }

        Ok(())
    }
}

// Implement ScheduledTask for MetricsCollectorService
#[app_impl]
impl<DB: AppDb> ScheduledTask for MetricsCollectorService<DB> {
    fn run(&self) -> Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + Send + '_>> {
        Box::pin(async move { self.collect_once().await })
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_data_cache_disk, parse_pct, parse_storage_size};
    use crate::models::{Backend, Frontend};

    #[test]
    fn parse_pct_accepts_space_before_percent() {
        assert_eq!(parse_pct("26.4 %"), Some(26.4));
        assert_eq!(parse_pct("0.5 %"), Some(0.5));
        assert_eq!(parse_pct("9.30 %"), Some(9.3));
        assert_eq!(parse_pct(""), None);
        assert_eq!(parse_pct("N/A"), None);
    }

    #[test]
    fn parse_storage_size_accepts_spaced_and_compact_units() {
        assert_eq!(parse_storage_size("1.5 TB"), Some((1.5 * 1024.0_f64.powi(4)) as i64));
        assert_eq!(parse_storage_size("6.1TB"), Some((6.1 * 1024.0_f64.powi(4)) as i64));
        assert_eq!(parse_storage_size("0"), None);
        assert_eq!(parse_storage_size(""), None);
    }

    #[test]
    fn parse_data_cache_disk_reads_used_and_total() {
        let parsed = parse_data_cache_disk(
            "Status: Normal, DiskUsage: 6.1TB/12.6TB, MemUsage: 0B/0B",
        );
        let (used, total, pct) = parsed.expect("disk usage should parse");
        assert!(used > 0);
        assert!(total > used);
        assert!((pct - (used as f64 / total as f64 * 100.0)).abs() < 0.0001);
        assert_eq!(parse_data_cache_disk("N/A"), None);
        let two_disks = parse_data_cache_disk(
            "Status: Normal, DiskUsage: 7.94TB/7.94TB, 0.79TB/7.94TB, MemUsage: 0B/0B",
        )
        .expect("two disks should sum");
        assert!(two_disks.2 > 50.0 && two_disks.2 < 60.0);
        let full_quota = parse_data_cache_disk(
            "Status: Normal, DiskUsage: 8.8TB/8.8TB, MemUsage: 0B/0B",
        )
        .expect("full cache quota should parse");
        assert!((full_quota.2 - 100.0).abs() < 0.0001);
    }

    #[test]
    fn cluster_disk_usage_is_weighted_not_hottest_node() {
        let tb = 1024.0_f64.powi(4);
        let samples = [
            (100.0, (8.8 * tb) as i64, (8.8 * tb) as i64),
            (50.0, (12.6 * tb) as i64, (6.3 * tb) as i64),
        ];
        let (pct, used, total) = super::cluster_disk_usage(&samples);
        assert!(pct > 70.0 && pct < 72.0, "pct={pct}");
        assert_eq!(used, samples[0].2 + samples[1].2);
        assert_eq!(total, samples[0].1 + samples[1].1);
    }

    #[test]
    fn fill_backend_cache_capacity_only_when_storage_empty() {
        let mut cn: Backend = serde_json::from_value(serde_json::json!({
            "ComputeNodeId": "1",
            "IP": "cn-0",
            "DataCacheMetrics": "Status: Normal, DiskUsage: 7.5TB/12.6TB, MemUsage: 0B/0B"
        }))
        .expect("cn");
        super::fill_backend_cache_capacity(&mut cn);
        assert_eq!(cn.data_used_capacity, "7.5 TB");
        assert_eq!(cn.total_capacity, "12.6 TB");
        assert_eq!(cn.used_pct, "59.5%");

        cn.data_used_capacity = "1.0 TB".to_string();
        cn.total_capacity = "2.0 TB".to_string();
        cn.used_pct = "50.0%".to_string();
        super::fill_backend_cache_capacity(&mut cn);
        assert_eq!(cn.data_used_capacity, "1.0 TB");
        assert_eq!(cn.used_pct, "50.0%");
    }

    #[test]
    fn backend_deserializes_compute_node_row() {
        let row = serde_json::json!({
            "ComputeNodeId": "322320",
            "IP": "cn-0",
            "Alive": "true",
            "CpuUsedPct": "26.4 %",
            "MemUsedPct": "9.30 %",
            "DataCacheMetrics": "Status: Normal, DiskUsage: 6.1TB/12.6TB, MemUsage: 0B/0B",
            "HasStoragePath": "true"
        });
        let backend: Backend = serde_json::from_value(row).expect("backend should deserialize");
        assert_eq!(backend.backend_id, "322320");
        assert_eq!(backend.cpu_used_pct, "26.4 %");
        assert_eq!(backend.mem_used_pct, "9.30 %");
        assert!(backend.total_capacity.is_empty());
    }

    #[tokio::test]
    async fn backend_list_cache_hits_then_invalidates() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("db");
        let mysql = std::sync::Arc::new(crate::services::MySQLPoolManager::new());
        let clusters = std::sync::Arc::new(crate::services::ClusterService::new(
            pool.clone(),
            mysql.clone(),
        ));
        let service = super::MetricsCollectorService::new(pool, clusters, mysql, 7);
        let node: Backend = serde_json::from_value(serde_json::json!({
            "ComputeNodeId": "1",
            "IP": "cn-0"
        }))
        .expect("node");
        service.store_backends(3, vec![node]);
        assert_eq!(
            service
                .cached_backends(3, std::time::Duration::from_secs(90))
                .expect("cache hit")
                .len(),
            1
        );
        service.invalidate_backends(3);
        assert!(service.cached_backends(3, std::time::Duration::from_secs(90)).is_none());
        assert!(service.stale_backends(3).is_none());
    }

    #[tokio::test]
    async fn backend_list_stale_cache_survives_ttl() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("db");
        let mysql = std::sync::Arc::new(crate::services::MySQLPoolManager::new());
        let clusters = std::sync::Arc::new(crate::services::ClusterService::new(
            pool.clone(),
            mysql.clone(),
        ));
        let service = super::MetricsCollectorService::new(pool, clusters, mysql, 7);
        let node: Backend = serde_json::from_value(serde_json::json!({
            "ComputeNodeId": "1",
            "IP": "cn-0"
        }))
        .expect("node");
        service.store_backends(3, vec![node]);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        assert!(
            service
                .cached_backends(3, std::time::Duration::from_millis(1))
                .is_none()
        );
        assert_eq!(service.stale_backends(3).expect("stale hit").len(), 1);
    }

    #[tokio::test]
    async fn frontend_list_cache_returns_stale_after_ttl() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("db");
        let mysql = std::sync::Arc::new(crate::services::MySQLPoolManager::new());
        let clusters = std::sync::Arc::new(crate::services::ClusterService::new(
            pool.clone(),
            mysql.clone(),
        ));
        let service = super::MetricsCollectorService::new(pool, clusters, mysql, 7);
        let node: Frontend = serde_json::from_value(serde_json::json!({
            "Name": "fe-0",
            "IP": "10.0.0.1",
            "EditLogPort": "9010",
            "HttpPort": "8030",
            "QueryPort": "9030",
            "RpcPort": "9020",
            "Role": "LEADER",
            "ClusterId": "1",
            "Join": "true",
            "Alive": "true",
            "ReplayedJournalId": "1",
            "LastHeartbeat": "2026-09-10",
            "ErrMsg": "",
            "Version": "3.5"
        }))
        .expect("frontend");
        service.store_frontends(3, vec![node]);
        assert_eq!(
            service
                .cached_frontends(3, std::time::Duration::from_secs(90))
                .expect("cache hit")
                .len(),
            1
        );
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        assert!(
            service
                .cached_frontends(3, std::time::Duration::from_millis(1))
                .is_none()
        );
        assert_eq!(service.stale_frontends(3).expect("stale hit").len(), 1);
    }
}
