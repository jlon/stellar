//! Historical Baseline Calculator
//!
//! This module calculates performance baselines from audit log data
//! to enable adaptive thresholds based on historical query behavior.
//!
//! Key Features:
//! - Query complexity-based grouping (simple/medium/complex/very complex)
//! - Table-level performance baseline (average query time per table)
//! - User-level performance baseline
//! - Time-based trend analysis (weekday/weekend, peak hours)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Historical Baseline Models
// ============================================================================

/// Historical query performance baseline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceBaseline {
    /// Query complexity level
    pub complexity: QueryComplexity,
    /// Statistics calculated from audit log
    pub stats: BaselineStats,
    /// Sample size (number of historical queries)
    pub sample_size: usize,
    /// Time range of the baseline data
    pub time_range_hours: u32,
}

/// Query complexity classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueryComplexity {
    /// Simple: single table scan, no JOIN
    Simple,
    /// Medium: 2-3 table JOIN, simple aggregation
    Medium,
    /// Complex: 4+ table JOIN, window functions, subqueries
    Complex,
    /// Very Complex: nested CTEs, multiple UDF calls, heavy computation
    VeryComplex,
}

impl QueryComplexity {
    /// Detect query complexity from SQL statement
    /// Uses token-based analysis to avoid false positives from strings/comments
    ///
    /// Scoring rules (v2.0 - production ready):
    /// - JOIN: +2 per join (min 1 join = Medium)
    /// - Subquery: +2 per subquery
    /// - Window function (OVER): +4
    /// - CTE (WITH...AS): +3
    /// - UNION/INTERSECT/EXCEPT: +2
    /// - COUNT(DISTINCT)/SUM(DISTINCT): +3 (expensive!)
    /// - ORDER BY without LIMIT: +2 (full sort)
    /// - LATERAL VIEW/EXPLODE/UNNEST: +3
    /// - REGEXP/RLIKE: +1
    /// - EXISTS/NOT EXISTS: +2
    /// - GROUP BY + HAVING: +1
    /// - Multiple aggregates: +1~2
    pub fn from_sql(sql: &str) -> Self {
        let cleaned = Self::remove_strings_and_comments(sql);
        let tokens = Self::tokenize(&cleaned);
        let sql_upper = cleaned.to_uppercase();

        let join_count = tokens.iter().filter(|t| *t == "JOIN").count();
        let select_count = tokens.iter().filter(|t| *t == "SELECT").count();
        let subquery_count = select_count.saturating_sub(1);

        let has_window = tokens.windows(2).any(|w| w[0] == "OVER");

        let has_cte = tokens
            .iter()
            .position(|t| t == "WITH")
            .map(|i| tokens[i..].iter().take(15).any(|t| t == "AS"))
            .unwrap_or(false)
            && select_count > 1;

        let has_set_op = tokens
            .iter()
            .any(|t| t == "UNION" || t == "INTERSECT" || t == "EXCEPT");

        let has_distinct_agg = sql_upper.contains("COUNT(DISTINCT")
            || sql_upper.contains("COUNT (DISTINCT")
            || sql_upper.contains("SUM(DISTINCT")
            || sql_upper.contains("AVG(DISTINCT");

        let has_order = tokens.iter().any(|t| t == "ORDER");
        let has_limit = tokens.iter().any(|t| t == "LIMIT");
        let has_expensive_sort = has_order && !has_limit;

        let has_lateral = tokens
            .iter()
            .any(|t| t == "LATERAL" || t == "EXPLODE" || t == "UNNEST" || t == "POSEXPLODE");

        let has_exists = tokens.iter().any(|t| t == "EXISTS");

        let has_regex = tokens.iter().any(|t| t == "REGEXP" || t == "RLIKE");

        let has_group_having =
            tokens.iter().any(|t| t == "GROUP") && tokens.iter().any(|t| t == "HAVING");

        let agg_funcs = [
            "COUNT",
            "SUM",
            "AVG",
            "MAX",
            "MIN",
            "GROUP_CONCAT",
            "APPROX_COUNT_DISTINCT",
            "HLL_UNION_AGG",
            "BITMAP_UNION",
        ];
        let agg_count = tokens
            .iter()
            .filter(|t| agg_funcs.contains(&t.as_str()))
            .count();

        let mut score = 0;

        score += match join_count {
            0 => 0,
            1 => 3,
            2..=3 => join_count * 2 + 1,
            _ => join_count * 2 + 2,
        };

        score += subquery_count * 2;
        if has_window {
            score += 4;
        }
        if has_cte {
            score += 3;
        }
        if has_set_op {
            score += 2;
        }
        if has_distinct_agg {
            score += 3;
        }
        if has_expensive_sort {
            score += 2;
        }
        if has_lateral {
            score += 3;
        }
        if has_exists {
            score += 2;
        }
        if has_regex {
            score += 1;
        }
        if has_group_having {
            score += 1;
        }
        score += (agg_count / 2).min(2);

        match score {
            0..=2 => Self::Simple,
            3..=7 => Self::Medium,
            8..=15 => Self::Complex,
            _ => Self::VeryComplex,
        }
    }

    /// Remove string literals and comments from SQL to avoid false keyword matches
    fn remove_strings_and_comments(sql: &str) -> String {
        let mut result = String::with_capacity(sql.len());
        let mut chars = sql.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    while let Some(c2) = chars.next() {
                        if c2 == '\'' {
                            if chars.peek() == Some(&'\'') {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    result.push(' ');
                },

                '"' => {
                    for c2 in chars.by_ref() {
                        if c2 == '"' {
                            break;
                        }
                    }
                    result.push(' ');
                },

                '-' if chars.peek() == Some(&'-') => {
                    chars.next();
                    for c2 in chars.by_ref() {
                        if c2 == '\n' {
                            break;
                        }
                    }
                },

                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    while let Some(c2) = chars.next() {
                        if c2 == '*' && chars.peek() == Some(&'/') {
                            chars.next();
                            break;
                        }
                    }
                },
                _ => result.push(c),
            }
        }
        result
    }

    /// Tokenize SQL into uppercase keywords/identifiers
    fn tokenize(sql: &str) -> Vec<String> {
        sql.to_uppercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }
}

/// Baseline statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaselineStats {
    /// Average query time (ms)
    pub avg_ms: f64,
    /// Median query time (ms)
    pub p50_ms: f64,
    /// 95th percentile (ms)
    pub p95_ms: f64,
    /// 99th percentile (ms)
    pub p99_ms: f64,
    /// Maximum query time (ms)
    pub max_ms: f64,
    /// Standard deviation (ms)
    pub std_dev_ms: f64,
}

// ============================================================================
// Audit Log Data Structure (from StarRocks audit table)
// ============================================================================

/// Audit log record from starrocks_audit_db__.starrocks_audit_tbl__
#[derive(Debug, Clone)]
pub struct AuditLogRecord {
    pub query_id: String,
    pub user: String,
    pub db: String,
    pub stmt: String,
    pub query_type: String,
    pub query_time_ms: i64,
    pub state: String,
    pub timestamp: String,
}

// ============================================================================
// Baseline Calculator
// ============================================================================

/// Baseline calculator from audit logs
pub struct BaselineCalculator {
    /// Minimum sample size required for reliable baseline
    pub min_sample_size: usize,
}

impl BaselineCalculator {
    pub fn new() -> Self {
        Self { min_sample_size: 30 }
    }

    /// Calculate baseline for a specific query complexity
    pub fn calculate(&self, records: &[AuditLogRecord]) -> Option<PerformanceBaseline> {
        if records.is_empty() {
            return None;
        }

        let complexity = if let Some(first) = records.first() {
            QueryComplexity::from_sql(&first.stmt)
        } else {
            QueryComplexity::Simple
        };

        let mut times: Vec<f64> = records
            .iter()
            .filter(|r| r.state == "EOF" || r.state == "OK")
            .map(|r| r.query_time_ms as f64)
            .collect();

        if times.len() < self.min_sample_size {
            return None;
        }

        times.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let stats = self.compute_stats(&times);

        Some(PerformanceBaseline {
            complexity,
            stats,
            sample_size: times.len(),
            time_range_hours: 168,
        })
    }

    /// Calculate baselines grouped by query complexity
    pub fn calculate_by_complexity(
        &self,
        records: &[AuditLogRecord],
    ) -> HashMap<QueryComplexity, PerformanceBaseline> {
        let mut grouped: HashMap<QueryComplexity, Vec<AuditLogRecord>> = HashMap::new();

        for record in records {
            let complexity = QueryComplexity::from_sql(&record.stmt);
            grouped.entry(complexity).or_default().push(record.clone());
        }

        grouped
            .into_iter()
            .filter_map(|(complexity, records)| {
                self.calculate(&records)
                    .map(|baseline| (complexity, baseline))
            })
            .collect()
    }

    /// Calculate baseline for specific table (based on table name pattern in SQL)
    pub fn calculate_for_table(
        &self,
        records: &[AuditLogRecord],
        table_name: &str,
    ) -> Option<PerformanceBaseline> {
        let filtered: Vec<AuditLogRecord> = records
            .iter()
            .filter(|r| r.stmt.to_uppercase().contains(&table_name.to_uppercase()))
            .cloned()
            .collect();

        self.calculate(&filtered)
    }

    /// Compute statistical metrics from sorted time series
    fn compute_stats(&self, times: &[f64]) -> BaselineStats {
        if times.is_empty() {
            return BaselineStats::default();
        }

        let sum: f64 = times.iter().sum();
        let avg = sum / times.len() as f64;

        let p50_idx = (times.len() as f64 * 0.5) as usize;
        let p95_idx = (times.len() as f64 * 0.95) as usize;
        let p99_idx = (times.len() as f64 * 0.99) as usize;

        let p50 = times
            .get(p50_idx.min(times.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p95 = times
            .get(p95_idx.min(times.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let p99 = times
            .get(p99_idx.min(times.len() - 1))
            .copied()
            .unwrap_or(0.0);
        let max = times.last().copied().unwrap_or(0.0);

        let variance = times.iter().map(|t| (t - avg).powi(2)).sum::<f64>() / times.len() as f64;
        let std_dev = variance.sqrt();

        BaselineStats {
            avg_ms: avg,
            p50_ms: p50,
            p95_ms: p95,
            p99_ms: p99,
            max_ms: max,
            std_dev_ms: std_dev,
        }
    }
}

impl Default for BaselineCalculator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Adaptive Threshold Calculator
// ============================================================================

/// Adaptive threshold calculator using historical baseline
pub struct AdaptiveThresholdCalculator {
    /// Baseline performance data
    baselines: HashMap<QueryComplexity, PerformanceBaseline>,
}

impl AdaptiveThresholdCalculator {
    pub fn new(baselines: HashMap<QueryComplexity, PerformanceBaseline>) -> Self {
        Self { baselines }
    }

    /// Get query time threshold based on complexity
    /// Returns threshold in milliseconds
    ///
    /// Strategy: Use P95 of historical baseline + 2 std_dev as threshold
    pub fn get_query_time_threshold(&self, complexity: QueryComplexity) -> f64 {
        if let Some(baseline) = self.baselines.get(&complexity) {
            let threshold = baseline.stats.p95_ms + 2.0 * baseline.stats.std_dev_ms;

            let min_threshold = match complexity {
                QueryComplexity::Simple => 5_000.0,
                QueryComplexity::Medium => 10_000.0,
                QueryComplexity::Complex => 30_000.0,
                QueryComplexity::VeryComplex => 60_000.0,
            };

            threshold.max(min_threshold)
        } else {
            self.get_default_threshold(complexity)
        }
    }

    /// Get skew threshold based on historical baseline
    /// Returns max/avg ratio threshold
    ///
    /// Strategy: If historical P99/P50 ratio is high, allow more skew
    pub fn get_skew_threshold(&self, complexity: QueryComplexity) -> f64 {
        if let Some(baseline) = self.baselines.get(&complexity) {
            let historical_ratio = if baseline.stats.p50_ms > 0.0 {
                baseline.stats.p99_ms / baseline.stats.p50_ms
            } else {
                2.0
            };

            2.0 + (historical_ratio - 2.0) * 0.2
        } else {
            2.0
        }
    }

    /// Fallback default thresholds
    fn get_default_threshold(&self, complexity: QueryComplexity) -> f64 {
        match complexity {
            QueryComplexity::Simple => 10_000.0,
            QueryComplexity::Medium => 30_000.0,
            QueryComplexity::Complex => 60_000.0,
            QueryComplexity::VeryComplex => 180_000.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_complexity_detection() {
        let sql1 = "SELECT * FROM users WHERE id = 1";
        assert_eq!(QueryComplexity::from_sql(sql1), QueryComplexity::Simple);

        let sql2 = "SELECT u.name, o.amount FROM users u JOIN orders o ON u.id = o.user_id";
        assert_eq!(QueryComplexity::from_sql(sql2), QueryComplexity::Medium);

        let sql3 = r#"
            SELECT u.name, SUM(o.amount) OVER (PARTITION BY u.id) 
            FROM users u 
            JOIN orders o ON u.id = o.user_id
            JOIN products p ON o.product_id = p.id
            WHERE p.price > 100
        "#;
        assert_eq!(QueryComplexity::from_sql(sql3), QueryComplexity::Complex);

        let sql4 = r#"
            WITH sales AS (
                SELECT user_id, SUM(amount) as total FROM orders GROUP BY user_id
            )
            SELECT u.name, s.total, RANK() OVER (ORDER BY s.total DESC)
            FROM users u 
            JOIN sales s ON u.id = s.user_id
            JOIN (SELECT * FROM products WHERE active = 1) p ON true
            UNION
            SELECT name, 0, 0 FROM inactive_users
        "#;
        assert_eq!(QueryComplexity::from_sql(sql4), QueryComplexity::VeryComplex);
    }

    #[test]
    fn test_query_complexity_edge_cases() {
        assert_eq!(QueryComplexity::from_sql(""), QueryComplexity::Simple);

        let sql = "SELECT * FROM a JOIN b ON a.id = b.id";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Medium);

        let sql =
            "SELECT * FROM a JOIN b ON a.id = b.id JOIN c ON b.id = c.id JOIN d ON c.id = d.id";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Complex);

        let sql = "SELECT id, ROW_NUMBER() OVER (ORDER BY id) FROM t";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);
    }

    #[test]
    fn test_query_complexity_ignores_strings() {
        let sql = "SELECT * FROM t WHERE name = 'JOIN this event'";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = "SELECT * FROM t WHERE desc LIKE '%UNION%'";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = "SELECT * FROM t WHERE note = 'with regards'";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = r#"SELECT * FROM t WHERE msg = 'SELECT JOIN UNION WITH OVER'"#;
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);
    }

    #[test]
    fn test_query_complexity_ignores_comments() {
        let sql = "SELECT * FROM t -- JOIN this later";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = "SELECT * FROM t /* UNION ALL */ WHERE id > 0";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = r#"
            -- This query uses JOIN, UNION, WITH CTE
            /* 
             * Very complex query design
             * Uses OVER window functions
             */
            SELECT * FROM simple_table WHERE x > 0
        "#;
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);
    }

    #[test]
    fn test_query_complexity_real_vs_string() {
        let sql = "SELECT * FROM a JOIN b ON a.id = b.id WHERE name = 'JOIN event'";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Medium);

        let sql = "SELECT * FROM t1 WHERE x = 'UNION' UNION SELECT * FROM t2";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);
    }

    #[test]
    fn test_query_complexity_distinct_aggregation() {
        let sql = "SELECT COUNT(DISTINCT user_id) FROM orders";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);

        let sql = "SELECT COUNT(DISTINCT user_id), SUM(DISTINCT amount) FROM orders GROUP BY date";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(
            complexity == QueryComplexity::Complex || complexity == QueryComplexity::VeryComplex
        );
    }

    #[test]
    fn test_query_complexity_order_by() {
        let sql = "SELECT * FROM orders ORDER BY created_at LIMIT 100";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = "SELECT * FROM orders ORDER BY created_at";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Simple);
    }

    #[test]
    fn test_query_complexity_lateral_explode() {
        let sql = "SELECT id, tag FROM t LATERAL VIEW EXPLODE(tags) tmp AS tag";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);

        let sql = "SELECT * FROM t, UNNEST(arr) AS x";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);
    }

    #[test]
    fn test_query_complexity_exists() {
        let sql =
            "SELECT * FROM orders o WHERE EXISTS (SELECT 1 FROM users u WHERE u.id = o.user_id)";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);

        let sql = "SELECT * FROM orders WHERE NOT EXISTS (SELECT 1 FROM refunds WHERE refunds.order_id = orders.id)";
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);
    }

    #[test]
    fn test_query_complexity_single_join_not_simple() {
        let sql = "SELECT * FROM orders o JOIN users u ON o.user_id = u.id";
        assert_ne!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Medium);
    }

    #[test]
    fn test_query_complexity_production_examples() {
        let sql = "SELECT date, SUM(amount) FROM orders WHERE date >= '2025-01-01' GROUP BY date";
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::Simple);

        let sql = r#"
            SELECT u.name, COUNT(*) as order_count, SUM(o.amount) as total
            FROM users u
            JOIN orders o ON u.id = o.user_id
            JOIN products p ON o.product_id = p.id
            WHERE o.date >= '2025-01-01'
            GROUP BY u.name
        "#;
        let complexity = QueryComplexity::from_sql(sql);
        assert!(complexity == QueryComplexity::Medium || complexity == QueryComplexity::Complex);

        let sql = r#"
            WITH daily_sales AS (
                SELECT date, SUM(amount) as total
                FROM orders
                GROUP BY date
            )
            SELECT date, total, 
                   AVG(total) OVER (ORDER BY date ROWS 7 PRECEDING) as ma7
            FROM daily_sales
        "#;
        let complexity = QueryComplexity::from_sql(sql);
        assert!(
            complexity == QueryComplexity::Complex || complexity == QueryComplexity::VeryComplex
        );

        let sql = r#"
            SELECT 
                region,
                product_category,
                COUNT(DISTINCT user_id) as unique_users,
                SUM(amount) as total_sales,
                RANK() OVER (PARTITION BY region ORDER BY SUM(amount) DESC) as rank
            FROM orders o
            JOIN users u ON o.user_id = u.id
            JOIN products p ON o.product_id = p.id
            WHERE o.date BETWEEN '2025-01-01' AND '2025-12-31'
            GROUP BY region, product_category
            HAVING COUNT(*) > 100
        "#;
        assert_eq!(QueryComplexity::from_sql(sql), QueryComplexity::VeryComplex);
    }

    #[test]
    fn test_baseline_calculation() {
        let calculator = BaselineCalculator::new();

        let records = vec![AuditLogRecord {
            query_id: "1".to_string(),
            user: "test".to_string(),
            db: "db1".to_string(),
            stmt: "SELECT * FROM t1".to_string(),
            query_type: "Query".to_string(),
            query_time_ms: 100,
            state: "EOF".to_string(),
            timestamp: "2025-12-08 10:00:00".to_string(),
        }];

        assert!(calculator.calculate(&records).is_none());

        let mut records = Vec::new();
        for i in 0..50 {
            records.push(AuditLogRecord {
                query_id: i.to_string(),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM t1".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 100 + (i as i64) * 10,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        let baseline = calculator.calculate(&records).unwrap();
        assert_eq!(baseline.sample_size, 50);
        assert!(baseline.stats.avg_ms > 0.0);
        assert!(baseline.stats.p95_ms > baseline.stats.p50_ms);
        assert!(baseline.stats.p99_ms >= baseline.stats.p95_ms);
    }

    #[test]
    fn test_baseline_stats_calculation() {
        let calculator = BaselineCalculator::new();

        let mut records = Vec::new();
        for i in 0..100 {
            records.push(AuditLogRecord {
                query_id: i.to_string(),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM t1".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 1000 + (i as i64) * 100,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        let baseline = calculator.calculate(&records).unwrap();

        assert!(baseline.stats.avg_ms > 5000.0 && baseline.stats.avg_ms < 7000.0);

        assert!(baseline.stats.p50_ms > 5000.0 && baseline.stats.p50_ms < 7000.0);

        assert!(baseline.stats.p95_ms > 9000.0 && baseline.stats.p95_ms < 11000.0);

        assert!(baseline.stats.std_dev_ms > 0.0);
    }

    #[test]
    fn test_baseline_filters_failed_queries() {
        let calculator = BaselineCalculator::new();

        let mut records = Vec::new();

        for i in 0..30 {
            records.push(AuditLogRecord {
                query_id: format!("success_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM t1".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 1000,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        for i in 0..20 {
            records.push(AuditLogRecord {
                query_id: format!("failed_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM t1".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 50000,
                state: "ERROR".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        let baseline = calculator.calculate(&records).unwrap();

        assert_eq!(baseline.sample_size, 30);

        assert!((baseline.stats.avg_ms - 1000.0).abs() < 1.0);
    }

    #[test]
    fn test_baseline_by_complexity() {
        let calculator = BaselineCalculator::new();

        let mut records = Vec::new();

        for i in 0..35 {
            records.push(AuditLogRecord {
                query_id: format!("simple_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM users".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 500 + (i as i64) * 10,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        for i in 0..35 {
            records.push(AuditLogRecord {
                query_id: format!("medium_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM users u JOIN orders o ON u.id = o.user_id".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 2000 + (i as i64) * 20,
                state: "OK".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        let baselines = calculator.calculate_by_complexity(&records);

        assert!(baselines.contains_key(&QueryComplexity::Simple));
        assert!(baselines.contains_key(&QueryComplexity::Medium));

        let simple_avg = baselines
            .get(&QueryComplexity::Simple)
            .unwrap()
            .stats
            .avg_ms;
        let medium_avg = baselines
            .get(&QueryComplexity::Medium)
            .unwrap()
            .stats
            .avg_ms;
        assert!(simple_avg < medium_avg);
    }

    #[test]
    fn test_baseline_for_table() {
        let calculator = BaselineCalculator::new();

        let mut records = Vec::new();

        for i in 0..35 {
            records.push(AuditLogRecord {
                query_id: format!("users_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM users WHERE id > 0".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 100 + (i as i64) * 5,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        for i in 0..35 {
            records.push(AuditLogRecord {
                query_id: format!("orders_{}", i),
                user: "test".to_string(),
                db: "db1".to_string(),
                stmt: "SELECT * FROM orders WHERE amount > 100".to_string(),
                query_type: "Query".to_string(),
                query_time_ms: 500 + (i as i64) * 10,
                state: "EOF".to_string(),
                timestamp: "2025-12-08 10:00:00".to_string(),
            });
        }

        let users_baseline = calculator.calculate_for_table(&records, "users").unwrap();
        assert_eq!(users_baseline.sample_size, 35);

        let orders_baseline = calculator.calculate_for_table(&records, "orders").unwrap();
        assert_eq!(orders_baseline.sample_size, 35);

        assert!(users_baseline.stats.avg_ms < orders_baseline.stats.avg_ms);
    }

    #[test]
    fn test_adaptive_threshold_calculator() {
        let mut baselines = HashMap::new();

        baselines.insert(
            QueryComplexity::Medium,
            PerformanceBaseline {
                complexity: QueryComplexity::Medium,
                stats: BaselineStats {
                    avg_ms: 5000.0,
                    p50_ms: 4000.0,
                    p95_ms: 8000.0,
                    p99_ms: 12000.0,
                    max_ms: 15000.0,
                    std_dev_ms: 2000.0,
                },
                sample_size: 100,
                time_range_hours: 168,
            },
        );

        let calculator = AdaptiveThresholdCalculator::new(baselines);

        let threshold = calculator.get_query_time_threshold(QueryComplexity::Medium);

        assert!(threshold >= 10000.0);
        assert!(threshold <= 15000.0);

        let simple_threshold = calculator.get_query_time_threshold(QueryComplexity::Simple);

        assert_eq!(simple_threshold, 10_000.0);
    }

    #[test]
    fn test_adaptive_skew_threshold() {
        let mut baselines = HashMap::new();

        baselines.insert(
            QueryComplexity::Medium,
            PerformanceBaseline {
                complexity: QueryComplexity::Medium,
                stats: BaselineStats {
                    avg_ms: 5000.0,
                    p50_ms: 4000.0,
                    p95_ms: 8000.0,
                    p99_ms: 12000.0,
                    max_ms: 15000.0,
                    std_dev_ms: 2000.0,
                },
                sample_size: 100,
                time_range_hours: 168,
            },
        );

        let calculator = AdaptiveThresholdCalculator::new(baselines);

        let skew_threshold = calculator.get_skew_threshold(QueryComplexity::Medium);

        assert!(skew_threshold > 2.0);
        assert!(skew_threshold < 3.0);
    }
}
