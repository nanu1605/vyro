// Benchmark mode (P20) in words: the areas, how figures read, and comparing two runs within
// their noise (method v1, docs/benchmark.md). A difference counts only beyond the larger of 3%
// and either run's spread.

import type { AreaId, BenchRun, BenchSetup, Metric, RunSummary, SummaryMetric } from './types';

export const NOISE_FLOOR = 0.03;
/** Sustained stability at or above this reads as steady, %. */
export const STEADY_PCT = 97;

export const AREAS: Record<AreaId, { label: string; hint: string }> = {
  cpuSingle: { label: 'CPU single-thread', hint: 'Four fixed kernels on one thread' },
  cpuMulti: { label: 'CPU multi-thread', hint: 'The same kernels on every logical processor' },
  memory: { label: 'Memory', hint: 'Read and copy bandwidth, and latency' },
  storage: { label: 'Storage', hint: 'A 1 GiB uncached file in the temporary folder' },
  gpuCompute: { label: 'GPU compute', hint: 'FP32 multiply-adds and video memory copies' },
  graphics: { label: 'Graphics', hint: 'An offscreen 2560×1440 scene, two frames in flight' },
  sustained: { label: 'Sustained', hint: '10 minutes of CPU and GPU load together' },
  efficiency: { label: 'Efficiency', hint: 'Work per watt' },
  stability: { label: 'Stability', hint: 'Wrong results, hardware errors, driver resets' },
};

export const MODES = {
  quick: { label: 'Quick', what: 'CPU, memory, storage, GPU compute and graphics' },
  full: { label: 'Full', what: 'Quick, plus 10 minutes of CPU and GPU load together' },
} as const;

/** "41,230", "62.4", "2,950", "0.85". */
export function fmtValue(value: number, unit: string): string {
  if (!Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  const digits =
    unit === 'fps/W'
      ? 2
      : unit === '×' || unit === '%' || unit === 'ns'
        ? 1
        : abs >= 100 || unit === '' || unit === 'W' || unit === 'MHz'
          ? 0
          : abs >= 10
            ? 1
            : 2;
  return value.toLocaleString('en-US', { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** "41,230 pts", "62.4 GB/s", "98.6%", "3". */
export function fmtMetric(m: { value: number; unit: string }): string {
  const v = fmtValue(m.value, m.unit);
  if (!m.unit) return v;
  return m.unit === '%' ? `${v}%` : `${v} ${m.unit}`;
}

/** "±1.2%": half the spread, the way the passes scatter around the median. */
export function fmtSpread(spread: number | null): string | null {
  if (spread == null || !Number.isFinite(spread)) return null;
  return `±${((spread / 2) * 100).toFixed(1)}%`;
}

/** "2 min 50 s", "13 min", "45 s". */
export function fmtDuration(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  const r = s % 60;
  if (m >= 10 || r === 0) return `${Math.round(s / 60)} min`;
  return `${m} min ${r} s`;
}

export function stabilityLabel(pct: number): string {
  if (pct >= STEADY_PCT) return 'Steady';
  if (pct >= 90) return 'Some drop';
  return 'Drops under load';
}

export type Verdict = 'faster' | 'slower' | 'same';

export interface CompareRow {
  key: string;
  area: AreaId;
  label: string;
  unit: string;
  value: number;
  base: number;
  /** (this − base) ÷ base. */
  change: number;
  /** The change it has to beat to count. */
  noise: number;
  verdict: Verdict;
}

type Comparable = Pick<Metric, 'key' | 'value' | 'spread'>;

function metricsOf(r: BenchRun | RunSummary): Comparable[] {
  return 'areas' in r ? r.areas.flatMap((a) => a.metrics) : r.metrics;
}

/** One metric against the same metric of another run; `null` when it cannot be compared. */
export function compareMetric(m: Metric, area: AreaId, base: Comparable | undefined): CompareRow | null {
  if (!base || m.better === 'shown' || !(base.value > 0) || !Number.isFinite(m.value)) return null;
  const change = (m.value - base.value) / base.value;
  const noise = Math.max(NOISE_FLOOR, m.spread ?? 0, base.spread ?? 0);
  const up = m.better === 'higher' ? change > 0 : change < 0;
  return {
    key: m.key,
    area,
    label: m.label,
    unit: m.unit,
    value: m.value,
    base: base.value,
    change,
    noise,
    verdict: Math.abs(change) <= noise ? 'same' : up ? 'faster' : 'slower',
  };
}

/** Every comparable metric of `run` against `base`; Sustained against `sustainBase` when given. */
export function compare(
  run: BenchRun,
  base: BenchRun | RunSummary | null,
  sustainBase: BenchRun | RunSummary | null = base,
): CompareRow[] {
  const rows: CompareRow[] = [];
  for (const a of run.areas) {
    const from = a.area === 'sustained' ? sustainBase : base;
    if (!from) continue;
    const theirs = metricsOf(from);
    for (const m of a.metrics) {
      const row = compareMetric(
        m,
        a.area,
        theirs.find((t) => t.key === m.key),
      );
      if (row) rows.push(row);
    }
  }
  return rows;
}

/** The newest earlier completed run of the same method that was not busy (of the given mode,
 * when asked). */
export function baselineFor(
  run: { id: number; ts: number; method: number },
  runs: RunSummary[],
  mode?: 'quick' | 'full',
): RunSummary | null {
  return (
    runs
      .filter(
        (r) =>
          r.id !== run.id &&
          (r.ts < run.ts || (r.ts === run.ts && r.id < run.id)) &&
          r.method === run.method &&
          r.status === 'completed' &&
          !r.busy &&
          (!mode || r.mode === mode),
      )
      .sort((a, b) => b.ts - a.ts || b.id - a.id)[0] ?? null
  );
}

/** "Within noise of the earlier run", "2 faster, 1 slower". */
export function summaryText(rows: CompareRow[]): string {
  if (!rows.length) return 'Nothing to compare';
  const faster = rows.filter((r) => r.verdict === 'faster').length;
  const slower = rows.filter((r) => r.verdict === 'slower').length;
  if (!faster && !slower) return 'All within noise';
  const parts = [faster && `${faster} faster`, slower && `${slower} slower`].filter(Boolean);
  const same = rows.length - faster - slower;
  return `${parts.join(', ')}${same ? `, ${same} within noise` : ''}`;
}

/** "faster", "slower", "better", "worse", "within noise": Sustained and Efficiency are better or
 * worse rather than faster or slower. */
export function verdictText(row: Pick<CompareRow, 'key' | 'verdict'>): string {
  if (row.verdict === 'same') return 'within noise';
  const quality = row.key.startsWith('sustain.') || row.key.startsWith('eff.');
  if (row.verdict === 'faster') return quality ? 'better' : 'faster';
  return quality ? 'worse' : 'slower';
}

/** "+4.2%" / "−3.0%". */
export function fmtChange(change: number): string {
  const pct = (change * 100).toFixed(1);
  return change >= 0 ? `+${pct}%` : `−${pct.slice(1)}%`;
}

const SETUP: [keyof BenchSetup, string][] = [
  ['cpu', 'CPU'],
  ['gpu', 'GPU'],
  ['gpuDriver', 'GPU driver'],
  ['ramGb', 'RAM (GB)'],
  ['ramMts', 'RAM speed (MT/s)'],
  ['board', 'Board'],
  ['bios', 'BIOS'],
  ['windows', 'Windows'],
  ['powerPlan', 'Power plan'],
  ['profile', 'Vyro profile'],
  ['fanProfile', 'Fan settings'],
  ['drive', 'Drive tested'],
  ['vyro', 'Vyro'],
];

export function setupRows(s: BenchSetup): { label: string; value: string }[] {
  return SETUP.flatMap(([k, label]) => {
    const v = s[k];
    return v == null || v === '' ? [] : [{ label, value: String(v) }];
  });
}

/** What differs between two runs' setups: "GPU driver 576.02 → 577.00". */
export function setupChanges(now: BenchSetup, before: BenchSetup): { label: string; from: string; to: string }[] {
  return SETUP.flatMap(([k, label]) => {
    const a = before[k];
    const b = now[k];
    if (a == null || b == null || String(a) === String(b)) return [];
    return [{ label, from: String(a), to: String(b) }];
  });
}

/** A run's headline figure for the list, by key. */
export function headline(r: { metrics: SummaryMetric[] } | BenchRun, key: string): number | null {
  const all = 'areas' in r ? r.areas.flatMap((a) => a.metrics) : r.metrics;
  return all.find((m) => m.key === key)?.value ?? null;
}
