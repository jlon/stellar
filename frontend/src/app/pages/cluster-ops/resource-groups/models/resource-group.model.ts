export interface ResourceGroup {
  name: string;
  id: number;
  cpu_weight?: number;
  exclusive_cpu_cores?: number;
  mem_limit?: string;
  big_query_cpu_second_limit?: number;
  big_query_scan_rows_limit?: number;
  big_query_mem_limit?: string;
  concurrency_limit?: number;
  spill_mem_limit_threshold?: string;
  classifiers: Classifier[];
}

export interface Classifier {
  id: number;
  weight: number;
  user?: string;
  role?: string;
  query_type?: string;
  source_ip?: string;
  db?: string;
}

export interface CreateResourceGroupRequest {
  name: string;
  cpu_weight?: number;
  exclusive_cpu_cores?: number;
  mem_limit?: string;
  big_query_cpu_second_limit?: number;
  big_query_scan_rows_limit?: number;
  big_query_mem_limit?: string;
  concurrency_limit?: number;
  spill_mem_limit_threshold?: string;
  classifiers: ClassifierRequest[];
}

export interface UpdateResourceGroupRequest {
  cpu_weight?: number;
  exclusive_cpu_cores?: number;
  mem_limit?: string;
  big_query_cpu_second_limit?: number;
  big_query_scan_rows_limit?: number;
  big_query_mem_limit?: string;
  concurrency_limit?: number;
  spill_mem_limit_threshold?: string;
  add_classifiers?: ClassifierRequest[];
  drop_classifier_ids?: number[];
}

export interface ClassifierRequest {
  user?: string;
  role?: string;
  query_type?: string[];
  source_ip?: string;
  db?: string;
  weight: number;
}

export interface ResourceGroupUsage {
  id: number;
  backend: string;
  be_in_use_cpu_cores: number;
  be_in_use_mem_bytes: number;
  be_running_queries: number;
}

export interface ResourceUsageAnalysis {
  cpu_analysis: UserCpuUsage[];
  memory_analysis: UserMemoryUsage[];
  concurrency_analysis: UserConcurrency[];
}

export interface UserCpuUsage {
  user: string;
  total_cpu_seconds: number;
  cpu_usage_percentage: number;
  suggested_cpu_weight: number;
  suggested_exclusive_cores: number;
}

export interface UserMemoryUsage {
  user: string;
  max_mem_mb: number;
  suggested_mem_limit: string;
  suggested_big_query_mem_limit: string;
}

export interface UserConcurrency {
  user: string;
  max_concurrency_per_second: number;
  suggested_concurrency_limit: number;
}