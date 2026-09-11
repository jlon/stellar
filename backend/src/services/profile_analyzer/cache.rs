use std::time::{Duration, Instant};

use dashmap::DashMap;

use super::models::ProfileAnalysisResponse;

const DEFAULT_TTL: Duration = Duration::from_secs(30 * 60);
const DEFAULT_MAX_ENTRIES: usize = 32;

struct CachedAnalysis {
    response: ProfileAnalysisResponse,
    cached_at: Instant,
}

pub struct ProfileAnalysisCache {
    entries: DashMap<(i64, String), CachedAnalysis>,
    ttl: Duration,
    max_entries: usize,
}

impl ProfileAnalysisCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            ttl: DEFAULT_TTL,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }

    pub fn get(&self, cluster_id: i64, query_id: &str) -> Option<ProfileAnalysisResponse> {
        let key = (cluster_id, query_id.to_string());
        if let Some(entry) = self.entries.get(&key) {
            if entry.cached_at.elapsed() > self.ttl {
                drop(entry);
                self.entries.remove(&key);
                return None;
            }
            return Some(entry.response.clone());
        }
        None
    }

    pub fn insert(&self, cluster_id: i64, query_id: String, mut response: ProfileAnalysisResponse) {
        response.llm_analysis = None;
        let key = (cluster_id, query_id);
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&key) {
            self.evict_oldest();
        }
        self.entries.insert(
            key,
            CachedAnalysis { response, cached_at: Instant::now() },
        );
    }

    pub fn invalidate(&self, cluster_id: i64, query_id: &str) {
        self.entries.remove(&(cluster_id, query_id.to_string()));
    }

    fn evict_oldest(&self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|entry| entry.value().cached_at)
            .map(|entry| entry.key().clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}

impl Default for ProfileAnalysisCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::profile_analyzer::models::ProfileAnalysisResponse;
    use std::collections::HashMap;

    fn sample(conclusion: &str) -> ProfileAnalysisResponse {
        ProfileAnalysisResponse {
            hotspots: vec![],
            conclusion: conclusion.to_string(),
            suggestions: vec![],
            performance_score: 80.0,
            execution_tree: None,
            summary: None,
            diagnostics: vec![],
            aggregated_diagnostics: vec![],
            node_diagnostics: HashMap::new(),
            profile_content: Some("raw-profile".to_string()),
            fragments: vec![],
            root_cause_analysis: None,
            llm_analysis: None,
        }
    }

    fn cache_with(ttl: Duration, max_entries: usize) -> ProfileAnalysisCache {
        ProfileAnalysisCache { entries: DashMap::new(), ttl, max_entries }
    }

    #[test]
    fn test_get_miss_then_hit() {
        let cache = ProfileAnalysisCache::new();
        assert!(cache.get(3, "q1").is_none());
        cache.insert(3, "q1".to_string(), sample("first"));
        let hit = cache.get(3, "q1").expect("cache hit");
        assert_eq!(hit.conclusion, "first");
        assert_eq!(hit.profile_content.as_deref(), Some("raw-profile"));
        assert!(cache.get(3, "q2").is_none());
        assert!(cache.get(1, "q1").is_none());
    }

    #[test]
    fn test_insert_strips_llm_analysis() {
        let cache = ProfileAnalysisCache::new();
        let mut response = sample("llm");
        response.llm_analysis = Some(crate::services::profile_analyzer::LLMEnhancedAnalysis {
            available: true,
            status: "pending".to_string(),
            ..Default::default()
        });
        cache.insert(3, "q1".to_string(), response);
        let hit = cache.get(3, "q1").expect("cache hit");
        assert!(hit.llm_analysis.is_none());
    }

    #[test]
    fn test_invalidate_and_expire() {
        let cache = cache_with(Duration::from_millis(5), 8);
        cache.insert(3, "q1".to_string(), sample("stale"));
        cache.invalidate(3, "q1");
        assert!(cache.get(3, "q1").is_none());

        cache.insert(3, "q1".to_string(), sample("fresh"));
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get(3, "q1").is_none());
    }

    #[test]
    fn test_evicts_oldest_when_full() {
        let cache = cache_with(Duration::from_secs(60), 2);
        cache.insert(3, "q1".to_string(), sample("one"));
        cache.insert(3, "q2".to_string(), sample("two"));
        cache.insert(3, "q3".to_string(), sample("three"));
        assert!(cache.get(3, "q1").is_none());
        assert_eq!(cache.get(3, "q2").expect("q2").conclusion, "two");
        assert_eq!(cache.get(3, "q3").expect("q3").conclusion, "three");
    }
}
