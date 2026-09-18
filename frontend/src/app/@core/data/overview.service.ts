import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { I18nService } from '../i18n/i18n.service';
import { ApiService } from './api.service';

export interface ClusterOverview {
  clusterId: number;
  clusterName: string;
  timestamp: string;
  healthCards: HealthCard[];
  performanceTrends: PerformanceTrends;
  resourceTrends: ResourceTrends;
  dataStatistics: DataStatistics;
  capacityPrediction: CapacityPrediction;
}

export interface HealthCard {
  title: string;
  value: string | number;
  status: 'success' | 'warning' | 'danger' | 'info';
  trend?: number; // positive = up, negative = down
  unit?: string;
  icon?: string;
  navigateTo?: string;
  description?: string; // Tooltip description for the metric
  cardId?: string; // Unique identifier for special cards (latency, disk, etc.)
  tier?: 'hero' | 'aux';
}

export interface PerformanceTrends {
  qps: TimeSeriesPoint[];
  rps: TimeSeriesPoint[];
  latency_p50: TimeSeriesPoint[];
  latency_p95: TimeSeriesPoint[];
  latency_p99: TimeSeriesPoint[];
  error_rate: TimeSeriesPoint[];
  timeout_rate?: TimeSeriesPoint[];
}

export interface ResourceTrends {
  cpu_usage: TimeSeriesPoint[];
  memory_usage: TimeSeriesPoint[];
  disk_usage: TimeSeriesPoint[];
  jvm_heap_usage: TimeSeriesPoint[];
  network_tx: TimeSeriesPoint[];
  network_rx: TimeSeriesPoint[];
  io_read: TimeSeriesPoint[];
  io_write: TimeSeriesPoint[];
  compaction_score: TimeSeriesPoint[];
  backend_alive?: TimeSeriesPoint[];
  frontend_alive?: TimeSeriesPoint[];
  tablet_count?: TimeSeriesPoint[];
  jvm_thread_count?: TimeSeriesPoint[];
  txn_success?: TimeSeriesPoint[];
  txn_failed?: TimeSeriesPoint[];
}

export interface TimeSeriesPoint {
  timestamp: string;
  value: number;
}

export interface DataStatistics {
  databaseCount: number;
  tableCount: number;
  totalDataSizeBytes: number;
  topTablesBySize: TopTableBySize[];
  topTablesByAccess: TopTableByAccess[];
  accessError?: string;
  mvTotal: number;
  mvRunning: number;
  mvFailed: number;
  mvSuccess: number;
  schemaChangeRunning: number;
  schemaChangePending: number;
  schemaChangeFinished: number;
  schemaChangeFailed: number;
  activeUsers1h: number;
  activeUsers24h: number;
}

export interface TopTableBySize {
  database: string;
  table: string;
  sizeBytes: number;
  rowCount?: number;
}

export interface TopTableByAccess {
  database: string;
  table: string;
  accessCount: number;
  lastAccess: string;
  uniqueUsers: number;
}

export interface CapacityPrediction {
  disk_total_bytes: number;
  disk_used_bytes: number;
  disk_usage_pct: number;
  daily_growth_bytes: number;
  days_until_full?: number;
  predicted_full_date?: string;
  growth_trend: string; // "increasing", "stable", "decreasing"
  real_data_size_bytes: number; // Real data size from information_schema (stored in object storage)
}

// Extended Cluster Overview (All 18 modules)
export interface ExtendedClusterOverview {
  cluster_id: number;
  cluster_name: string;
  deployment_mode?: 'shared_nothing' | 'shared_data';
  timestamp: string;
  health: ClusterHealth;
  kpi: KeyPerformanceIndicators;
  resources: ResourceMetrics;
  performance_trends: PerformanceTrends;
  resource_trends: ResourceTrends;
  data_stats?: DataStatistics;
  mv_stats: MaterializedViewStats;
  load_jobs: LoadJobStats;
  transactions: TransactionStats;
  schema_changes: SchemaChangeStats;
  compaction: CompactionStats;
  sessions: SessionStats;
  network_io: NetworkIOStats;
  capacity?: CapacityPrediction;
  alerts: Alert[];
  meta_log_count?: number;
  unfinished_query?: number;
  safe_mode?: boolean;
}

export interface ClusterHealth {
  status: 'healthy' | 'warning' | 'critical';
  score: number; // 0-100
  starrocks_version: string; // StarRocks version
  be_nodes_online: number;
  be_nodes_total: number;
  fe_nodes_online: number;
  fe_nodes_total: number;
  compaction_score: number;
  alerts: string[];
}

export interface KeyPerformanceIndicators {
  qps: number;
  qps_trend: number;
  p99_latency_ms: number;
  p99_latency_trend: number;
  success_rate: number;
  success_rate_trend: number;
  error_rate: number;
}

export interface ResourceMetrics {
  cpu_usage_pct: number;
  cpu_trend: number;
  memory_usage_pct: number;
  memory_trend: number;
  disk_usage_pct: number;
  disk_trend: number;
  compaction_score: number;
  compaction_status: string; // "normal", "warning", "critical"
}

export interface MaterializedViewStats {
  total: number;
  running: number;
  success: number;
  failed: number;
  pending: number;
}

export interface LoadJobStats {
  running: number;
  pending: number;
  finished: number;
  failed: number;
  cancelled: number;
}

export interface TransactionStats {
  running: number;
  committed: number;
  aborted: number;
}

export interface SchemaChangeStats {
  running: number;
  pending: number;
  finished: number;
  failed: number;
  cancelled: number;
}

// Compaction Stats for Storage-Compute Separation Architecture
// Reference: https://forum.mirrorship.cn/t/topic/13256
// In shared-data mode:
// - Compaction is scheduled by FE at Partition level
// - No distinction between base/cumulative compaction
// - Score is per-partition, not per-BE
export interface CompactionStats {
  baseCompactionRunning: number;           // Always 0 in shared-data mode
  cumulativeCompactionRunning: number;     // Total compaction tasks running
  maxScore: number;                        // Max compaction score across all partitions (from FE)
  avgScore: number;                        // Same as maxScore in shared-data mode
  beScores: BECompactionScore[];           // Empty in shared-data mode
}

export interface BECompactionScore {
  beId: number;
  beHost: string;
  score: number;
}

// Compaction Detail Stats for Storage-Compute Separation Architecture
// Provides detailed compaction monitoring including:
// - Top 10 partitions by compaction score
// - Running and finished task statistics
// - Duration statistics (min, max, avg)
export interface CompactionDetailStats {
  topPartitions: TopPartitionByScore[];
  taskStats: CompactionTaskStats;
  durationStats: CompactionDurationStats;
  nodeDisks?: NodeDiskUsage[];
}

export interface NodeDiskUsage {
  host: string;
  usedPct: number;
}

export interface TopPartitionByScore {
  dbName: string;
  tableName: string;
  partitionName: string;
  maxScore: number;
  avgScore: number;
  p50Score: number;
}

export interface CompactionTaskStats {
  runningCount: number;
  finishedCount: number;
  totalCount: number;
}

export interface CompactionDurationStats {
  minDurationMs: number;
  maxDurationMs: number;
  avgDurationMs: number;
}

export interface SessionStats {
  active_users_1h: number;
  active_users_24h: number;
  current_connections: number;
  running_queries: RunningQuery[];
}

export interface RunningQuery {
  queryId: string;
  user: string;
  database: string;
  startTime: string;
  durationMs: number;
  state: string;
  queryPreview: string;
}

export interface NetworkIOStats {
  networkTxBytesPerSec: number;
  networkRxBytesPerSec: number;
  diskReadBytesPerSec: number;
  diskWriteBytesPerSec: number;
}

export interface Alert {
  level: 'critical' | 'warning' | 'info';
  category: string;
  message: string;
  timestamp: string;
  action?: string;
}

@Injectable({
  providedIn: 'root',
})
export class OverviewService {
  private api = inject(ApiService);
  private i18n = inject(I18nService);


  /**
   * Format bytes to human-readable size with adaptive unit
   * 自适应单位显示：大于1024T显示P，大于1024G显示T，以此类推
   */
  formatBytes(bytes: number): { value: string; unit: string } {
    if (bytes === 0) return { value: '0', unit: 'B' };
    
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
    const k = 1024;
    
    // Find the appropriate unit
    let unitIndex = 0;
    let value = bytes;
    
    while (value >= k && unitIndex < units.length - 1) {
      value /= k;
      unitIndex++;
    }
    
    // Format value: show 1 decimal place for values < 10, otherwise round
    const formattedValue = value < 10 ? value.toFixed(1) : Math.round(value).toString();
    
    return {
      value: formattedValue,
      unit: units[unitIndex]
    };
  }

  getClusterOverview(clusterId: number, timeRange: string = '1h'): Observable<ClusterOverview> {
    return this.api.get(`/clusters/${clusterId}/overview`, { time_range: timeRange });
  }

  getHealthCards(clusterId: number): Observable<HealthCard[]> {
    return this.api.get(`/clusters/${clusterId}/overview/health`);
  }

  getPerformanceTrends(clusterId: number, timeRange: string = '1h'): Observable<PerformanceTrends> {
    return this.api.get(`/clusters/${clusterId}/overview/performance`, { time_range: timeRange });
  }

  getResourceTrends(clusterId: number, timeRange: string = '1h'): Observable<ResourceTrends> {
    return this.api.get(`/clusters/${clusterId}/overview/resources`, { time_range: timeRange });
  }

  getDataStatistics(clusterId: number): Observable<DataStatistics> {
    return this.api.get(`/clusters/${clusterId}/overview/data-stats`);
  }

  getActiveDataStatistics(timeRange: string = '1h'): Observable<DataStatistics> {
    return this.api.get(`/clusters/overview/data-stats`, { time_range: timeRange });
  }

  getCapacityPrediction(clusterId: number): Observable<CapacityPrediction> {
    return this.api.get(`/clusters/${clusterId}/overview/capacity-prediction`);
  }

  getExtendedClusterOverview(timeRange: string = '24h'): Observable<ExtendedClusterOverview> {
    return this.api.get(`/clusters/overview/extended`, { time_range: timeRange });
  }

  /**
   * Get compaction detail statistics for storage-compute separation architecture
   * 
   * @param timeRange Time range for task statistics: 1h, 6h, 24h, 3d (default: 1h)
   * @returns CompactionDetailStats including:
   *   - Top 10 partitions by compaction score
   *   - Running and finished task counts
   *   - Duration statistics (min, max, avg)
   */
  getCompactionDetailStats(timeRange: string = '1h'): Observable<CompactionDetailStats> {
    return this.api.get(`/clusters/overview/compaction-details`, { time_range: timeRange });
  }

  /**
   * Transform ExtendedClusterOverview to HealthCard[] for display
   * Converts backend data structure to frontend card format
   */
  transformToHealthCards(overview: ExtendedClusterOverview): HealthCard[] {
    const sharedData = overview.deployment_mode === 'shared_data';
    const diskPct = overview.capacity?.disk_usage_pct ?? overview.resources.disk_usage_pct ?? 0;
    const days = overview.capacity?.days_until_full;
    const diskFull = diskPct >= 99.5 || days === 0;
    const daysValue = diskFull ? '0' : days == null ? '稳定' : String(days);
    const daysUnit = diskFull || days != null ? '天' : '';
    const daysStatus = diskFull || (days != null && days < 30)
      ? 'danger'
      : days != null && days < 90
        ? 'warning'
        : 'success';
    const metaLog = overview.meta_log_count ?? 0;
    const healthStatus = overview.health.status === 'critical'
      ? 'danger'
      : overview.health.status === 'warning'
        ? 'warning'
        : 'success';
    return [
      {
        title: this.i18n.instant('状态'),
        value: Math.round(overview.health.score).toString(),
        status: healthStatus,
        cardId: 'health',
      },
      {
        title: 'FE',
        value: `${overview.health.fe_nodes_online}/${overview.health.fe_nodes_total}`,
        status: this.nodePairStatus(overview.health.fe_nodes_online, overview.health.fe_nodes_total),
        navigateTo: '/pages/starrocks/frontends',
        description: this.i18n.instant('查看 FE 节点'),
        cardId: 'fe',
      },
      {
        title: sharedData ? 'CN' : 'BE',
        value: `${overview.health.be_nodes_online}/${overview.health.be_nodes_total}`,
        status: this.nodePairStatus(overview.health.be_nodes_online, overview.health.be_nodes_total),
        navigateTo: '/pages/starrocks/backends',
        description: this.i18n.instant('查看计算节点'),
        cardId: 'be',
      },
      {
        title: 'Score',
        value: Math.round(overview.resources.compaction_score).toString(),
        status: overview.resources.compaction_score > 100 ? 'warning' : 'success',
        description: '查看 Compaction 任务',
        cardId: 'compaction_score',
      },
      {
        title: 'P99',
        value: Math.round(overview.kpi.p99_latency_ms).toString(),
        unit: 'ms',
        status: overview.kpi.p99_latency_ms < 1000 ? 'success' :
                overview.kpi.p99_latency_ms < 5000 ? 'warning' : 'danger',
        navigateTo: '/pages/starrocks/queries/audit-logs',
        description: this.i18n.instant('查看查询审计日志与执行耗时'),
        cardId: 'p99',
      },
      {
        title: this.i18n.instant('错误率'),
        value: (overview.kpi.error_rate || 0).toFixed(1),
        unit: '%',
        status: overview.kpi.error_rate > 5 ? 'warning' : 'success',
        navigateTo: '/pages/starrocks/queries/audit-logs',
        description: this.i18n.instant('查看查询审计日志与执行状态'),
        cardId: 'error_rate',
      },
      {
        title: this.i18n.instant(sharedData ? '缓存' : '磁盘'),
        value: Math.round(diskPct).toString(),
        unit: '%',
        // shared-data 的本地盘是数据缓存配额：写满是 LRU 淘汰的稳态，不是容量事故
        status: sharedData ? 'info' : diskPct > 90 ? 'danger' : diskPct > 80 ? 'warning' : 'success',
        navigateTo: '/pages/starrocks/backends',
        description: this.i18n.instant(sharedData ? '查看计算节点数据缓存配额使用情况（写满后按 LRU 淘汰）' : '查看计算节点磁盘使用情况'),
        cardId: 'disk',
      },
      {
        title: this.i18n.instant('距存满'),
        value: sharedData ? '不适用' : daysValue,
        unit: sharedData ? '' : daysUnit,
        // 数据在对象存储：本地缓存配额推不出「距存满」
        status: sharedData ? 'info' : daysStatus,
        navigateTo: '/pages/starrocks/backends',
        description: this.i18n.instant(sharedData ? '存算分离集群的数据位于对象存储，本地盘不承载数据' : '查看计算节点容量'),
        cardId: 'days_full',
      },
      {
        title: 'Meta Log',
        value: String(metaLog),
        status: metaLog > 100000 ? 'danger' : metaLog > 50000 ? 'warning' : 'success',
        cardId: 'meta_log',
      },
    ];
  }

  /**
   * Transform ExtendedClusterOverview to DataStatistics
   */
  transformDataStatistics(overview: ExtendedClusterOverview): DataStatistics {
    return this.mapDataStatistics(overview.data_stats);
  }

  mapDataStatistics(raw: any): DataStatistics {
    return {
      databaseCount: raw?.database_count ?? raw?.databaseCount ?? 0,
      tableCount: raw?.table_count ?? raw?.tableCount ?? 0,
      totalDataSizeBytes: raw?.total_data_size ?? raw?.totalDataSizeBytes ?? 0,
      mvTotal: raw?.mv_total ?? raw?.mvTotal ?? 0,
      mvRunning: raw?.mv_running ?? raw?.mvRunning ?? 0,
      mvSuccess: raw?.mv_success ?? raw?.mvSuccess ?? 0,
      mvFailed: raw?.mv_failed ?? raw?.mvFailed ?? 0,
      schemaChangeRunning: raw?.schema_change_running ?? raw?.schemaChangeRunning ?? 0,
      schemaChangePending: raw?.schema_change_pending ?? raw?.schemaChangePending ?? 0,
      schemaChangeFinished: raw?.schema_change_finished ?? raw?.schemaChangeFinished ?? 0,
      schemaChangeFailed: raw?.schema_change_failed ?? raw?.schemaChangeFailed ?? 0,
      activeUsers1h: raw?.active_users_1h ?? raw?.activeUsers1h ?? 0,
      activeUsers24h: raw?.active_users_24h ?? raw?.activeUsers24h ?? 0,
      topTablesBySize: (raw?.top_tables_by_size || raw?.topTablesBySize || []).map((t: any) => ({
        database: t.database,
        table: t.table,
        sizeBytes: t.size_bytes ?? t.sizeBytes,
        rowCount: t.rows ?? t.rowCount
      })),
      topTablesByAccess: (raw?.top_tables_by_access || raw?.topTablesByAccess || []).map((t: any) => ({
        database: t.database,
        table: t.table,
        accessCount: t.access_count ?? t.accessCount,
        lastAccess: t.last_access ?? t.lastAccess,
        uniqueUsers: t.unique_users ?? t.uniqueUsers ?? 0
      })),
      accessError: raw?.access_error || raw?.accessError || undefined
    };
  }

  private nodePairStatus(online: number, total: number): HealthCard['status'] {
    if (total <= 0) {
      return 'warning';
    }
    return online === total ? 'success' : 'danger';
  }
}
