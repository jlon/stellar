use crate::db::query as db_query;
// Overview Service
// Purpose: Provide aggregated cluster overview data (real-time + historical)
// Design Ref: ARCHITECTURE_ANALYSIS_AND_INTEGRATION.md

use crate::db::AppDb;
use crate::models::cluster::Cluster;
use crate::services::cluster_timeout;
use crate::services::metrics_collector_service::{DISK_CRITICAL_PCT, DISK_WARNING_PCT};
use crate::services::{ClusterService, DataStatistics, DataStatisticsService, MetricsSnapshot};
use crate::utils::{ApiError, ApiResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Pool;
use std::sync::Arc;
use stellar_macros::app_impl;
use utoipa::ToSchema;

/// Time range for querying historical data
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeRange {
    #[serde(rename = "1h")]
    Hours1,
    #[serde(rename = "6h")]
    Hours6,
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "3d")]
    Days3,
}

impl TimeRange {
    pub fn to_duration(&self) -> chrono::Duration {
        match self {
            TimeRange::Hours1 => chrono::Duration::hours(1),
            TimeRange::Hours6 => chrono::Duration::hours(6),
            TimeRange::Hours24 => chrono::Duration::hours(24),
            TimeRange::Days3 => chrono::Duration::days(3),
        }
    }

    pub fn start_time(&self) -> DateTime<Utc> {
        Utc::now() - self.to_duration()
    }

    pub fn end_time(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Cluster overview data
#[derive(Debug, Serialize, ToSchema)]
pub struct ClusterOverview {
    pub cluster_id: i64,
    pub cluster_name: String,
    pub timestamp: DateTime<Utc>,

    pub latest_snapshot: Option<MetricsSnapshot>,

    pub performance_trends: PerformanceTrends,
    pub resource_trends: ResourceTrends,

    pub statistics: AggregatedStatistics,
}

/// Performance trends over time
#[derive(Debug, Serialize, ToSchema)]
pub struct PerformanceTrends {
    pub qps: Vec<TimeSeriesPoint>,
    pub rps: Vec<TimeSeriesPoint>,
    pub latency_p50: Vec<TimeSeriesPoint>,
    pub latency_p95: Vec<TimeSeriesPoint>,
    pub latency_p99: Vec<TimeSeriesPoint>,
    pub error_rate: Vec<TimeSeriesPoint>,
    pub timeout_rate: Vec<TimeSeriesPoint>,
}

/// Resource trends over time
#[derive(Debug, Serialize, ToSchema)]
pub struct ResourceTrends {
    pub cpu_usage: Vec<TimeSeriesPoint>,
    pub memory_usage: Vec<TimeSeriesPoint>,
    pub disk_usage: Vec<TimeSeriesPoint>,
    pub jvm_heap_usage: Vec<TimeSeriesPoint>,
    pub network_tx: Vec<TimeSeriesPoint>,
    pub network_rx: Vec<TimeSeriesPoint>,
    pub io_read: Vec<TimeSeriesPoint>,
    pub io_write: Vec<TimeSeriesPoint>,
    pub compaction_score: Vec<TimeSeriesPoint>,
    pub backend_alive: Vec<TimeSeriesPoint>,
    pub frontend_alive: Vec<TimeSeriesPoint>,
    pub tablet_count: Vec<TimeSeriesPoint>,
    pub jvm_thread_count: Vec<TimeSeriesPoint>,
    pub txn_success: Vec<TimeSeriesPoint>,
    pub txn_failed: Vec<TimeSeriesPoint>,
}

/// Time series data point
#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct TimeSeriesPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// Capacity prediction result
#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct CapacityPrediction {
    pub disk_total_bytes: i64,
    pub disk_used_bytes: i64,
    pub disk_usage_pct: f64,
    pub daily_growth_bytes: i64,
    pub days_until_full: Option<i32>,
    pub predicted_full_date: Option<String>,
    pub growth_trend: String,      // "increasing", "stable", "decreasing"
    pub real_data_size_bytes: i64, // Real data size from information_schema (stored in object storage)
}

/// Aggregated statistics
#[derive(Debug, Serialize, ToSchema)]
pub struct AggregatedStatistics {
    pub avg_qps: f64,
    pub max_qps: f64,
    pub avg_latency_p99: f64,
    pub avg_cpu_usage: f64,
    pub avg_memory_usage: f64,
    pub avg_disk_usage: f64,
}

/// Health status card
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthCard {
    pub title: String,
    pub value: String,
    pub status: HealthStatus,
    pub description: String,
}

/// Health status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
}

impl HealthStatus {
    /// 严重级别排序：用于从多个维度中取最严重者，保证 status 与 score 同源
    fn rank(self) -> u8 {
        match self {
            HealthStatus::Healthy => 0,
            HealthStatus::Warning => 1,
            HealthStatus::Critical => 2,
        }
    }
}

// ---- 健康评分模型（满分 100，逐维度扣分） ----
// 阈值与告警共用，避免同一事实出现两套口径（历史 bug：横幅 Critical / 状态卡 Warning）。
const PENALTY_NODE_OFFLINE: f64 = 30.0;
const PENALTY_COMPACTION_CRITICAL: f64 = 20.0;
const PENALTY_COMPACTION_WARNING: f64 = 10.0;
const PENALTY_DISK_CRITICAL: f64 = 20.0;
const PENALTY_DISK_WARNING: f64 = 10.0;
const PENALTY_CPU_HIGH: f64 = 10.0;
/// Compaction score 分档
const COMPACTION_CRITICAL_SCORE: f64 = 100.0;
const COMPACTION_WARNING_SCORE: f64 = 50.0;
/// 计算节点平均 CPU 水位（告警与评分一致）
const CPU_HIGH_PCT: f64 = 80.0;
/// 任一维度 Critical 时的分数上限：复合分不得把 P0 事件粉饰成「还行」
const SCORE_CAP_ON_CRITICAL: f64 = 60.0;

/// 快照里 `disk_usage_pct` 的语义由部署模式决定，消费点必须按语义使用：
/// - shared-nothing：本地盘即数据存储，使用率代表容量风险；
/// - shared-data：数据在对象存储，本地盘只是 Data Cache 配额，写满是 LRU 淘汰的稳态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskMetricKind {
    DataStore,
    DataCache,
}

impl DiskMetricKind {
    /// 按部署模式判定快照磁盘指标的语义
    pub fn from_cluster(cluster: &Cluster) -> Self {
        if cluster.is_shared_data() { Self::DataCache } else { Self::DataStore }
    }

    /// 只有承载数据的磁盘，使用率高才构成容量事故
    fn tracks_capacity(self) -> bool {
        self == Self::DataStore
    }
}

/// 单节点磁盘压力判定：返回 (级别, 文案)。
///
/// 与"容量"判定分离——加权平均反映整体容量，最大值反映单点风险。
/// 平均正常但某节点将写满，是数据分布问题而不是容量问题，处置方式不同。
/// shared-data 的本地盘是 Data Cache 配额（写满为 LRU 稳态），不做该判定。
/// 文案同时给出最高节点与全集群平均，判断依据不隐藏。
pub(crate) fn node_disk_pressure(
    tracks_capacity: bool,
    max_pct: f64,
    avg_pct: f64,
) -> Option<(HealthStatus, String)> {
    if !tracks_capacity || max_pct <= 0.0 {
        return None;
    }
    // 均匀高水位属于整体容量问题，由"容量"判定负责，不重复报成单点风险。
    // 15 个百分点是启发式阈值（无权威依据），可按实际集群调优。
    const SKEW_GAP_PCT: f64 = 15.0;
    if max_pct - avg_pct < SKEW_GAP_PCT {
        return None;
    }
    if max_pct > DISK_CRITICAL_PCT {
        Some((
            HealthStatus::Critical,
            format!("有节点磁盘使用率 {max_pct:.1}%（全集群平均 {avg_pct:.1}%）"),
        ))
    } else if max_pct > DISK_WARNING_PCT {
        Some((
            HealthStatus::Warning,
            format!("有节点磁盘使用率偏高 {max_pct:.1}%（全集群平均 {avg_pct:.1}%）"),
        ))
    } else {
        None
    }
}

/// 健康维度累加器：每个维度只声明一次严重级与扣分，status 与 score 由同一份声明推导。
struct HealthAccumulator {
    score: f64,
    worst: HealthStatus,
    alerts: Vec<String>,
}

impl HealthAccumulator {
    fn new() -> Self {
        Self { score: 100.0, worst: HealthStatus::Healthy, alerts: Vec::new() }
    }

    fn record(&mut self, level: HealthStatus, penalty: f64, message: String) {
        self.score -= penalty;
        if level.rank() > self.worst.rank() {
            self.worst = level;
        }
        self.alerts.push(message);
    }

    /// Critical 维度封顶，其余按扣分结果（不为负）
    fn score(&self) -> f64 {
        let capped = if self.worst == HealthStatus::Critical {
            self.score.min(SCORE_CAP_ON_CRITICAL)
        } else {
            self.score
        };
        capped.max(0.0)
    }
}

/// Cluster health overview (Hero Card)
#[derive(Debug, Serialize, ToSchema)]
pub struct ClusterHealth {
    pub status: HealthStatus,
    pub score: f64,                // 0-100
    pub starrocks_version: String, // StarRocks version
    pub be_nodes_online: i32,
    pub be_nodes_total: i32,
    pub fe_nodes_online: i32,
    pub fe_nodes_total: i32,
    pub compaction_score: f64,
    pub alerts: Vec<String>,
}

/// Key performance indicators
#[derive(Debug, Serialize, ToSchema)]
pub struct KeyPerformanceIndicators {
    pub qps: f64,
    pub qps_trend: f64, // percentage change
    pub p99_latency_ms: f64,
    pub p99_latency_trend: f64,
    pub success_rate: f64,
    pub success_rate_trend: f64,
    pub error_rate: f64,
}

/// Resource metrics
#[derive(Debug, Serialize, ToSchema)]
pub struct ResourceMetrics {
    pub cpu_usage_pct: f64,
    pub cpu_trend: f64,
    pub memory_usage_pct: f64,
    pub memory_trend: f64,
    pub disk_usage_pct: f64,
    pub disk_trend: f64,
    pub compaction_score: f64,
    pub compaction_status: String, // "normal", "warning", "critical"
}

/// Materialized view statistics
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct MaterializedViewStats {
    pub total: i32,
    pub running: i32,
    pub success: i32,
    pub failed: i32,
    pub pending: i32,
}

/// Load job statistics
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct LoadJobStats {
    pub running: i32,
    pub pending: i32,
    pub finished: i32,
    pub failed: i32,
    pub cancelled: i32,
}

/// Transaction statistics
#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionStats {
    pub running: i32,
    pub committed: i32,
    pub aborted: i32,
}

/// Schema change statistics
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct SchemaChangeStats {
    pub running: i32,
    pub pending: i32,
    pub finished: i32,
    pub failed: i32,
    pub cancelled: i32,
}

/// Compaction statistics
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct CompactionStats {
    pub base_compaction_running: i32,
    pub cumulative_compaction_running: i32,
    pub max_score: f64,
    pub avg_score: f64,
    pub be_scores: Vec<BECompactionScore>,
}

/// BE compaction score
#[derive(Debug, Serialize, ToSchema)]
pub struct BECompactionScore {
    pub be_id: i64,
    pub be_host: String,
    pub score: f64,
}

/// Compaction detailed statistics (for storage-compute separation architecture)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDetailStats {
    pub top_partitions: Vec<TopPartitionByScore>,
    pub task_stats: CompactionTaskStats,
    pub duration_stats: CompactionDurationStats,
    #[serde(default)]
    pub node_disks: Vec<NodeDiskUsage>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NodeDiskUsage {
    pub host: String,
    pub used_pct: f64,
}

/// Top partition by compaction score
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopPartitionByScore {
    pub db_name: String,
    pub table_name: String,
    pub partition_name: String,
    pub max_score: f64,
    pub avg_score: f64,
    pub p50_score: f64,
}

/// Compaction task statistics
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompactionTaskStats {
    pub running_count: i32,
    pub finished_count: i32,
    pub total_count: i32,
}

/// Compaction duration statistics
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDurationStats {
    pub min_duration_ms: i64,
    pub max_duration_ms: i64,
    pub avg_duration_ms: i64,
}

/// Session statistics
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct SessionStats {
    pub active_users_1h: i32,
    pub active_users_24h: i32,
    pub current_connections: i32,
    pub running_queries: Vec<RunningQuery>,
}

/// Running query info
#[derive(Debug, Serialize, ToSchema)]
pub struct RunningQuery {
    pub query_id: String,
    pub user: String,
    pub database: String,
    pub start_time: String,
    pub duration_ms: i64,
    pub state: String,
    pub query_preview: String, // First 200 chars
}

/// Network and IO statistics
#[derive(Debug, Serialize, ToSchema)]
pub struct NetworkIOStats {
    pub network_tx_bytes_per_sec: f64,
    pub network_rx_bytes_per_sec: f64,
    pub disk_read_bytes_per_sec: f64,
    pub disk_write_bytes_per_sec: f64,
}

/// Alert information
#[derive(Debug, Serialize, ToSchema)]
pub struct Alert {
    pub level: AlertLevel,
    pub category: String,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub action: Option<String>, // Suggested action
}

/// Alert level
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AlertLevel {
    Critical,
    Warning,
    Info,
}

/// Extended cluster overview with all modules
#[derive(Debug, Serialize, ToSchema)]
pub struct ExtendedClusterOverview {
    pub cluster_id: i64,
    pub cluster_name: String,
    pub deployment_mode: crate::models::cluster::DeploymentMode,
    pub timestamp: DateTime<Utc>,

    pub health: ClusterHealth,

    pub kpi: KeyPerformanceIndicators,

    pub resources: ResourceMetrics,

    pub performance_trends: PerformanceTrends,
    pub resource_trends: ResourceTrends,

    pub data_stats: Option<DataStatistics>,

    pub mv_stats: MaterializedViewStats,

    pub load_jobs: LoadJobStats,

    pub transactions: TransactionStats,

    pub schema_changes: SchemaChangeStats,

    pub compaction: CompactionStats,

    pub sessions: SessionStats,

    pub network_io: NetworkIOStats,

    pub capacity: Option<CapacityPrediction>,

    pub alerts: Vec<Alert>,

    pub meta_log_count: i64,
    pub unfinished_query: i64,
    pub safe_mode: bool,
}

fn empty_compaction_detail_stats() -> CompactionDetailStats {
    CompactionDetailStats {
        top_partitions: Vec::new(),
        task_stats: CompactionTaskStats { running_count: 0, finished_count: 0, total_count: 0 },
        duration_stats: CompactionDurationStats {
            min_duration_ms: 0,
            max_duration_ms: 0,
            avg_duration_ms: 0,
        },
        node_disks: Vec::new(),
    }
}

fn counter_delta_vs_last_positive(values: impl IntoIterator<Item = i64>) -> Vec<f64> {
    let mut last_positive: Option<i64> = None;
    values
        .into_iter()
        .map(|current| {
            let delta = match last_positive {
                Some(previous) if current >= previous && previous > 0 => {
                    (current - previous) as f64
                },
                _ => 0.0,
            };
            if current > 0 {
                last_positive = Some(current);
            }
            delta
        })
        .collect()
}

fn parse_usage_pct(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().trim_end_matches('%').trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse()
        .ok()
        .filter(|value: &f64| value.is_finite() && *value >= 0.0)
}

#[derive(Clone)]
pub struct OverviewService<DB: AppDb> {
    db: Pool<DB>,
    cluster_service: Arc<ClusterService<DB>>,
    data_statistics_service: Option<Arc<DataStatisticsService<DB>>>,
    mysql_pool_manager: Arc<crate::services::mysql_pool_manager::MySQLPoolManager>,
}

#[app_impl]
impl<DB: AppDb> OverviewService<DB> {
    /// Create a new OverviewService
    pub fn new(
        db: Pool<DB>,
        cluster_service: Arc<ClusterService<DB>>,
        mysql_pool_manager: Arc<crate::services::mysql_pool_manager::MySQLPoolManager>,
    ) -> Self {
        Self { db, cluster_service, data_statistics_service: None, mysql_pool_manager }
    }

    /// Set data statistics service (optional dependency)
    pub fn with_data_statistics(mut self, service: Arc<DataStatisticsService<DB>>) -> Self {
        self.data_statistics_service = Some(service);
        self
    }

    /// Get cluster overview (main API)
    pub async fn get_cluster_overview(
        &self,
        cluster_id: i64,
        time_range: TimeRange,
    ) -> ApiResult<ClusterOverview> {
        tracing::debug!(
            "Getting overview for cluster {} with time range {:?}",
            cluster_id,
            time_range
        );

        let cluster = self.cluster_service.get_cluster(cluster_id).await?;

        let latest_snapshot = self.get_latest_snapshot(cluster_id).await?;

        let history = self.get_history_snapshots(cluster_id, &time_range).await?;

        let performance_trends = self.calculate_performance_trends(&history);
        let resource_trends = self.calculate_resource_trends(&history);
        let statistics = self.calculate_aggregated_statistics(&history);

        Ok(ClusterOverview {
            cluster_id,
            cluster_name: cluster.name,
            timestamp: Utc::now(),
            latest_snapshot,
            performance_trends,
            resource_trends,
            statistics,
        })
    }

    /// Get health status cards
    pub async fn get_health_cards(&self, cluster_id: i64) -> ApiResult<Vec<HealthCard>> {
        let snapshot = self.get_latest_snapshot(cluster_id).await?;
        let disk =
            DiskMetricKind::from_cluster(&self.cluster_service.get_cluster(cluster_id).await?);

        let snapshot = match snapshot {
            Some(s) => s,
            None => {
                return Ok(vec![HealthCard {
                    title: "No Data".to_string(),
                    value: "N/A".to_string(),
                    status: HealthStatus::Warning,
                    description: "No metrics data available yet".to_string(),
                }]);
            },
        };

        let mut cards = Vec::new();

        let cluster_status = if snapshot.backend_alive == snapshot.backend_total
            && snapshot.frontend_alive == snapshot.frontend_total
        {
            HealthStatus::Healthy
        } else if snapshot.backend_alive > 0 && snapshot.frontend_alive > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Critical
        };

        cards.push(HealthCard {
            title: "Cluster Status".to_string(),
            value: format!(
                "{}/{} BE/CN, {}/{} FE",
                snapshot.backend_alive,
                snapshot.backend_total,
                snapshot.frontend_alive,
                snapshot.frontend_total
            ),
            status: cluster_status,
            description: "Compute nodes (BE/CN) and Frontend nodes availability".to_string(),
        });

        let qps_status = if snapshot.qps < 100.0 {
            HealthStatus::Healthy
        } else if snapshot.qps < 1000.0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Critical
        };

        cards.push(HealthCard {
            title: "Query Load".to_string(),
            value: format!("{:.1} QPS", snapshot.qps),
            status: qps_status,
            description: "Current queries per second".to_string(),
        });

        let cpu_status = if snapshot.avg_cpu_usage < 70.0 {
            HealthStatus::Healthy
        } else if snapshot.avg_cpu_usage < 85.0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Critical
        };

        cards.push(HealthCard {
            title: "CPU Usage".to_string(),
            value: format!("{:.1}%", snapshot.avg_cpu_usage),
            status: cpu_status,
            description: "Average CPU usage across all compute nodes".to_string(),
        });

        // shared-data 集群没有「数据盘」：本地盘是数据缓存配额，写满不构成容量风险
        let (disk_title, disk_description, disk_status) = if disk.tracks_capacity() {
            let status = if snapshot.disk_usage_pct > DISK_CRITICAL_PCT {
                HealthStatus::Critical
            } else if snapshot.disk_usage_pct > DISK_WARNING_PCT {
                HealthStatus::Warning
            } else {
                HealthStatus::Healthy
            };
            ("Disk Usage", "Total disk space usage", status)
        } else {
            (
                "Data Cache Quota",
                "Local data cache quota usage (evicted by LRU when full)",
                HealthStatus::Healthy,
            )
        };

        cards.push(HealthCard {
            title: disk_title.to_string(),
            value: format!("{:.1}%", snapshot.disk_usage_pct),
            status: disk_status,
            description: disk_description.to_string(),
        });

        Ok(cards)
    }

    /// Get performance trends
    pub async fn get_performance_trends(
        &self,
        cluster_id: i64,
        time_range: TimeRange,
    ) -> ApiResult<PerformanceTrends> {
        let history = self.get_history_snapshots(cluster_id, &time_range).await?;
        Ok(self.calculate_performance_trends(&history))
    }

    /// Get resource trends
    pub async fn get_resource_trends(
        &self,
        cluster_id: i64,
        time_range: TimeRange,
    ) -> ApiResult<ResourceTrends> {
        let history = self.get_history_snapshots(cluster_id, &time_range).await?;
        Ok(self.calculate_resource_trends(&history))
    }

    /// Get data statistics (database/table counts, top tables, etc.)
    pub async fn get_data_statistics(
        &self,
        cluster_id: i64,
        time_range: Option<&TimeRange>,
    ) -> ApiResult<DataStatistics> {
        if let Some(ref service) = self.data_statistics_service {
            if let Some(stats) = service.get_statistics(cluster_id).await? {
                let age = Utc::now() - stats.updated_at;
                if age.num_minutes() < 10 {
                    return Ok(stats);
                }

                // stale-while-revalidate：缓存过期时返回稍旧数据，后台刷新，
                // 避免前端请求被 StarRocks 全量采集（秒级）阻塞。
                // 已知权衡：两次快速连续的刷新可能乱序提交，最后一次胜出由 DB 层 upsert 兑底。
                let background = Arc::clone(service);
                tokio::spawn(async move {
                    if let Err(e) = background.update_statistics(cluster_id, None).await {
                        tracing::warn!("Background data statistics refresh failed: {}", e);
                    }
                });
                return Ok(stats);
            }

            // 首次无缓存：同步采集
            let time_range_start = time_range.map(|tr| tr.start_time());
            service
                .update_statistics(cluster_id, time_range_start)
                .await
        } else {
            Err(ApiError::internal_error("Data statistics service not configured"))
        }
    }

    /// Predict disk capacity
    ///
    /// Uses linear regression on historical disk usage data to predict when disk will be full
    pub async fn predict_capacity(
        &self,
        cluster_id: i64,
        disk: DiskMetricKind,
    ) -> ApiResult<CapacityPrediction> {
        let cutoff = Utc::now() - chrono::Duration::hours(2);

        let snapshots: Vec<(i64, i64, f64, DateTime<Utc>)> = db_query::query_as(
            r#"
            SELECT 
                disk_total_bytes,
                disk_used_bytes,
                disk_usage_pct,
                collected_at
            FROM metrics_snapshots
            WHERE cluster_id = ? AND collected_at >= ?
            ORDER BY collected_at ASC
            "#,
        )
        .bind(cluster_id)
        .bind(cutoff)
        .fetch_all(&self.db)
        .await?;

        if snapshots.is_empty() {
            return Err(ApiError::internal_error(
                "No historical data available for capacity prediction",
            ));
        }

        let latest = snapshots.last().ok_or_else(|| {
            ApiError::internal_error("No historical data available for capacity prediction")
        })?;
        let disk_total_bytes = latest.0;
        let disk_usage_pct = latest.2;

        let disk_used_bytes = ((disk_total_bytes as f64) * disk_usage_pct / 100.0) as i64;

        let first_time = snapshots
            .first()
            .ok_or_else(|| {
                ApiError::internal_error("No historical data available for capacity prediction")
            })?
            .3
            .timestamp();
        let last_time = latest.3.timestamp();
        let time_span_days = (last_time - first_time) as f64 / 86400.0;

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;
        let n = snapshots.len() as f64;

        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for snapshot in &snapshots {
            let x = (snapshot.3.timestamp() - first_time) as f64 / 86400.0;

            let y = (snapshot.0 as f64) * snapshot.2 / 100.0;

            min_y = min_y.min(y);
            max_y = max_y.max(y);

            sum_x += x;
            sum_y += y;
            sum_xy += x * y;
            sum_x2 += x * x;
        }

        let denominator = n * sum_x2 - sum_x * sum_x;
        let daily_growth_bytes = if denominator.abs() < 1e-10 {
            0
        } else {
            let slope = (n * sum_xy - sum_x * sum_y) / denominator;

            let data_variance = max_y - min_y;
            let slope_abs = slope.abs();

            if data_variance < 1_000_000_000.0
                || slope_abs > 10_000_000_000_000.0
                || time_span_days < 0.01
            {
                tracing::debug!(
                    "Linear regression unstable: variance={:.0}, slope={:.0}, time_span={:.3} days. Setting growth to 0.",
                    data_variance,
                    slope,
                    time_span_days
                );
                0
            } else {
                slope as i64
            }
        };

        let growth_trend = if daily_growth_bytes > 1_000_000_000 {
            "increasing"
        } else if daily_growth_bytes > 0 {
            "stable"
        } else {
            "decreasing"
        };

        let remaining_bytes = disk_total_bytes.saturating_sub(disk_used_bytes);
        // 数据缓存配额没有「写满即事故」语义：缓存会 LRU 淘汰，用量也会上下浮动，
        // 拿缓存算「距存满」只会得出「0 天」这类假预警。shared-data 集群不产出该结论。
        let (days_until_full, predicted_full_date) = if !disk.tracks_capacity() {
            (None, None)
        } else if disk_usage_pct >= 99.5 || remaining_bytes == 0 {
            (Some(0), Some(Utc::now().format("%Y-%m-%d").to_string()))
        } else if daily_growth_bytes > 0 {
            let days = (remaining_bytes as f64 / daily_growth_bytes as f64).ceil() as i32;
            let full_date = Utc::now() + chrono::Duration::days(days as i64);
            (Some(days), Some(full_date.format("%Y-%m-%d").to_string()))
        } else {
            (None, None)
        };

        let daily_growth_bytes = if disk.tracks_capacity() { daily_growth_bytes } else { 0 };

        Ok(CapacityPrediction {
            disk_total_bytes,
            disk_used_bytes,
            disk_usage_pct,
            daily_growth_bytes,
            days_until_full,
            predicted_full_date,
            growth_trend: growth_trend.to_string(),
            real_data_size_bytes: 0,
        })
    }

    /// Get the latest snapshot for a cluster
    async fn get_latest_snapshot(&self, cluster_id: i64) -> ApiResult<Option<MetricsSnapshot>> {
        #[derive(sqlx::FromRow)]
        struct SnapshotRow {
            cluster_id: i64,
            collected_at: DateTime<Utc>,
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
            max_disk_usage_pct: f64,
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
                max_disk_usage_pct: r.max_disk_usage_pct,
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

    /// Get historical snapshots for a time range
    async fn get_history_snapshots(
        &self,
        cluster_id: i64,
        time_range: &TimeRange,
    ) -> ApiResult<Vec<MetricsSnapshot>> {
        #[derive(sqlx::FromRow)]
        struct SnapshotRow {
            cluster_id: i64,
            collected_at: DateTime<Utc>,
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
            max_disk_usage_pct: f64,
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

        let start_time = time_range.start_time();
        let end_time = time_range.end_time();

        let rows: Vec<SnapshotRow> = db_query::query_as(
            r#"
            SELECT * FROM metrics_snapshots
            WHERE cluster_id = ? 
              AND collected_at BETWEEN ? AND ?
            ORDER BY collected_at ASC
            "#,
        )
        .bind(cluster_id)
        .bind(start_time)
        .bind(end_time)
        .fetch_all(&self.db)
        .await?;

        let snapshots = rows
            .into_iter()
            .map(|r| MetricsSnapshot {
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
                max_disk_usage_pct: r.max_disk_usage_pct,
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
            })
            .collect();

        Ok(snapshots)
    }

    fn snapshot_points(
        snapshots: &[MetricsSnapshot],
        value: impl Fn(&MetricsSnapshot) -> f64,
    ) -> Vec<TimeSeriesPoint> {
        snapshots
            .iter()
            .map(|snapshot| TimeSeriesPoint {
                timestamp: snapshot.collected_at,
                value: value(snapshot),
            })
            .collect()
    }

    fn snapshot_ratio_pct(curr_part: i64, curr_total: i64, prev: Option<(i64, i64)>) -> f64 {
        if let Some((prev_part, prev_total)) = prev {
            let delta_total = curr_total.saturating_sub(prev_total);
            let delta_part = curr_part.saturating_sub(prev_part);
            if delta_total > 0 {
                return (delta_part as f64 / delta_total as f64) * 100.0;
            }
        }
        if curr_total > 0 { curr_part as f64 / curr_total as f64 * 100.0 } else { 0.0 }
    }

    fn snapshot_delta_vs_last_positive(
        snapshots: &[MetricsSnapshot],
        value: impl Fn(&MetricsSnapshot) -> i64,
    ) -> Vec<TimeSeriesPoint> {
        let deltas = counter_delta_vs_last_positive(snapshots.iter().map(&value));
        snapshots
            .iter()
            .zip(deltas)
            .map(|(snapshot, value)| TimeSeriesPoint { timestamp: snapshot.collected_at, value })
            .collect()
    }

    fn snapshot_ratio_series(
        snapshots: &[MetricsSnapshot],
        part: impl Fn(&MetricsSnapshot) -> i64,
        total: impl Fn(&MetricsSnapshot) -> i64,
    ) -> Vec<TimeSeriesPoint> {
        snapshots
            .iter()
            .enumerate()
            .map(|(index, snapshot)| TimeSeriesPoint {
                timestamp: snapshot.collected_at,
                value: Self::snapshot_ratio_pct(
                    part(snapshot),
                    total(snapshot),
                    index
                        .checked_sub(1)
                        .and_then(|i| snapshots.get(i))
                        .map(|prev| (part(prev), total(prev))),
                ),
            })
            .collect()
    }

    fn calculate_performance_trends(&self, snapshots: &[MetricsSnapshot]) -> PerformanceTrends {
        PerformanceTrends {
            qps: Self::snapshot_points(snapshots, |s| s.qps),
            rps: Self::snapshot_points(snapshots, |s| s.rps),
            latency_p50: Self::snapshot_points(snapshots, |s| s.query_latency_p50),
            latency_p95: Self::snapshot_points(snapshots, |s| s.query_latency_p95),
            latency_p99: Self::snapshot_points(snapshots, |s| s.query_latency_p99),
            error_rate: Self::snapshot_ratio_series(
                snapshots,
                |s| s.query_error,
                |s| s.query_total,
            ),
            timeout_rate: Self::snapshot_ratio_series(
                snapshots,
                |s| s.query_timeout,
                |s| s.query_total,
            ),
        }
    }

    fn calculate_resource_trends(&self, snapshots: &[MetricsSnapshot]) -> ResourceTrends {
        ResourceTrends {
            cpu_usage: Self::snapshot_points(snapshots, |s| s.avg_cpu_usage),
            memory_usage: Self::snapshot_points(snapshots, |s| s.avg_memory_usage),
            disk_usage: Self::snapshot_points(snapshots, |s| s.disk_usage_pct),
            jvm_heap_usage: Self::snapshot_points(snapshots, |s| s.jvm_heap_usage_pct),
            network_tx: Self::snapshot_points(snapshots, |s| s.network_send_rate),
            network_rx: Self::snapshot_points(snapshots, |s| s.network_receive_rate),
            io_read: Self::snapshot_points(snapshots, |s| s.io_read_rate),
            io_write: Self::snapshot_points(snapshots, |s| s.io_write_rate),
            compaction_score: Self::snapshot_points(snapshots, |s| s.max_compaction_score),
            backend_alive: Self::snapshot_points(snapshots, |s| f64::from(s.backend_alive)),
            frontend_alive: Self::snapshot_points(snapshots, |s| f64::from(s.frontend_alive)),
            tablet_count: Self::snapshot_points(snapshots, |s| s.tablet_count as f64),
            jvm_thread_count: Self::snapshot_points(snapshots, |s| f64::from(s.jvm_thread_count)),
            txn_success: Self::snapshot_delta_vs_last_positive(snapshots, |s| s.txn_success_total),
            txn_failed: Self::snapshot_delta_vs_last_positive(snapshots, |s| s.txn_failed_total),
        }
    }

    /// Calculate aggregated statistics from snapshots
    fn calculate_aggregated_statistics(
        &self,
        snapshots: &[MetricsSnapshot],
    ) -> AggregatedStatistics {
        if snapshots.is_empty() {
            return AggregatedStatistics {
                avg_qps: 0.0,
                max_qps: 0.0,
                avg_latency_p99: 0.0,
                avg_cpu_usage: 0.0,
                avg_memory_usage: 0.0,
                avg_disk_usage: 0.0,
            };
        }

        let count = snapshots.len() as f64;

        let avg_qps = snapshots.iter().map(|s| s.qps).sum::<f64>() / count;
        let max_qps = snapshots.iter().map(|s| s.qps).fold(0.0, f64::max);
        let avg_latency_p99 = snapshots.iter().map(|s| s.query_latency_p99).sum::<f64>() / count;
        let avg_cpu_usage = snapshots.iter().map(|s| s.avg_cpu_usage).sum::<f64>() / count;
        let avg_memory_usage = snapshots.iter().map(|s| s.avg_memory_usage).sum::<f64>() / count;
        let avg_disk_usage = snapshots.iter().map(|s| s.disk_usage_pct).sum::<f64>() / count;

        AggregatedStatistics {
            avg_qps,
            max_qps,
            avg_latency_p99,
            avg_cpu_usage,
            avg_memory_usage,
            avg_disk_usage,
        }
    }

    /// Get extended cluster overview with all 18 modules
    pub async fn get_extended_overview(
        &self,
        cluster_id: i64,
        time_range: TimeRange,
    ) -> ApiResult<ExtendedClusterOverview> {
        let cluster = self.cluster_service.get_cluster(cluster_id).await?;
        let disk = DiskMetricKind::from_cluster(&cluster);

        let (latest, snapshots) = tokio::try_join!(
            self.get_latest_snapshot(cluster_id),
            self.get_history_snapshots(cluster_id, &time_range)
        )?;

        let capacity = match self.predict_capacity(cluster_id, disk).await {
            Ok(cap) => Some(cap),
            Err(e) => {
                tracing::debug!("Capacity prediction skipped for cluster {}: {}", cluster_id, e);
                None
            },
        };

        let health = latest
            .as_ref()
            .map(|s| Self::cluster_health_from_snapshot(s, disk))
            .unwrap_or_else(Self::empty_cluster_health);
        let kpi = self.calculate_kpi(&latest, &snapshots);
        let resources = self.calculate_resource_metrics(&latest, &snapshots);
        let performance_trends = self.calculate_performance_trends(&snapshots);
        let resource_trends = self.calculate_resource_trends(&snapshots);
        let transactions = self.get_transaction_stats(&latest);
        let network_io = self.calculate_network_io_stats(&latest);
        let compaction = Self::compaction_from_snapshot(&latest);
        let load_jobs = LoadJobStats {
            running: latest.as_ref().map(|s| s.load_running).unwrap_or(0),
            finished: latest
                .as_ref()
                .map(|s| s.load_finished_total as i32)
                .unwrap_or(0),
            ..LoadJobStats::default()
        };
        let alerts = self.generate_alerts(&health, &resources, &latest, disk);

        Ok(ExtendedClusterOverview {
            cluster_id,
            cluster_name: cluster.name,
            deployment_mode: cluster.deployment_mode,
            timestamp: Utc::now(),
            health,
            kpi,
            resources,
            performance_trends,
            resource_trends,
            data_stats: None,
            mv_stats: MaterializedViewStats::default(),
            load_jobs,
            transactions,
            schema_changes: SchemaChangeStats::default(),
            compaction,
            sessions: SessionStats::default(),
            network_io,
            capacity,
            alerts,
            meta_log_count: latest.as_ref().map(|s| s.meta_log_count).unwrap_or(0),
            unfinished_query: latest.as_ref().map(|s| s.unfinished_query).unwrap_or(0),
            safe_mode: latest.as_ref().is_some_and(|s| s.safe_mode > 0),
        })
    }

    fn empty_cluster_health() -> ClusterHealth {
        ClusterHealth {
            status: HealthStatus::Warning,
            score: 0.0,
            starrocks_version: "Unknown".to_string(),
            be_nodes_online: 0,
            be_nodes_total: 0,
            fe_nodes_online: 0,
            fe_nodes_total: 0,
            compaction_score: 0.0,
            alerts: vec!["暂无采集数据".to_string()],
        }
    }

    fn cluster_health_from_snapshot(
        snapshot: &MetricsSnapshot,
        disk: DiskMetricKind,
    ) -> ClusterHealth {
        let be_nodes_online = snapshot.backend_alive;
        let be_nodes_total = snapshot.backend_total;
        let fe_nodes_online = snapshot.frontend_alive;
        let fe_nodes_total = snapshot.frontend_total;
        let compaction_score = snapshot.max_compaction_score;

        // 一个节点都没采到：与「无快照」同一口径，不得报 100 分健康
        if be_nodes_total == 0 && fe_nodes_total == 0 {
            return Self::empty_cluster_health();
        }

        let mut health = HealthAccumulator::new();

        if be_nodes_online < be_nodes_total {
            health.record(
                HealthStatus::Critical,
                PENALTY_NODE_OFFLINE,
                format!("{} 计算节点离线", be_nodes_total - be_nodes_online),
            );
        }
        if fe_nodes_online < fe_nodes_total {
            health.record(
                HealthStatus::Critical,
                PENALTY_NODE_OFFLINE,
                format!("{} FE 节点离线", fe_nodes_total - fe_nodes_online),
            );
        }
        if compaction_score > COMPACTION_CRITICAL_SCORE {
            health.record(
                HealthStatus::Critical,
                PENALTY_COMPACTION_CRITICAL,
                format!("Compaction Score过高: {:.1}", compaction_score),
            );
        } else if compaction_score > COMPACTION_WARNING_SCORE {
            health.record(
                HealthStatus::Warning,
                PENALTY_COMPACTION_WARNING,
                format!("Compaction Score偏高: {:.1}", compaction_score),
            );
        }
        // 数据缓存配额写满是缓存常态，不构成容量事故（见 DiskMetricKind）
        if disk.tracks_capacity() {
            if snapshot.disk_usage_pct > DISK_CRITICAL_PCT {
                health.record(
                    HealthStatus::Critical,
                    PENALTY_DISK_CRITICAL,
                    format!("磁盘使用率过高: {:.1}%", snapshot.disk_usage_pct),
                );
            } else if snapshot.disk_usage_pct > DISK_WARNING_PCT {
                health.record(
                    HealthStatus::Warning,
                    PENALTY_DISK_WARNING,
                    format!("磁盘使用率偏高: {:.1}%", snapshot.disk_usage_pct),
                );
            }

            // 单节点压力：与整体容量分离，避免"平均正常但某节点将写满"被漏报
            if let Some((level, message)) =
                node_disk_pressure(true, snapshot.max_disk_usage_pct, snapshot.disk_usage_pct)
            {
                let penalty = if level == HealthStatus::Critical {
                    PENALTY_DISK_CRITICAL
                } else {
                    PENALTY_DISK_WARNING
                };
                health.record(level, penalty, message);
            }
        }
        if snapshot.avg_cpu_usage > CPU_HIGH_PCT {
            health.record(
                HealthStatus::Warning,
                PENALTY_CPU_HIGH,
                format!("CPU 使用率偏高: {:.1}%", snapshot.avg_cpu_usage),
            );
        }

        ClusterHealth {
            status: health.worst,
            score: health.score(),
            starrocks_version: "Unknown".to_string(),
            be_nodes_online,
            be_nodes_total,
            fe_nodes_online,
            fe_nodes_total,
            compaction_score,
            alerts: health.alerts,
        }
    }

    fn compaction_from_snapshot(snapshot: &Option<MetricsSnapshot>) -> CompactionStats {
        let max_score = snapshot
            .as_ref()
            .map(|s| s.max_compaction_score)
            .unwrap_or(0.0);
        CompactionStats { max_score, avg_score: max_score, ..CompactionStats::default() }
    }

    fn calculate_kpi(
        &self,
        snapshot: &Option<MetricsSnapshot>,
        snapshots: &[MetricsSnapshot],
    ) -> KeyPerformanceIndicators {
        let current = snapshot.as_ref();

        let prev_avg_qps = if snapshots.len() > 1 {
            let prev = &snapshots[0..snapshots.len() - 1];
            prev.iter().map(|s| s.qps).sum::<f64>() / prev.len() as f64
        } else {
            0.0
        };

        let prev_avg_latency = if snapshots.len() > 1 {
            let prev = &snapshots[0..snapshots.len() - 1];
            prev.iter().map(|s| s.query_latency_p99).sum::<f64>() / prev.len() as f64
        } else {
            0.0
        };

        let qps = current.map(|s| s.qps).unwrap_or(0.0);
        let p99_latency_ms = current.map(|s| s.query_latency_p99).unwrap_or(0.0);
        let qps_trend =
            if prev_avg_qps > 0.0 { ((qps - prev_avg_qps) / prev_avg_qps) * 100.0 } else { 0.0 };
        let p99_latency_trend = if prev_avg_latency > 0.0 {
            ((p99_latency_ms - prev_avg_latency) / prev_avg_latency) * 100.0
        } else {
            0.0
        };

        let (success_rate, error_rate) = if let Some(s) = current {
            let total = s.query_total as f64;
            let success = s.query_success as f64;
            let errors = s.query_error as f64;
            if total > 0.0 {
                ((success / total) * 100.0, (errors / total) * 100.0)
            } else {
                (100.0, 0.0)
            }
        } else {
            (100.0, 0.0)
        };

        KeyPerformanceIndicators {
            qps,
            qps_trend,
            p99_latency_ms,
            p99_latency_trend,
            success_rate,
            success_rate_trend: 0.0,
            error_rate,
        }
    }

    /// Module 3: Calculate resource metrics
    fn calculate_resource_metrics(
        &self,
        snapshot: &Option<MetricsSnapshot>,
        snapshots: &[MetricsSnapshot],
    ) -> ResourceMetrics {
        let current = snapshot.as_ref();

        let prev_avg_cpu = if snapshots.len() > 1 {
            let prev = &snapshots[0..snapshots.len() - 1];
            prev.iter().map(|s| s.avg_cpu_usage).sum::<f64>() / prev.len() as f64
        } else {
            0.0
        };

        let cpu_usage_pct = current.map(|s| s.avg_cpu_usage).unwrap_or(0.0);
        let memory_usage_pct = current.map(|s| s.avg_memory_usage).unwrap_or(0.0);
        let disk_usage_pct = current.map(|s| s.disk_usage_pct).unwrap_or(0.0);
        let compaction_score = current.map(|s| s.max_compaction_score).unwrap_or(0.0);

        let cpu_trend = if prev_avg_cpu > 0.0 {
            ((cpu_usage_pct - prev_avg_cpu) / prev_avg_cpu) * 100.0
        } else {
            0.0
        };

        let compaction_status = if compaction_score > 100.0 {
            "critical".to_string()
        } else if compaction_score > 50.0 {
            "warning".to_string()
        } else {
            "normal".to_string()
        };

        ResourceMetrics {
            cpu_usage_pct,
            cpu_trend,
            memory_usage_pct,
            memory_trend: 0.0,
            disk_usage_pct,
            disk_trend: 0.0,
            compaction_score,
            compaction_status,
        }
    }

    /// Module 9: Get transaction stats
    fn get_transaction_stats(&self, snapshot: &Option<MetricsSnapshot>) -> TransactionStats {
        let snapshot = snapshot.as_ref();
        TransactionStats {
            running: snapshot.map(|s| s.txn_running).unwrap_or(0),
            committed: snapshot.map(|s| s.txn_success_total as i32).unwrap_or(0),
            aborted: snapshot.map(|s| s.txn_failed_total as i32).unwrap_or(0),
        }
    }

    /// Get detailed compaction statistics for storage-compute separation architecture
    ///
    /// This method queries:
    /// 1. Top 10 partitions by compaction score from information_schema.partitions_meta
    /// 2. Running and finished compaction tasks from information_schema.be_cloud_native_compactions
    /// 3. Duration statistics (min, max, avg) for compactions within the time range
    pub async fn get_compaction_detail_stats(
        &self,
        cluster_id: i64,
        time_range: &str,
    ) -> ApiResult<CompactionDetailStats> {
        use crate::models::cluster::ClusterType;
        use crate::services::MySQLClient;

        let cluster = self.cluster_service.get_cluster(cluster_id).await?;

        if cluster.cluster_type == ClusterType::Doris {
            tracing::debug!(
                "[Doris] Compaction detail stats via HTTP API not fully implemented yet"
            );

            return Ok(empty_compaction_detail_stats());
        }

        if cluster.is_shared_nothing() {
            return self.shared_nothing_disk_stats(cluster).await;
        }

        let pool = self.mysql_pool_manager.get_pool(&cluster).await?;
        let client = MySQLClient::from_pool(pool).with_timeout(cluster_timeout(&cluster));

        let hours_back = match time_range {
            "1h" => 1,
            "6h" => 6,
            "24h" => 24,
            "3d" => 72,
            _ => 1,
        };

        let top_partitions_query = r#"
            SELECT 
                DB_NAME, 
                TABLE_NAME, 
                PARTITION_NAME, 
                MAX_CS as max_score, 
                AVG_CS as avg_score, 
                P50_CS as p50_score
            FROM information_schema.partitions_meta
            WHERE MAX_CS > 0 
              AND DB_NAME NOT IN ('_statistics_', 'information_schema', 'sys', 'starrocks_audit_db__', '__internal_schema', 'mysql')
              AND TABLE_NAME NOT IN ('starrocks_audit_tbl__', 'audit_log')
            ORDER BY MAX_CS DESC
            LIMIT 10
        "#;

        let (_headers, rows) = client
            .query_raw(top_partitions_query)
            .await
            .unwrap_or((vec![], vec![]));

        let top_partitions: Vec<TopPartitionByScore> = rows
            .into_iter()
            .filter_map(|row| {
                if row.len() >= 6 {
                    let db_name = row.first().map(|s| s.to_string()).unwrap_or_default();
                    let table_name = row.get(1).map(|s| s.to_string()).unwrap_or_default();

                    if db_name == "_statistics_"
                        || db_name == "information_schema"
                        || db_name == "sys"
                        || db_name == "starrocks_audit_db__"
                        || db_name == "__internal_schema"
                        || db_name == "mysql"
                        || table_name == "starrocks_audit_tbl__"
                        || table_name == "audit_log"
                    {
                        tracing::debug!("Filtering out system table: {}.{}", db_name, table_name);
                        return None;
                    }

                    Some(TopPartitionByScore {
                        db_name,
                        table_name,
                        partition_name: row.get(2).map(|s| s.to_string()).unwrap_or_default(),
                        max_score: row.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        avg_score: row.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                        p50_score: row.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    })
                } else {
                    None
                }
            })
            .collect();

        let task_stats_query = r#"SHOW PROC '/compactions'"#;

        let (_headers, rows) = client
            .query_raw(task_stats_query)
            .await
            .unwrap_or((vec![], vec![]));

        tracing::debug!("Compaction PROC query returned {} rows", rows.len());

        let mut total_count = 0;
        let mut running_count = 0;
        let mut finished_count = 0;
        let mut durations: Vec<i64> = Vec::new();

        for row in &rows {
            if row.len() >= 5 {
                let start_time_str = row.get(2).map(|s| s.to_string()).unwrap_or_default();
                let finish_time_str = row.get(4).map(|s| s.to_string()).unwrap_or_default();

                let is_within_time_range = if !start_time_str.is_empty() && start_time_str != "NULL"
                {
                    if let Ok(start_time) =
                        chrono::NaiveDateTime::parse_from_str(&start_time_str, "%Y-%m-%d %H:%M:%S")
                    {
                        let now = chrono::Utc::now().naive_utc();
                        let time_diff = now.signed_duration_since(start_time);
                        time_diff.num_hours() <= hours_back
                    } else {
                        false
                    }
                } else {
                    false
                };

                let is_running = finish_time_str.is_empty() || finish_time_str == "NULL";
                let _is_finished = !is_running;

                if is_within_time_range || is_running {
                    total_count += 1;
                    if is_running {
                        running_count += 1;
                    } else {
                        finished_count += 1;

                        if !start_time_str.is_empty()
                            && start_time_str != "NULL"
                            && !finish_time_str.is_empty()
                            && finish_time_str != "NULL"
                            && let (Ok(start_time), Ok(finish_time)) = (
                                chrono::NaiveDateTime::parse_from_str(
                                    &start_time_str,
                                    "%Y-%m-%d %H:%M:%S",
                                ),
                                chrono::NaiveDateTime::parse_from_str(
                                    &finish_time_str,
                                    "%Y-%m-%d %H:%M:%S",
                                ),
                            )
                        {
                            let duration =
                                finish_time.signed_duration_since(start_time).num_seconds();
                            durations.push(duration);
                        }
                    }
                }
            }
        }

        let task_stats = CompactionTaskStats { total_count, running_count, finished_count };

        tracing::debug!(
            "Processed compaction stats: total={}, running={}, finished={}",
            total_count,
            running_count,
            finished_count
        );

        let duration_stats = if durations.is_empty() {
            CompactionDurationStats { min_duration_ms: 0, max_duration_ms: 0, avg_duration_ms: 0 }
        } else {
            let min_duration = durations.iter().min().unwrap_or(&0);
            let max_duration = durations.iter().max().unwrap_or(&0);
            let avg_duration = durations.iter().sum::<i64>() / durations.len() as i64;

            CompactionDurationStats {
                min_duration_ms: min_duration * 1000,
                max_duration_ms: max_duration * 1000,
                avg_duration_ms: avg_duration * 1000,
            }
        };

        tracing::debug!(
            "Duration stats: min={}ms, max={}ms, avg={}ms",
            duration_stats.min_duration_ms,
            duration_stats.max_duration_ms,
            duration_stats.avg_duration_ms
        );

        Ok(CompactionDetailStats {
            top_partitions,
            task_stats,
            duration_stats,
            node_disks: Vec::new(),
        })
    }

    async fn shared_nothing_disk_stats(
        &self,
        cluster: crate::models::Cluster,
    ) -> ApiResult<CompactionDetailStats> {
        let adapter = crate::services::create_adapter(cluster, self.mysql_pool_manager.clone());
        let backends = match adapter.get_backends().await {
            Ok(nodes) => nodes,
            Err(error) => {
                tracing::warn!("Failed to list backends for shared-nothing disk panel: {}", error);
                Vec::new()
            },
        };
        let mut node_disks: Vec<NodeDiskUsage> = backends
            .iter()
            .filter_map(|node| {
                let used_pct = parse_usage_pct(&node.max_disk_used_pct)
                    .or_else(|| parse_usage_pct(&node.used_pct))
                    .or_else(|| parse_usage_pct(&node.data_used_pct))?;
                let host =
                    if node.host.is_empty() { node.backend_id.clone() } else { node.host.clone() };
                Some(NodeDiskUsage { host, used_pct })
            })
            .collect();
        node_disks.sort_by(|left, right| right.used_pct.total_cmp(&left.used_pct));
        node_disks.truncate(10);
        Ok(CompactionDetailStats { node_disks, ..empty_compaction_detail_stats() })
    }

    /// Module 13: Calculate network & IO stats
    fn calculate_network_io_stats(&self, snapshot: &Option<MetricsSnapshot>) -> NetworkIOStats {
        let snapshot = snapshot.as_ref();
        NetworkIOStats {
            network_tx_bytes_per_sec: snapshot.map(|s| s.network_send_rate).unwrap_or(0.0),
            network_rx_bytes_per_sec: snapshot.map(|s| s.network_receive_rate).unwrap_or(0.0),
            disk_read_bytes_per_sec: snapshot.map(|s| s.io_read_rate).unwrap_or(0.0),
            disk_write_bytes_per_sec: snapshot.map(|s| s.io_write_rate).unwrap_or(0.0),
        }
    }

    /// Module 18: Generate alerts based on current state
    fn generate_alerts(
        &self,
        health: &ClusterHealth,
        resources: &ResourceMetrics,
        snapshot: &Option<MetricsSnapshot>,
        disk: DiskMetricKind,
    ) -> Vec<Alert> {
        let mut alerts = Vec::new();

        if snapshot.is_none() {
            alerts.push(Alert {
                level: AlertLevel::Warning,
                category: "采集".to_string(),
                message: "暂无采集数据".to_string(),
                timestamp: Utc::now(),
                action: None,
            });
            return alerts;
        }

        if health.be_nodes_online < health.be_nodes_total {
            alerts.push(Alert {
                level: AlertLevel::Critical,
                category: "节点状态".to_string(),
                message: format!("{} 计算节点离线", health.be_nodes_total - health.be_nodes_online),
                timestamp: Utc::now(),
                action: Some("检查计算节点状态并重启".to_string()),
            });
        }

        if health.compaction_score > COMPACTION_CRITICAL_SCORE {
            alerts.push(Alert {
                level: AlertLevel::Critical,
                category: "Compaction".to_string(),
                message: format!("Compaction Score过高: {:.1}", health.compaction_score),
                timestamp: Utc::now(),
                action: Some("检查磁盘IO性能，考虑增加计算节点".to_string()),
            });
        }

        // 数据缓存配额写满是缓存常态，不得报成磁盘告警（见 DiskMetricKind）
        if disk.tracks_capacity() && resources.disk_usage_pct > DISK_WARNING_PCT {
            let (level, wording) = if resources.disk_usage_pct > DISK_CRITICAL_PCT {
                (AlertLevel::Critical, "磁盘使用率过高")
            } else {
                (AlertLevel::Warning, "磁盘使用率偏高")
            };
            alerts.push(Alert {
                level,
                category: "容量".to_string(),
                message: format!("{wording}: {:.1}%", resources.disk_usage_pct),
                timestamp: Utc::now(),
                action: Some("清理过期数据或扩容磁盘".to_string()),
            });
        }

        // 单节点磁盘压力单独成条：category 与容量区分开，处置方式不同
        if let Some(snapshot) = snapshot.as_ref() {
            if let Some((level, message)) = node_disk_pressure(
                disk.tracks_capacity(),
                snapshot.max_disk_usage_pct,
                resources.disk_usage_pct,
            ) {
                alerts.push(Alert {
                    level: match level {
                        HealthStatus::Critical => AlertLevel::Critical,
                        _ => AlertLevel::Warning,
                    },
                    category: "节点分布".to_string(),
                    message,
                    timestamp: Utc::now(),
                    action: Some("定位使用率最高的节点，检查数据分布与分区分桶".to_string()),
                });
            }
        }

        if resources.cpu_usage_pct > CPU_HIGH_PCT {
            alerts.push(Alert {
                level: AlertLevel::Warning,
                category: "资源".to_string(),
                message: format!("CPU使用率偏高: {:.1}%", resources.cpu_usage_pct),
                timestamp: Utc::now(),
                action: Some("检查慢查询，优化查询性能".to_string()),
            });
        }

        if let Some(snapshot) = snapshot {
            if snapshot.safe_mode > 0 {
                alerts.push(Alert {
                    level: AlertLevel::Critical,
                    category: "FE".to_string(),
                    message: "集群处于 Safe Mode".to_string(),
                    timestamp: Utc::now(),
                    action: Some("检查磁盘和元数据 Checkpoint".to_string()),
                });
            }
            if snapshot.meta_log_count > 100_000 {
                alerts.push(Alert {
                    level: AlertLevel::Critical,
                    category: "FE".to_string(),
                    message: format!("Meta Log 未 Checkpoint: {}", snapshot.meta_log_count),
                    timestamp: Utc::now(),
                    action: Some("检查 FE JVM 与 Checkpoint".to_string()),
                });
            } else if snapshot.meta_log_count > 50_000 {
                alerts.push(Alert {
                    level: AlertLevel::Warning,
                    category: "FE".to_string(),
                    message: format!("Meta Log 堆积: {}", snapshot.meta_log_count),
                    timestamp: Utc::now(),
                    action: Some("关注 FE Checkpoint 是否停滞".to_string()),
                });
            }
        }

        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::MySQLPoolManager;
    use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
    use std::time::Duration;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect("sqlite::memory:")
            .await
            .expect("test db");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("enable foreign keys");
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    async fn seed_cluster(pool: &SqlitePool) {
        seed_cluster_with_mode(pool, "shared_data").await;
    }

    async fn seed_cluster_with_mode(pool: &SqlitePool, deployment_mode: &str) {
        sqlx::query(
            "INSERT INTO organizations (code, name, description, is_system) VALUES ('org', 'Org', '', 0)",
        )
        .execute(pool)
        .await
        .expect("org");
        sqlx::query(
            "INSERT INTO clusters (name, fe_host, fe_http_port, fe_query_port, username, password_encrypted, catalog, deployment_mode, cluster_type, is_active, organization_id)
             VALUES ('c1', '127.0.0.1', 8030, 9030, 'root', 'p', 'default_catalog', ?, 'starrocks', 1, 1)",
        )
        .bind(deployment_mode)
        .execute(pool)
        .await
        .expect("cluster");
    }

    /// 插入一条快照；参数至少包含 BE/FE 在线数、磁盘水位、compaction score
    #[allow(clippy::too_many_arguments)]
    async fn seed_snapshot(
        pool: &SqlitePool,
        be_online: i32,
        be_total: i32,
        fe_online: i32,
        fe_total: i32,
        disk_usage_pct: f64,
        compaction_score: f64,
        avg_cpu_usage: f64,
    ) {
        let used = (disk_usage_pct * 10.0) as i64;
        sqlx::query(
            r#"
            INSERT INTO metrics_snapshots (
                cluster_id, collected_at, qps, query_latency_p99,
                backend_total, backend_alive, frontend_total, frontend_alive,
                avg_cpu_usage, disk_total_bytes, disk_used_bytes, disk_usage_pct,
                max_compaction_score, load_running
            ) VALUES (1, ?, 1.0, 10.0, ?, ?, ?, ?, ?, 1000, ?, ?, ?, 0)
            "#,
        )
        .bind(Utc::now())
        .bind(be_total)
        .bind(be_online)
        .bind(fe_total)
        .bind(fe_online)
        .bind(avg_cpu_usage)
        .bind(used)
        .bind(disk_usage_pct)
        .bind(compaction_score)
        .execute(pool)
        .await
        .expect("snapshot");
    }

    fn service(pool: SqlitePool) -> OverviewService<sqlx::Sqlite> {
        let mysql = Arc::new(MySQLPoolManager::new());
        let cluster_service = Arc::new(ClusterService::new(pool.clone(), mysql.clone()));
        OverviewService::new(pool, cluster_service, mysql)
    }

    #[tokio::test]
    async fn extended_overview_without_snapshot_succeeds() {
        let pool = test_pool().await;
        seed_cluster(&pool).await;
        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert_eq!(overview.cluster_name, "c1");
        assert_eq!(overview.deployment_mode, crate::models::cluster::DeploymentMode::SharedData);
        assert!(matches!(overview.health.status, HealthStatus::Warning));
        assert!(
            overview
                .health
                .alerts
                .iter()
                .any(|a| a.contains("暂无采集数据"))
        );
        assert!(
            overview
                .alerts
                .iter()
                .any(|alert| alert.message.contains("暂无采集数据"))
        );
        assert!(overview.performance_trends.qps.is_empty());
        assert!(overview.data_stats.is_none());
        assert_eq!(overview.sessions.current_connections, 0);
        assert!(overview.capacity.is_none());
    }

    #[tokio::test]
    async fn extended_overview_reads_snapshot_only() {
        let pool = test_pool().await;
        seed_cluster(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO metrics_snapshots (
                cluster_id, collected_at, qps, query_latency_p99,
                backend_total, backend_alive, frontend_total, frontend_alive,
                avg_cpu_usage, disk_total_bytes, disk_used_bytes, disk_usage_pct,
                max_compaction_score, load_running
            ) VALUES (1, ?, 12.5, 320.0, 3, 3, 1, 1, 22.0, 1000, 410, 41.0, 8.0, 2)
            "#,
        )
        .bind(Utc::now())
        .execute(&pool)
        .await
        .expect("snapshot");

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert!(matches!(overview.health.status, HealthStatus::Healthy));
        assert_eq!(overview.health.be_nodes_online, 3);
        assert_eq!(overview.health.be_nodes_total, 3);
        assert!((overview.kpi.qps - 12.5).abs() < f64::EPSILON);
        assert!((overview.kpi.p99_latency_ms - 320.0).abs() < f64::EPSILON);
        assert_eq!(overview.load_jobs.running, 2);
        assert_eq!(overview.compaction.max_score, 8.0);
        assert_eq!(overview.performance_trends.timeout_rate.len(), 1);
        assert_eq!(overview.resource_trends.backend_alive.len(), 1);
        assert!((overview.resource_trends.backend_alive[0].value - 3.0).abs() < f64::EPSILON);
        assert!(overview.data_stats.is_none());
        assert_eq!(overview.mv_stats.total, 0);
        assert!(overview.sessions.running_queries.is_empty());
        assert_eq!(overview.performance_trends.qps.len(), 1);
        assert_eq!(overview.performance_trends.error_rate.len(), 1);
        assert_eq!(overview.resource_trends.compaction_score.len(), 1);
        assert!((overview.resource_trends.compaction_score[0].value - 8.0).abs() < f64::EPSILON);
        assert_eq!(overview.resource_trends.tablet_count.len(), 1);
        assert_eq!(overview.resource_trends.jvm_thread_count.len(), 1);
        assert_eq!(overview.resource_trends.txn_success.len(), 1);
        assert_eq!(overview.meta_log_count, 0);
        assert!(!overview.safe_mode);
        assert!(overview.capacity.is_some());
        assert_eq!(overview.capacity.as_ref().and_then(|c| c.days_until_full), None);
    }

    #[tokio::test]
    async fn capacity_days_until_full_is_zero_when_disk_is_full() {
        let pool = test_pool().await;
        seed_cluster_with_mode(&pool, "shared_nothing").await;
        seed_snapshot(&pool, 1, 1, 1, 1, 100.0, 1.0, 10.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert_eq!(overview.capacity.as_ref().and_then(|c| c.days_until_full), Some(0));
    }

    #[tokio::test]
    async fn shared_data_cache_quota_full_is_healthy_not_capacity_event() {
        // 真实集群 bj-com：shared-data + `DataCacheMetrics: DiskUsage 9.1TB/9.1TB`，
        // 采集回退到缓存配额 → disk_usage_pct=100。缓存写满是 LRU 淘汰的稳态，不得报磁盘事故。
        let pool = test_pool().await;
        seed_cluster(&pool).await; // shared_data
        seed_snapshot(&pool, 54, 54, 3, 3, 100.0, 9.0, 0.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert!(matches!(overview.health.status, HealthStatus::Healthy));
        assert!((overview.health.score - 100.0).abs() < f64::EPSILON);
        assert!(!overview.health.alerts.iter().any(|a| a.contains("磁盘")));
        assert!(!overview.alerts.iter().any(|a| a.message.contains("磁盘")));
        assert_eq!(overview.capacity.as_ref().and_then(|c| c.days_until_full), None);
        assert_eq!(overview.capacity.as_ref().map(|c| c.daily_growth_bytes), Some(0));
    }

    #[tokio::test]
    async fn shared_nothing_disk_critical_is_critical_and_caps_score() {
        let pool = test_pool().await;
        seed_cluster_with_mode(&pool, "shared_nothing").await;
        seed_snapshot(&pool, 3, 3, 3, 3, 100.0, 9.0, 0.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        // 扣 20 分 → 80，但 Critical 维度必须封顶，不能粉饰成「还行」
        assert!(matches!(overview.health.status, HealthStatus::Critical));
        assert_eq!(overview.health.score, SCORE_CAP_ON_CRITICAL);
        assert!(
            overview
                .health
                .alerts
                .iter()
                .any(|a| a.contains("磁盘使用率过高"))
        );
        // 告警与状态卡同口径：横幅也必须说「过高」而不是「偏高」
        let banner = overview
            .alerts
            .iter()
            .find(|a| a.message.contains("磁盘"))
            .expect("磁盘横幅告警");
        assert!(matches!(banner.level, AlertLevel::Critical));
        assert!(banner.message.contains("过高"));
    }

    #[tokio::test]
    async fn disk_warning_threshold_is_shared_by_status_and_score() {
        let pool = test_pool().await;
        seed_cluster_with_mode(&pool, "shared_nothing").await;
        seed_snapshot(&pool, 3, 3, 3, 3, 85.0, 9.0, 0.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert!(matches!(overview.health.status, HealthStatus::Warning));
        assert_eq!(overview.health.score, 100.0 - PENALTY_DISK_WARNING);
        let banner = overview
            .alerts
            .iter()
            .find(|a| a.message.contains("磁盘"))
            .expect("磁盘横幅告警");
        assert!(matches!(banner.level, AlertLevel::Warning));
        assert!(banner.message.contains("偏高"));
    }

    #[tokio::test]
    async fn fe_node_offline_counts_toward_health() {
        let pool = test_pool().await;
        seed_cluster(&pool).await;
        seed_snapshot(&pool, 3, 3, 1, 3, 10.0, 9.0, 0.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert!(matches!(overview.health.status, HealthStatus::Critical));
        assert_eq!(overview.health.score, SCORE_CAP_ON_CRITICAL);
        assert!(overview.health.alerts.iter().any(|a| a == "2 FE 节点离线"));
    }

    #[tokio::test]
    async fn snapshot_without_any_node_data_scores_zero() {
        let pool = test_pool().await;
        seed_cluster(&pool).await;
        seed_snapshot(&pool, 0, 0, 0, 0, 0.0, 0.0, 0.0).await;

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert!(matches!(overview.health.status, HealthStatus::Warning));
        assert!(overview.health.score < f64::EPSILON);
        assert!(
            overview
                .health
                .alerts
                .iter()
                .any(|a| a.contains("暂无采集数据"))
        );
    }

    #[tokio::test]
    async fn extended_overview_alerts_on_meta_log_and_safe_mode() {
        let pool = test_pool().await;
        seed_cluster(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO metrics_snapshots (
                cluster_id, collected_at, backend_total, backend_alive, frontend_total, frontend_alive,
                meta_log_count, safe_mode
            ) VALUES (1, ?, 3, 3, 1, 1, 60000, 1)
            "#,
        )
        .bind(Utc::now())
        .execute(&pool)
        .await
        .expect("snapshot");

        let overview = service(pool)
            .get_extended_overview(1, TimeRange::Hours1)
            .await
            .expect("overview");

        assert_eq!(overview.meta_log_count, 60_000);
        assert!(overview.safe_mode);
        assert!(
            overview
                .alerts
                .iter()
                .any(|alert| alert.message.contains("Safe Mode"))
        );
        assert!(
            overview
                .alerts
                .iter()
                .any(|alert| alert.message.contains("Meta Log"))
        );
    }

    #[test]
    fn parse_usage_pct_reads_starrocks_backend_format() {
        assert_eq!(super::parse_usage_pct("85.20 %"), Some(85.2));
        assert_eq!(super::parse_usage_pct("12.5%"), Some(12.5));
        assert_eq!(super::parse_usage_pct(""), None);
        assert_eq!(super::parse_usage_pct("N/A"), None);
    }

    #[test]
    fn snapshot_ratio_pct_uses_increment_then_falls_back() {
        assert!(
            (super::OverviewService::<sqlx::Sqlite>::snapshot_ratio_pct(8, 100, Some((5, 80)))
                - 15.0)
                .abs()
                < f64::EPSILON
        );
        assert!(
            (super::OverviewService::<sqlx::Sqlite>::snapshot_ratio_pct(8, 100, None) - 8.0).abs()
                < f64::EPSILON
        );
        assert_eq!(super::OverviewService::<sqlx::Sqlite>::snapshot_ratio_pct(0, 0, None), 0.0);
    }

    #[test]
    fn counter_delta_skips_zero_and_fe_switch() {
        assert_eq!(
            super::counter_delta_vs_last_positive([0, 100, 0, 130, 80, 95]),
            vec![0.0, 0.0, 0.0, 30.0, 0.0, 15.0]
        );
    }
}
