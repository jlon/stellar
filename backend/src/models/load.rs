use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 导入任务查询条件。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct LoadQueryParams {
    /// 数据库名称。
    #[serde(alias = "database")]
    pub db: Option<String>,
    /// 导入类型，例如 STREAM_LOAD、ROUTINE_LOAD。
    #[serde(rename = "type")]
    pub load_type: Option<String>,
    /// 引擎原始状态。
    pub state: Option<String>,
    /// 按作业 ID、Label、表名或错误信息搜索。
    pub search: Option<String>,
    /// 时间范围：24h、7d、30d 或 all。
    pub range: Option<String>,
    /// 返回条数，服务端限制在 1..=500。
    pub limit: Option<u32>,
}

/// 导入任务阶段。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoadStage {
    pub key: String,
    pub label: String,
    pub duration_ms: u64,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    /// completed、running 或 failed。
    pub status: String,
}

/// 失败原因分类。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoadFailureCause {
    pub code: String,
    pub label: String,
    pub suggestion: String,
}

/// 统一导入任务 DTO。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoadJob {
    pub job_id: Option<String>,
    pub label: Option<String>,
    pub profile_id: Option<String>,
    pub database: Option<String>,
    pub table_name: Option<String>,
    pub user: Option<String>,
    pub warehouse: Option<String>,
    pub state: String,
    pub progress: Option<String>,
    pub load_type: String,
    pub priority: Option<String>,
    pub scan_rows: Option<u64>,
    pub scan_bytes: Option<u64>,
    pub filtered_rows: Option<u64>,
    pub unselected_rows: Option<u64>,
    pub sink_rows: Option<u64>,
    pub create_time: Option<String>,
    pub load_start_time: Option<String>,
    pub load_commit_time: Option<String>,
    pub load_finish_time: Option<String>,
    pub error_msg: Option<String>,
    pub tracking_sql: Option<String>,
    pub rejected_record_path: Option<String>,
    /// 引擎返回的原始 JSON 或文本，不对字段做猜测。
    pub runtime_details: Option<String>,
    pub properties: Option<String>,
    pub stage_timeline: Vec<LoadStage>,
    pub failure_cause: Option<LoadFailureCause>,
}

/// 导入任务概览统计。
#[derive(Debug, Clone, Default, Serialize, ToSchema)]
pub struct LoadSummary {
    pub running: u32,
    pub queued: u32,
    pub failed: u32,
    pub finished: u32,
}

/// 导入任务列表响应。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoadListResponse {
    pub items: Vec<LoadJob>,
    pub total: usize,
    pub has_more: bool,
    pub source: String,
    pub summary: LoadSummary,
}
