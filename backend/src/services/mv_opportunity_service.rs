use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::config::AuditLogConfig;
use crate::models::{
    Cluster, ClusterType, MaterializedViewOpportunity, MaterializedViewOpportunityResponse,
    MaterializedViewRef,
};
use crate::services::MySQLPoolManager;
use crate::services::audit_log_service::{
    AuditLogService, AuditQuerySample, audit_query_user_message,
};
use crate::utils::{ApiError, ApiResult};

const DEFAULT_WINDOW_HOURS: i64 = 24;
const MAX_WINDOW_HOURS: i64 = 168;
const MAX_AUDIT_SAMPLES: usize = 10_000;
const MAX_CANDIDATES: usize = 20;
const MIN_EXECUTIONS: u64 = 3;
const MAX_SQL_BYTES: usize = 4_096;
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_CACHE_ENTRIES: usize = 256;

static SQL_COMMENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)/\*.*?\*/|--[^\r\n]*").expect("SQL comment regex is valid"));
static SQL_STRING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"'(?:''|[^'])*'").expect("SQL string literal regex is valid"));
static SQL_NUMBER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d+(?:\.\d+)?\b").expect("SQL number regex is valid"));
static SQL_WHITESPACE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+").expect("SQL whitespace regex is valid"));
static COMPLEX_QUERY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:with|join|union|intersect|except)\b|\bfrom\s*\(")
        .expect("SQL complexity regex is valid")
});
static GROUP_BY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bgroup\s+by\b").expect("GROUP BY regex is valid"));
static AGGREGATE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:count|sum|min|max|avg)\s*\(").expect("aggregate regex is valid")
});
static FROM_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\bfrom\s+((?:`[^`]+`|[A-Za-z_][A-Za-z0-9_$]*)(?:\s*\.\s*(?:`[^`]+`|[A-Za-z_][A-Za-z0-9_$]*)){0,2})",
    )
    .expect("FROM identifier regex is valid")
});
static IMPLICIT_JOIN_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^\s*(?:as\s+)?(?:`[^`]+`|[A-Za-z_][A-Za-z0-9_$]*)?\s*,")
        .expect("implicit join regex is valid")
});

static OPPORTUNITY_CACHE: Lazy<OpportunityCache> = Lazy::new(OpportunityCache::default);

#[derive(Default)]
struct OpportunityCache {
    entries: Mutex<HashMap<(i64, i64), CachedOpportunityResponse>>,
}

struct CachedOpportunityResponse {
    response: MaterializedViewOpportunityResponse,
    expires_at: Instant,
}

impl OpportunityCache {
    fn get(
        &self,
        cluster_id: i64,
        window_hours: i64,
    ) -> Option<MaterializedViewOpportunityResponse> {
        let mut entries = self.entries.lock().ok()?;
        let key = (cluster_id, window_hours);
        let entry = entries.get(&key)?;
        if entry.expires_at > Instant::now() {
            return Some(entry.response.clone());
        }
        entries.remove(&key);
        None
    }

    fn put(
        &self,
        cluster_id: i64,
        window_hours: i64,
        response: MaterializedViewOpportunityResponse,
    ) {
        if let Ok(mut entries) = self.entries.lock() {
            let now = Instant::now();
            entries.retain(|_, entry| entry.expires_at > now);
            let key = (cluster_id, window_hours);
            if !entries.contains_key(&key) && entries.len() >= MAX_CACHE_ENTRIES {
                if let Some(oldest_key) = entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.expires_at)
                    .map(|(key, _)| *key)
                {
                    entries.remove(&oldest_key);
                }
            }
            entries
                .insert(key, CachedOpportunityResponse { response, expires_at: now + CACHE_TTL });
        }
    }
}

pub struct MaterializedViewOpportunityService {
    audit_log: AuditLogService,
}

impl MaterializedViewOpportunityService {
    pub fn new(mysql_pool_manager: Arc<MySQLPoolManager>, audit_config: AuditLogConfig) -> Self {
        Self { audit_log: AuditLogService::new(mysql_pool_manager, audit_config) }
    }

    pub async fn discover(
        &self,
        cluster: &Cluster,
        requested_hours: Option<i64>,
        force_refresh: bool,
    ) -> ApiResult<MaterializedViewOpportunityResponse> {
        let window_hours = requested_hours
            .unwrap_or(DEFAULT_WINDOW_HOURS)
            .clamp(1, MAX_WINDOW_HOURS);
        if !force_refresh {
            if let Some(response) = OPPORTUNITY_CACHE.get(cluster.id, window_hours) {
                return Ok(response);
            }
        }
        let observed_at = Utc::now();

        if cluster.cluster_type != ClusterType::StarRocks {
            let response = MaterializedViewOpportunityResponse {
                supported: false,
                engine: cluster.cluster_type.display_name().to_string(),
                source: "unavailable".to_string(),
                observed_at,
                window_hours,
                sampled_query_count: 0,
                truncated: false,
                candidates: Vec::new(),
                warnings: vec!["当前只支持 StarRocks 异步物化视图的工作负载机会识别。".to_string()],
            };
            OPPORTUNITY_CACHE.put(cluster.id, window_hours, response.clone());
            return Ok(response);
        }

        let samples = self
            .audit_log
            .get_starrocks_query_samples(cluster, window_hours, MAX_AUDIT_SAMPLES)
            .await
            .map_err(|error| {
                ApiError::bad_gateway(audit_query_user_message(
                    &error,
                    &self.audit_log.audit_table_name(cluster),
                ))
            })?;
        let sampled_query_count = samples.samples.len();
        let (candidates, result_truncated) = build_opportunities(samples.samples);
        let truncated = samples.truncated || result_truncated;
        let mut warnings = Vec::new();
        if samples.truncated {
            warnings.push(format!(
                "审计样本已限制为最近 {} 条查询，结果只覆盖该范围。",
                MAX_AUDIT_SAMPLES
            ));
        }
        if result_truncated {
            warnings.push(format!("机会列表仅显示累计耗时最高的 {} 项。", MAX_CANDIDATES));
        }

        let response = MaterializedViewOpportunityResponse {
            supported: true,
            engine: cluster.cluster_type.display_name().to_string(),
            source: "starrocks_audit_log".to_string(),
            observed_at,
            window_hours,
            sampled_query_count,
            truncated,
            candidates,
            warnings,
        };
        OPPORTUNITY_CACHE.put(cluster.id, window_hours, response.clone());
        Ok(response)
    }
}

#[derive(Debug)]
struct OpportunitySample {
    sql_pattern: String,
    source_database: String,
    source_table: String,
}

#[derive(Debug)]
struct OpportunityAccumulator {
    sql_pattern: String,
    source_database: String,
    source_table: String,
    durations: Vec<u64>,
    first_seen: String,
    last_seen: String,
}

pub(crate) fn build_opportunities(
    samples: Vec<AuditQuerySample>,
) -> (Vec<MaterializedViewOpportunity>, bool) {
    let mut grouped: HashMap<String, OpportunityAccumulator> = HashMap::new();

    for sample in samples {
        let Some(opportunity) = parse_opportunity_sample(&sample) else {
            continue;
        };
        let key = format!(
            "{}.{}:{}",
            opportunity.source_database.to_ascii_lowercase(),
            opportunity.source_table.to_ascii_lowercase(),
            opportunity.sql_pattern.to_ascii_lowercase(),
        );
        let entry = grouped
            .entry(key)
            .or_insert_with(|| OpportunityAccumulator {
                sql_pattern: opportunity.sql_pattern,
                source_database: opportunity.source_database,
                source_table: opportunity.source_table,
                durations: Vec::new(),
                first_seen: sample.timestamp.clone(),
                last_seen: sample.timestamp.clone(),
            });
        entry.durations.push(sample.duration_ms);
        if sample.timestamp < entry.first_seen {
            entry.first_seen = sample.timestamp.clone();
        }
        if sample.timestamp > entry.last_seen {
            entry.last_seen = sample.timestamp;
        }
    }

    let mut candidates = grouped
        .into_values()
        .filter(|entry| entry.durations.len() as u64 >= MIN_EXECUTIONS)
        .map(|mut entry| {
            entry.durations.sort_unstable();
            let execution_count = entry.durations.len() as u64;
            let total_duration_ms = entry.durations.iter().copied().sum::<u64>();
            let p95_index = (entry.durations.len() * 95).div_ceil(100).saturating_sub(1);
            MaterializedViewOpportunity {
                sql_pattern: entry.sql_pattern,
                source_database: entry.source_database,
                source_table: entry.source_table,
                execution_count,
                total_duration_ms,
                average_duration_ms: total_duration_ms / execution_count,
                p95_duration_ms: entry.durations[p95_index],
                first_seen: entry.first_seen,
                last_seen: entry.last_seen,
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .total_duration_ms
            .cmp(&left.total_duration_ms)
            .then_with(|| right.execution_count.cmp(&left.execution_count))
            .then_with(|| left.sql_pattern.cmp(&right.sql_pattern))
    });
    let truncated = candidates.len() > MAX_CANDIDATES;
    candidates.truncate(MAX_CANDIDATES);
    (candidates, truncated)
}

fn parse_opportunity_sample(sample: &AuditQuerySample) -> Option<OpportunitySample> {
    if sample.stmt.len() > MAX_SQL_BYTES {
        return None;
    }
    let sql_pattern = redact_sql_pattern(&sample.stmt)?;
    let normalized = sql_pattern.trim_end_matches(';').trim();
    if normalized.is_empty()
        || normalized.contains(';')
        || !normalized.to_ascii_lowercase().starts_with("select ")
        || COMPLEX_QUERY_RE.is_match(normalized)
        || !GROUP_BY_RE.is_match(normalized)
        || !AGGREGATE_RE.is_match(normalized)
    {
        return None;
    }

    let captures = FROM_RE.captures_iter(normalized).collect::<Vec<_>>();
    if captures.len() != 1 {
        return None;
    }
    let source_capture = captures[0].get(1)?;
    if IMPLICIT_JOIN_RE.is_match(&normalized[source_capture.end()..]) {
        return None;
    }
    let source = source_capture.as_str();
    let (source_database, source_table) = parse_source_table(source, &sample.database)?;
    Some(OpportunitySample { sql_pattern, source_database, source_table })
}

fn redact_sql_pattern(sql: &str) -> Option<String> {
    let without_comments = SQL_COMMENT_RE.replace_all(sql, " ");
    let without_strings = SQL_STRING_RE.replace_all(&without_comments, "?");
    let without_numbers = SQL_NUMBER_RE.replace_all(&without_strings, "?");
    let pattern = SQL_WHITESPACE_RE
        .replace_all(&without_numbers, " ")
        .trim()
        .to_string();
    (!pattern.is_empty()).then_some(pattern)
}

fn parse_source_table(source: &str, fallback_database: &str) -> Option<(String, String)> {
    let parts = source
        .split('.')
        .map(|part| part.trim().trim_matches('`').replace("``", "`"))
        .collect::<Vec<_>>();
    let (database, table) = match parts.as_slice() {
        [table] => (fallback_database.trim().to_string(), table.clone()),
        [database, table] => (database.clone(), table.clone()),
        // The existing controlled creation API intentionally has no catalog input.
        [_catalog, _database, _table] => return None,
        _ => return None,
    };
    MaterializedViewRef::validate_identifier("source database", &database).ok()?;
    MaterializedViewRef::validate_identifier("source table", &table).ok()?;
    Some((database, table))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> MaterializedViewOpportunityResponse {
        MaterializedViewOpportunityResponse {
            supported: true,
            engine: "StarRocks".to_string(),
            source: "starrocks_audit_log".to_string(),
            observed_at: Utc::now(),
            window_hours: 24,
            sampled_query_count: 1,
            truncated: false,
            candidates: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn opportunity_cache_isolates_cluster_and_window() {
        let cache = OpportunityCache::default();
        cache.put(1, 24, response());

        assert!(cache.get(1, 24).is_some());
        assert!(cache.get(2, 24).is_none());
        assert!(cache.get(1, 72).is_none());
    }

    #[test]
    fn opportunity_cache_limits_entries() {
        let cache = OpportunityCache::default();
        for cluster_id in 0..=MAX_CACHE_ENTRIES as i64 {
            cache.put(cluster_id, 24, response());
        }

        assert_eq!(cache.entries.lock().unwrap().len(), MAX_CACHE_ENTRIES);
    }
}
