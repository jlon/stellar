/** 复测序列的最小结构（来自 GET /clusters/profiles/:id/retest）。 */
export interface RetestRunLike {
  QueryId: string;
  TimeMs: number;
  IsBaseline: boolean;
}

export function formatDurationMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(2)}s`;
  return `${(ms / 60000).toFixed(2)}m`;
}

/**
 * 诊断那次与最近一次执行的耗时对比。
 *
 * 没有新的执行时如实说明，不编造结论；差异只作呈现，不断言因果。
 */
export function retestDeltaLabel(runs: RetestRunLike[]): string {
  if (!runs || runs.length < 2) {
    return "";
  }
  const baseline = runs.find((run) => run.IsBaseline) || runs[0];
  const latest = runs[runs.length - 1];
  if (baseline.QueryId === latest.QueryId) {
    return "尚无新的执行记录：让业务重跑该查询后点刷新即可对比";
  }
  const from = Number(baseline.TimeMs) || 0;
  const to = Number(latest.TimeMs) || 0;
  const delta = to - from;
  const arrow = delta <= 0 ? "↓" : "↑";
  const percent = from > 0 ? Math.round((Math.abs(delta) / from) * 100) : 0;
  return `诊断那次 ${formatDurationMs(from)} → 最近一次 ${formatDurationMs(to)}（${arrow} ${percent}%）`;
}
