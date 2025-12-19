//! Baseline Service - Production-Ready Implementation
//!
//! This service manages baseline calculation and caching:
//! 1. **Background refresh**: Async updates without blocking requests
//! 2. **Graceful fallback**: Works when audit log unavailable
//! 3. **Caching**: In-memory cache with configurable TTL
//! 4. **Error resilience**: Never fails, always returns valid data

use crate::services::mysql_client::MySQLClient;
use crate::services::profile_analyzer::analyzer::{
    AuditLogRecord, BaselineCacheManager, BaselineCalculator, BaselineProvider,
    BaselineRefreshConfig, BaselineSource, PerformanceBaseline, QueryComplexity,
};
use std::collections::HashMap;
use tracing::{error, info, warn};

// ============================================================================
// Baseline Service
// ============================================================================

/// Production-ready baseline service
///
/// # Design Principles
///
/// 1. **Never block request path**: Baseline fetching is async/cached
/// 2. **Always return valid data**: Defaults if audit log unavailable
/// 3. **Minimal database load**: Cache results, batch queries
/// 4. **Observable**: Logging and metrics for monitoring
pub struct BaselineService {
    calculator: BaselineCalculator,
    config: BaselineRefreshConfig,
}

impl BaselineService {
    pub fn new() -> Self {
        Self { calculator: BaselineCalculator::new(), config: BaselineRefreshConfig::default() }
    }

    pub fn with_config(config: BaselineRefreshConfig) -> Self {
        Self { calculator: BaselineCalculator::new(), config }
    }

    /// Get baseline for specific cluster and complexity - FAST, NEVER BLOCKS
    ///
    /// This is the main entry point. It:
    /// 1. Checks cache first (O(1) lookup)
    /// 2. Returns cached data if valid
    /// 3. Falls back to defaults if cache miss
    ///
    /// # Example
    /// ```rust
    /// let baseline = service.get_baseline(cluster_id, QueryComplexity::Medium);
    /// let threshold = baseline.stats.p95_ms + 2.0 * baseline.stats.std_dev_ms;
    /// ```
    pub fn get_baseline(
        &self,
        cluster_id: i64,
        complexity: QueryComplexity,
    ) -> PerformanceBaseline {
        BaselineProvider::get(cluster_id, complexity)
    }

    /// Get baseline without cluster (backward compatibility)
    pub fn get_baseline_default(&self, complexity: QueryComplexity) -> PerformanceBaseline {
        BaselineProvider::get_default(complexity)
    }

    /// Get all baselines for cluster - FAST, NEVER BLOCKS
    pub fn get_all_baselines(
        &self,
        cluster_id: i64,
    ) -> HashMap<QueryComplexity, PerformanceBaseline> {
        let mut result = HashMap::new();
        for complexity in [
            QueryComplexity::Simple,
            QueryComplexity::Medium,
            QueryComplexity::Complex,
            QueryComplexity::VeryComplex,
        ] {
            result.insert(complexity, self.get_baseline(cluster_id, complexity));
        }
        result
    }

    /// Check if we have real audit data for cluster (not defaults)
    pub fn has_audit_data(&self, cluster_id: i64) -> bool {
        BaselineProvider::has_audit_data(cluster_id)
    }

    /// Refresh baselines from audit log for a specific cluster
    ///
    /// This method:
    /// 1. Queries audit log table
    /// 2. Calculates baselines
    /// 3. Updates cluster-specific cache
    ///
    /// # Errors
    /// Returns Err if audit log query fails, but cache remains valid
    pub async fn refresh_from_audit_log_for_cluster(
        &self,
        mysql: &MySQLClient,
        cluster_id: i64,
        cluster_type: &crate::models::cluster::ClusterType,
    ) -> Result<RefreshResult, String> {
        info!("Starting baseline refresh for cluster {} from audit log", cluster_id);

        if !self.audit_table_exists(mysql, cluster_type).await {
            warn!("Audit log table not found for cluster {}, using defaults", cluster_id);
            BaselineProvider::update(
                cluster_id,
                BaselineCacheManager::default_baselines(),
                BaselineSource::Default,
            );
            return Ok(RefreshResult {
                source: BaselineSource::Default,
                sample_count: 0,
                complexity_counts: HashMap::new(),
            });
        }

        let records = match self.fetch_audit_logs(mysql, cluster_type).await {
            Ok(records) => records,
            Err(e) => {
                error!("Failed to fetch audit logs for cluster {}: {}", cluster_id, e);
                BaselineProvider::update(
                    cluster_id,
                    BaselineCacheManager::default_baselines(),
                    BaselineSource::Default,
                );
                return Err(e);
            },
        };

        if records.is_empty() {
            warn!("No audit records found for cluster {}, using defaults", cluster_id);
            BaselineProvider::update(
                cluster_id,
                BaselineCacheManager::default_baselines(),
                BaselineSource::Default,
            );
            return Ok(RefreshResult {
                source: BaselineSource::Default,
                sample_count: 0,
                complexity_counts: HashMap::new(),
            });
        }

        let baselines = self.calculator.calculate_by_complexity(&records);

        let mut final_baselines = BaselineCacheManager::default_baselines();
        let mut complexity_counts = HashMap::new();

        for (complexity, baseline) in baselines {
            complexity_counts.insert(complexity, baseline.sample_size);
            final_baselines.insert(complexity, baseline);
        }

        let sample_count = records.len();
        if let Some(drift) =
            BaselineProvider::update(cluster_id, final_baselines, BaselineSource::AuditLog)
        {
            if drift.has_degradation() {
                warn!("Cluster {}: {}", cluster_id, drift.summary());
            } else {
                info!("Cluster {}: {}", cluster_id, drift.summary());
            }
        }

        info!(
            "Baseline refresh complete for cluster {}: {} records, {:?}",
            cluster_id, sample_count, complexity_counts
        );

        Ok(RefreshResult { source: BaselineSource::AuditLog, sample_count, complexity_counts })
    }

    /// Refresh baselines from audit log (backward compatibility, uses cluster_id=0)
    pub async fn refresh_from_audit_log(
        &self,
        mysql: &MySQLClient,
    ) -> Result<RefreshResult, String> {
        self.refresh_from_audit_log_for_cluster(
            mysql,
            0,
            &crate::models::cluster::ClusterType::StarRocks,
        )
        .await
    }

    /// Calculate baseline for a specific table (on-demand, not cached)
    pub async fn calculate_table_baseline(
        &self,
        mysql: &MySQLClient,
        table_name: &str,
        cluster_type: &crate::models::cluster::ClusterType,
    ) -> Result<Option<PerformanceBaseline>, String> {
        let records = self.fetch_audit_logs(mysql, cluster_type).await?;
        Ok(self.calculator.calculate_for_table(&records, table_name))
    }

    /// Check if audit log table exists
    async fn audit_table_exists(
        &self,
        mysql: &MySQLClient,
        cluster_type: &crate::models::cluster::ClusterType,
    ) -> bool {
        use crate::models::cluster::ClusterType;

        let audit_table = match cluster_type {
            ClusterType::StarRocks => "starrocks_audit_db__.starrocks_audit_tbl__",
            ClusterType::Doris => "__internal_schema.audit_log",
        };

        let sql = format!("SELECT 1 FROM {} LIMIT 1", audit_table);
        mysql.query_raw(&sql).await.is_ok()
    }

    /// Fetch audit log records
    async fn fetch_audit_logs(
        &self,
        mysql: &MySQLClient,
        cluster_type: &crate::models::cluster::ClusterType,
    ) -> Result<Vec<AuditLogRecord>, String> {
        use crate::models::cluster::ClusterType;

        let (
            audit_table,
            query_id_field,
            user_field,
            db_field,
            stmt_field,
            stmt_type_field,
            query_time_field,
            state_field,
            time_field,
            is_query_field,
        ) = match cluster_type {
            ClusterType::StarRocks => (
                "starrocks_audit_db__.starrocks_audit_tbl__",
                "queryId",
                "user",
                "db",
                "stmt",
                "queryType",
                "queryTime",
                "state",
                "timestamp",
                "isQuery",
            ),
            ClusterType::Doris => (
                "__internal_schema.audit_log",
                "query_id",
                "user",
                "database",
                "stmt",
                "stmt_type",
                "query_time",
                "state",
                "time",
                "is_query",
            ),
        };

        let sql = format!(
            r#"
            SELECT 
                `{query_id_field}` as queryId,
                COALESCE(`{user_field}`, '') AS user,
                COALESCE(`{db_field}`, '') AS db,
                `{stmt_field}` as stmt,
                COALESCE(`{stmt_type_field}`, 'Query') AS queryType,
                `{query_time_field}` AS query_time_ms,
                COALESCE(`{state_field}`, '') AS state,
                `{time_field}` as timestamp
            FROM {audit_table}
            WHERE 
                `{is_query_field}` = 1
                AND `{time_field}` >= DATE_SUB(NOW(), INTERVAL {} HOUR)
                AND `{state_field}` IN ('EOF', 'OK')
                AND `{query_time_field}` > 0
            ORDER BY `{time_field}` DESC
            LIMIT 10000
            "#,
            self.config.audit_log_hours
        );

        let (columns, rows) = mysql
            .query_raw(&sql)
            .await
            .map_err(|e| format!("Audit log query failed: {:?}", e))?;

        let mut col_idx = HashMap::new();
        for (i, col) in columns.iter().enumerate() {
            col_idx.insert(col.clone(), i);
        }

        let mut records = Vec::with_capacity(rows.len());
        for row in &rows {
            let query_id = col_idx
                .get("queryId")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            let user = col_idx
                .get("user")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            let db = col_idx
                .get("db")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            let stmt = col_idx
                .get("stmt")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            let query_type = col_idx
                .get("queryType")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_else(|| "Query".to_string());

            let query_time_ms = col_idx
                .get("query_time_ms")
                .and_then(|&i| row.get(i))
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);

            let state = col_idx
                .get("state")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            let timestamp = col_idx
                .get("timestamp")
                .and_then(|&i| row.get(i))
                .cloned()
                .unwrap_or_default();

            records.push(AuditLogRecord {
                query_id,
                user,
                db,
                stmt,
                query_type,
                query_time_ms,
                state,
                timestamp,
            });
        }

        Ok(records)
    }
}

impl Default for BaselineService {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Refresh Result
// ============================================================================

/// Result of baseline refresh operation
#[derive(Debug)]
pub struct RefreshResult {
    /// Data source
    pub source: BaselineSource,
    /// Total sample count
    pub sample_count: usize,
    /// Sample count per complexity
    pub complexity_counts: HashMap<QueryComplexity, usize>,
}

// ============================================================================
// Initialization Helper
// ============================================================================

/// Initialize baseline system (call once at application startup)
///
/// # Example
/// ```rust
/// // In main.rs or startup code
/// init_baseline_system();
///
/// // Optionally trigger initial refresh
/// if let Some(mysql) = get_mysql_client() {
///     let service = BaselineService::new();
///     tokio::spawn(async move {
///         let _ = service.refresh_from_audit_log(&mysql).await;
///     });
/// }
/// ```
pub fn init_baseline_system() {
    BaselineProvider::init();
    info!("Baseline system initialized with default cache");
}

/// Initialize with custom TTL
pub fn init_baseline_system_with_ttl(ttl_seconds: u64) {
    BaselineProvider::init_with_ttl(ttl_seconds);
    info!("Baseline system initialized with {}s TTL", ttl_seconds);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_creation() {
        let service = BaselineService::new();
        let cluster_id = 1;

        let baseline = service.get_baseline(cluster_id, QueryComplexity::Medium);
        assert!(baseline.stats.avg_ms > 0.0);
    }

    #[test]
    fn test_get_all_baselines() {
        let service = BaselineService::new();
        let cluster_id = 1;
        let baselines = service.get_all_baselines(cluster_id);

        assert_eq!(baselines.len(), 4);
        assert!(baselines.contains_key(&QueryComplexity::Simple));
        assert!(baselines.contains_key(&QueryComplexity::Medium));
        assert!(baselines.contains_key(&QueryComplexity::Complex));
        assert!(baselines.contains_key(&QueryComplexity::VeryComplex));
    }

    #[test]
    fn test_default_fallback() {
        let service = BaselineService::new();
        let cluster_id = 1;
        assert!(!service.has_audit_data(cluster_id));

        let baseline = service.get_baseline(cluster_id, QueryComplexity::Complex);
        assert!(baseline.stats.p95_ms > 0.0);
    }
}
