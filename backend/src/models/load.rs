use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 导入任务查询条件。
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct LoadQueryParams {
    /// 详情接口内部使用的精确作业 ID，不接受外部 Query 参数。
    #[serde(skip)]
    pub job_id: Option<String>,
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
    /// 上一页末尾任务的位置，由服务端返回，不透明处理。
    pub cursor: Option<String>,
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

/// Doris `SHOW LOAD` 为已选失败作业返回的原始诊断字段。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DorisLoadFailureDetails {
    pub url: Option<String>,
    pub error_msg: Option<String>,
    pub job_details: Option<String>,
}

/// Routine Load 父作业的实时消费状态。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RoutineLoadDetails {
    pub current_task_num: Option<u32>,
    pub statistics: Option<String>,
    pub progress: Option<String>,
    pub timestamp_progress: Option<String>,
    pub latest_source_position: Option<String>,
    pub offset_lag: Option<String>,
    pub reason_of_state_changed: Option<String>,
    pub error_log_urls: Option<String>,
    pub tracking_sql: Option<String>,
    pub other_msg: Option<String>,
    pub tasks: Vec<RoutineLoadTask>,
}

/// Routine Load 当前子任务。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RoutineLoadTask {
    pub task_id: Option<String>,
    pub txn_id: Option<String>,
    pub txn_status: Option<String>,
    pub create_time: Option<String>,
    pub last_scheduled_time: Option<String>,
    pub execute_start_time: Option<String>,
    pub be_id: Option<String>,
    pub data_source_properties: Option<String>,
    pub message: Option<String>,
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
    /// Routine Load 父作业的实时位点和子任务；仅在详情接口按需查询。
    pub routine_load: Option<RoutineLoadDetails>,
    /// Doris 失败作业的原始 `SHOW LOAD` 诊断字段；仅在详情接口按需查询。
    pub doris_failure: Option<DorisLoadFailureDetails>,
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
    pub next_cursor: Option<String>,
    pub source: String,
    pub summary: LoadSummary,
}
