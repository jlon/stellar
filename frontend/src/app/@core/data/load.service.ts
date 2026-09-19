import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { ApiService } from './api.service';

export interface LoadStage {
  key: string;
  label: string;
  duration_ms: number;
  start_time?: string;
  end_time?: string;
  status: 'completed' | 'running' | 'failed' | string;
}

export interface LoadFailureCause {
  code: string;
  label: string;
  suggestion: string;
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
  stage_timeline: LoadStage[];
  failure_cause?: LoadFailureCause;
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
  source: string;
  summary: LoadSummary;
}

export interface LoadFilters {
  db?: string;
  type?: string;
  state?: string;
  search?: string;
  range?: '24h' | '7d' | '30d' | 'all';
  limit?: number;
}

@Injectable({
  providedIn: 'root',
})
export class LoadService {
  private readonly api = inject(ApiService);

  list(filters: LoadFilters = {}): Observable<LoadListResponse> {
    const params: Record<string, string | number> = {
      range: filters.range || '24h',
      limit: filters.limit || 100,
    };
    if (filters.db) params.db = filters.db;
    if (filters.type) params.type = filters.type;
    if (filters.state) params.state = filters.state;
    if (filters.search?.trim()) params.search = filters.search.trim();
    return this.api.get<LoadListResponse>('/clusters/loads', params);
  }

  get(jobId: string, db?: string): Observable<LoadJob> {
    return this.api.get<LoadJob>(`/clusters/loads/${encodeURIComponent(jobId)}`, db ? { db } : undefined);
  }
}
