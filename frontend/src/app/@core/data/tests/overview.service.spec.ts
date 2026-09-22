import { TestBed } from '@angular/core/testing';

import { ApiService } from '../api.service';
import { ExtendedClusterOverview, OverviewService } from '../overview.service';
import { I18nService } from '../../i18n/i18n.service';

describe('OverviewService', () => {
  let service: OverviewService;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [
        { provide: ApiService, useValue: {} },
        { provide: I18nService, useValue: { instant: (key: string) => key } },
      ],
    });
    service = TestBed.runInInjectionContext(() => new OverviewService());
  });

  it('names Compaction score and applies its documented thresholds', () => {
    const cards = service.transformToHealthCards(overviewWithCompaction(101));
    const compaction = cards.find(card => card.cardId === 'compaction_score');

    expect(compaction).toEqual(jasmine.objectContaining({
      title: '压缩积压分',
      status: 'danger',
    }));
    expect(compaction?.description).toContain('超过 50 需关注，超过 100 为严重积压');
  });

  it('keeps query error rate explicitly scoped to query_err', () => {
    const cards = service.transformToHealthCards(overviewWithCompaction(0));
    const queryError = cards.find(card => card.cardId === 'error_rate');

    expect(queryError).toEqual(jasmine.objectContaining({ title: '查询错误率' }));
    expect(queryError?.description).toContain('查询执行失败 / 总查询');
    expect(queryError?.description).toContain('FE query_err');
    expect(queryError?.description).toContain('不合并');
  });
});

function overviewWithCompaction(compactionScore: number): ExtendedClusterOverview {
  return {
    cluster_id: 1,
    cluster_name: 'test',
    timestamp: '2026-09-22T00:00:00Z',
    health: {
      status: 'healthy',
      score: 96,
      starrocks_version: '3.3',
      be_nodes_online: 3,
      be_nodes_total: 3,
      fe_nodes_online: 3,
      fe_nodes_total: 3,
      compaction_score: compactionScore,
      alerts: [],
    },
    kpi: {
      qps: 1,
      qps_trend: 0,
      p99_latency_ms: 10,
      p99_latency_trend: 0,
      success_rate: 100,
      success_rate_trend: 0,
      error_rate: 0,
    },
    resources: {
      cpu_usage_pct: 10,
      cpu_trend: 0,
      memory_usage_pct: 10,
      memory_trend: 0,
      disk_usage_pct: 10,
      disk_trend: 0,
      compaction_score: compactionScore,
      compaction_status: 'normal',
    },
    performance_trends: emptyPerformanceTrends(),
    resource_trends: emptyResourceTrends(),
    mv_stats: { total: 0, running: 0, success: 0, failed: 0, pending: 0 },
    load_jobs: { running: 0, pending: 0, finished: 0, failed: 0, cancelled: 0 },
    transactions: { running: 0, committed: 0, aborted: 0 },
    schema_changes: { running: 0, pending: 0, finished: 0, failed: 0, cancelled: 0 },
    compaction: { baseCompactionRunning: 0, cumulativeCompactionRunning: 0, maxScore: 0, avgScore: 0, beScores: [] },
    sessions: { active_users_1h: 0, active_users_24h: 0, current_connections: 0, running_queries: [] },
    network_io: { networkTxBytesPerSec: 0, networkRxBytesPerSec: 0, diskReadBytesPerSec: 0, diskWriteBytesPerSec: 0 },
    alerts: [],
  };
}

function emptyPerformanceTrends() {
  return { qps: [], rps: [], latency_p50: [], latency_p95: [], latency_p99: [], error_rate: [] };
}

function emptyResourceTrends() {
  return {
    cpu_usage: [], memory_usage: [], disk_usage: [], jvm_heap_usage: [], network_tx: [], network_rx: [],
    io_read: [], io_write: [], compaction_score: [],
  };
}
