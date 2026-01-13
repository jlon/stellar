use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceGroup {
    pub name: String,
    pub id: i64,
    pub cpu_weight: Option<i32>,
    pub exclusive_cpu_cores: Option<i32>,
    pub mem_limit: Option<String>,
    pub big_query_cpu_second_limit: Option<i64>,
    pub big_query_scan_rows_limit: Option<i64>,
    pub big_query_mem_limit: Option<String>,
    pub concurrency_limit: Option<i32>,
    pub spill_mem_limit_threshold: Option<String>,
    pub classifiers: Vec<Classifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Classifier {
    pub id: i64,
    pub weight: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateResourceGroupRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_weight: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_cpu_cores: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_cpu_second_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_scan_rows_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_mem_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill_mem_limit_threshold: Option<String>,
    #[serde(default)]
    pub classifiers: Vec<ClassifierRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateResourceGroupRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_weight: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclusive_cpu_cores: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_cpu_second_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_scan_rows_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub big_query_mem_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill_mem_limit_threshold: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add_classifiers: Option<Vec<ClassifierRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_classifier_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClassifierRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_type: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db: Option<String>,
    #[serde(default = "default_weight")]
    pub weight: i32,
}

fn default_weight() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceGroupUsage {
    pub id: i64,
    pub backend: String,
    pub be_in_use_cpu_cores: f64,
    pub be_in_use_mem_bytes: i64,
    pub be_running_queries: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ResourceUsageAnalysis {
    pub cpu_analysis: Vec<UserCpuUsage>,
    pub memory_analysis: Vec<UserMemoryUsage>,
    pub concurrency_analysis: Vec<UserConcurrency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserCpuUsage {
    pub user: String,
    pub total_cpu_seconds: f64,
    pub cpu_usage_percentage: f64,
    pub suggested_cpu_weight: i32,
    pub suggested_exclusive_cores: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserMemoryUsage {
    pub user: String,
    pub max_mem_mb: f64,
    pub suggested_mem_limit: String,
    pub suggested_big_query_mem_limit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserConcurrency {
    pub user: String,
    pub max_concurrency_per_second: f64,
    pub suggested_concurrency_limit: i32,
}
