import { Injectable, inject } from "@angular/core";
import { Observable } from "rxjs";
import { ApiService } from "./api.service";

export interface LoadStage {
  key: string;
  label: string;
  duration_ms: number;
  start_time?: string;
  end_time?: string;
  status: "completed" | "running" | "failed" | string;
}

export interface LoadFailureCause {
  code: string;
  label: string;
  suggestion: string;
  /** 该原因下可执行的修复步骤（引擎参数与核对项）。 */
  steps?: string[];
}

/** 当前状态下可执行的引擎侧处置动作。 */
export interface LoadAction {
  action: string;
  label: string;
  description: string;
  statement: string;
}

export interface LoadActionResponse {
  success: boolean;
  message?: string;
  /** 执行后重新读取的 Routine Load 父作业状态。 */
  state?: string;
}

export interface DorisLoadFailureDetails {
  url?: string;
  error_msg?: string;
  job_details?: string;
}

export interface RoutineLoadTask {
  task_id?: string;
  txn_id?: string;
  txn_status?: string;
  create_time?: string;
  last_scheduled_time?: string;
  execute_start_time?: string;
  be_id?: string;
  data_source_properties?: string;
  message?: string;
}

export interface RoutineLoadDetails {
  /** 父作业状态（引擎 SHOW ALL ROUTINE LOAD 的 State）。 */
  state?: string;
  current_task_num?: number;
  statistics?: string;
  progress?: string;
  timestamp_progress?: string;
  latest_source_position?: string;
  offset_lag?: string;
  reason_of_state_changed?: string;
  error_log_urls?: string;
  tracking_sql?: string;
  other_msg?: string;
  tasks: RoutineLoadTask[];
}

export interface LoadJob {
  job_id?: string;
  label?: string;
  profile_id?: string;
  database?: string;
  table_name?: string;
  user?: string;
  warehouse?: string;
  state: string;
  progress?: string;
  load_type: string;
  priority?: string;
  scan_rows?: number;
  scan_bytes?: number;
  filtered_rows?: number;
  unselected_rows?: number;
  sink_rows?: number;
  create_time?: string;
  load_start_time?: string;
  load_commit_time?: string;
  load_finish_time?: string;
  error_msg?: string;
  tracking_sql?: string;
  rejected_record_path?: string;
  runtime_details?: string;
  properties?: string;
  routine_load?: RoutineLoadDetails;
  doris_failure?: DorisLoadFailureDetails;
  stage_timeline: LoadStage[];
  failure_cause?: LoadFailureCause;
  /** 仅在详情接口按当前状态返回，列表接口为空。 */
  actions?: LoadAction[];
}

export interface LoadSummary {
  running: number;
  queued: number;
  failed: number;
  finished: number;
}

export interface LoadListResponse {
  items: LoadJob[];
  total: number;
  has_more: boolean;
  next_cursor?: string;
  source: string;
  summary: LoadSummary;
}

export interface LoadFilters {
  db?: string;
  type?: string;
  state?: string;
  search?: string;
  range?: "24h" | "7d" | "30d" | "all";
  limit?: number;
  cursor?: string;
}

@Injectable({
  providedIn: "root",
})
export class LoadService {
  private readonly api = inject(ApiService);

  list(filters: LoadFilters = {}): Observable<LoadListResponse> {
    const params: Record<string, string | number> = {
      range: filters.range || "24h",
      limit: filters.limit || 100,
    };
    if (filters.db) params.db = filters.db;
    if (filters.type) params.type = filters.type;
    if (filters.state) params.state = filters.state;
    if (filters.search?.trim()) params.search = filters.search.trim();
    if (filters.cursor) params.cursor = filters.cursor;
    return this.api.get<LoadListResponse>("/clusters/loads", params);
  }

  get(jobId: string, db?: string): Observable<LoadJob> {
    return this.api.get<LoadJob>(
      `/clusters/loads/${encodeURIComponent(jobId)}`,
      db ? { db } : undefined,
    );
  }

  /** 对导入作业执行引擎侧处置动作；作业名与数据库由服务端解析。 */
  executeAction(jobId: string, action: string): Observable<LoadActionResponse> {
    return this.api.post<LoadActionResponse>(
      `/clusters/loads/${encodeURIComponent(jobId)}/actions`,
      { action },
      120000,
    );
  }
}
