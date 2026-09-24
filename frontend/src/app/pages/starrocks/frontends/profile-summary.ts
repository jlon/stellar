// The backend accepts only a reviewed async-profiler renderer and numeric frame commands.
// Extract bounded evidence, never forward executable HTML or raw cpool strings to an AI provider.
export interface MemoryProfileHotspot {
  name: string;
  samples: number;
  percentage: number;
}

export interface MemoryProfileSummary {
  rootSamples: number;
  stackDepth: number;
  frameCount: number;
  hotspots: MemoryProfileHotspot[];
  comparisonCandidates: MemoryProfileHotspot[];
}

export type MemoryProfileHotspotChangeKind = 'rising' | 'falling' | 'new' | 'not-ranked' | 'steady';

export interface MemoryProfileHotspotChange {
  name: string;
  baselinePercentage: number | null;
  comparisonPercentage: number | null;
  percentagePointDelta: number | null;
  kind: MemoryProfileHotspotChangeKind;
}

export interface MemoryProfileComparison {
  baseline: Pick<MemoryProfileSummary, 'rootSamples' | 'stackDepth' | 'frameCount'>;
  comparison: Pick<MemoryProfileSummary, 'rootSamples' | 'stackDepth' | 'frameCount'>;
  hasCoverageChange: boolean;
  changes: MemoryProfileHotspotChange[];
}

const comparisonCandidateLimit = 32;
const comparisonChangeLimit = 8;
const steadyPercentagePointThreshold = 1;

export function parseMemoryProfileSummary(html: string): MemoryProfileSummary | null {
  const data = html.match(/const cpool = \[([\s\S]*?)\];\s*unpack\(cpool\);([\s\S]*?)search\(\);/);
  if (!data) return null;

  const names: string[] = [];
  const entries = /'((?:\\.|[^'\\\r\n])*)'/g;
  for (const match of data[1].matchAll(entries)) {
    const raw = match[1];
    if (raw.replace(/\\(['\\])/g, '').includes('\\')) return null;
    const value = raw.replace(/\\(['\\])/g, '$1');
    const prefix = names.length ? value.charCodeAt(0) - 32 : 0;
    if (names.length && (prefix < 0 || prefix > names[names.length - 1].length)) return null;
    names.push(names.length ? names[names.length - 1].slice(0, prefix) + value.slice(1) : value);
  }
  if (!names.length) return null;

  let width = 0;
  let level = 0;
  let rootWidth = 0;
  let frames = 0;
  let maxLevel = 0;
  const hottest = new Map<string, number>();
  const calls = /(?:^|\n)([nuf])\((\d+(?:\.\d+)?(?:,\s*-?\d+(?:\.\d+)?){0,6})\)/g;
  for (const match of data[2].matchAll(calls)) {
    const [key, ...args] = match[2].split(',').map(Number);
    if (match[1] === 'f') {
      level = args[0];
      width = args[2] || width;
    } else {
      level += match[1] === 'u' ? 1 : 0;
      width = args[0] || width;
    }
    if (level < 0 || level >= 1024) return null;
    maxLevel = Math.max(maxLevel, level);
    if (frames === 0) {
      if (match[1] !== 'n' || !width) return null;
      rootWidth = width;
    }
    frames++;
    const name = names[key >>> 3]?.trim();
    if (level > 0 && name && name.length <= 120 && /^[a-zA-Z0-9_.$@/()+\[\]<>:,-]+$/.test(name)) {
      hottest.set(name, Math.max(hottest.get(name) || 0, width));
    }
  }
  if (!rootWidth || !frames || !hottest.size) return null;
  const ranked = [...hottest].sort((a, b) => b[1] - a[1]);
  const top = [
    ...ranked.filter(([name]) => name.startsWith('com/starrocks/')).slice(0, 5),
    ...ranked.filter(([name]) => !name.startsWith('com/starrocks/')).slice(0, 3),
  ];
  return {
    rootSamples: rootWidth,
    stackDepth: maxLevel + 1,
    frameCount: frames,
    hotspots: top.map(([name, samples]) => ({ name, samples, percentage: samples / rootWidth * 100 })),
    comparisonCandidates: ranked.slice(0, comparisonCandidateLimit).map(([name, samples]) => ({
      name,
      samples,
      percentage: samples / rootWidth * 100,
    })),
  };
}

export function compareMemoryProfileSummaries(
  baseline: MemoryProfileSummary,
  comparison: MemoryProfileSummary,
): MemoryProfileComparison {
  const baselineCandidates = new Map(baseline.comparisonCandidates.map((hotspot) => [hotspot.name, hotspot]));
  const comparisonCandidates = new Map(comparison.comparisonCandidates.map((hotspot) => [hotspot.name, hotspot]));
  const allChanges = [...new Set([...baselineCandidates.keys(), ...comparisonCandidates.keys()])]
    .map((name): MemoryProfileHotspotChange => {
      const before = baselineCandidates.get(name);
      const after = comparisonCandidates.get(name);
      if (!before) {
        return {
          name,
          baselinePercentage: null,
          comparisonPercentage: after!.percentage,
          percentagePointDelta: null,
          kind: 'new',
        };
      }
      if (!after) {
        return {
          name,
          baselinePercentage: before.percentage,
          comparisonPercentage: null,
          percentagePointDelta: null,
          kind: 'not-ranked',
        };
      }
      const percentagePointDelta = after.percentage - before.percentage;
      return {
        name,
        baselinePercentage: before.percentage,
        comparisonPercentage: after.percentage,
        percentagePointDelta,
        kind: Math.abs(percentagePointDelta) <= steadyPercentagePointThreshold
          ? 'steady'
          : percentagePointDelta > 0 ? 'rising' : 'falling',
      };
    })
    .sort((left, right) => {
      const priority = comparisonChangePriority(left) - comparisonChangePriority(right);
      return priority || comparisonChangeMagnitude(right) - comparisonChangeMagnitude(left);
    });

  return {
    baseline: profileComparisonStats(baseline),
    comparison: profileComparisonStats(comparison),
    hasCoverageChange: allChanges.some((change) => change.kind === 'rising' || change.kind === 'falling'),
    changes: allChanges.slice(0, comparisonChangeLimit),
  };
}

function comparisonChangePriority(change: MemoryProfileHotspotChange): number {
  return change.kind === 'rising' || change.kind === 'falling' ? 0 : 1;
}

function comparisonChangeMagnitude(change: MemoryProfileHotspotChange): number {
  return change.percentagePointDelta === null
    ? Math.max(change.baselinePercentage ?? 0, change.comparisonPercentage ?? 0)
    : Math.abs(change.percentagePointDelta);
}

function profileComparisonStats(summary: MemoryProfileSummary): Pick<MemoryProfileSummary, 'rootSamples' | 'stackDepth' | 'frameCount'> {
  return {
    rootSamples: summary.rootSamples,
    stackDepth: summary.stackDepth,
    frameCount: summary.frameCount,
  };
}

export function summarizeMemoryProfile(html: string): string | null {
  const summary = parseMemoryProfileSummary(html);
  if (!summary) return null;
  return formatMemoryProfileSummary(summary);
}

export function formatMemoryProfileSummary(summary: MemoryProfileSummary): string {
  return `FE 内存分配采样：根帧 ${summary.rootSamples} 个样本，${summary.frameCount} 个帧。StarRocks 帧最多 5 项，其余最多 3 项（各自按最大单帧宽度排序；相对根帧样本数，非真实内存字节；父子帧不可相加）：\n${summary.hotspots.map(({ name, samples }) => `- ${JSON.stringify(name)}: ${samples}/${summary.rootSamples} 样本`).join('\n')}`;
}
