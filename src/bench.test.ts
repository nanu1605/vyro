import { describe, expect, it } from 'vitest';
import {
  baselineFor,
  compare,
  compareMetric,
  fmtChange,
  fmtDuration,
  fmtMetric,
  fmtSpread,
  fmtValue,
  headline,
  setupChanges,
  setupRows,
  stabilityLabel,
  summaryText,
  verdictText,
} from './bench';
import type { BenchRun, Metric, RunSummary } from './types';

const metric = (key: string, value: number, spread: number | null, better: Metric['better'] = 'higher', unit = 'pts'): Metric => ({
  key, label: key, unit, value, spread, passes: [], better,
});

function run(id: number, ts: number, metrics: Metric[], extra: Partial<BenchRun> = {}): BenchRun {
  return {
    id, ts, method: 1, mode: 'quick', status: 'completed', reason: null, secs: 130, busy: false, notes: [],
    setup: { vyro: '0.4.0', cpu: 'CPU', cores: 8, threads: 16, gpu: 'GPU', gpuDriver: '576.02', ramGb: 32, ramMts: 6000, board: null,
      bios: '1.A0', windows: null, powerPlan: 'Balanced', profile: 'Benchmark', fanProfile: 'Benchmark', drive: null, busyPct: 2 },
    areas: [{ area: 'cpuMulti', metrics, watch: null, missing: null, detail: null, loops: [] }],
    errors: { calc: 0, storage: 0, hardware: 0, gpuResets: 0 },
    ...extra,
  };
}

const summary = (id: number, ts: number, extra: Partial<RunSummary> = {}): RunSummary => ({
  id, ts, method: 1, mode: 'quick', status: 'completed', busy: false, errors: 0, metrics: [], ...extra,
});

describe('benchmark figures', () => {
  it('formats values by unit', () => {
    expect(fmtMetric(metric('a', 41230.4, null))).toBe('41,230 pts');
    expect(fmtMetric(metric('a', 62.44, null, 'higher', 'GB/s'))).toBe('62.4 GB/s');
    expect(fmtMetric(metric('a', 98.44, null, 'higher', '%'))).toBe('98.4%');
    expect(fmtMetric(metric('a', 3, null, 'shown', ''))).toBe('3');
    expect(fmtValue(1.176, 'fps/W')).toBe('1.18');
    expect(fmtValue(Number.NaN, 'pts')).toBe('—');
    expect(fmtSpread(0.024)).toBe('±1.2%');
    expect(fmtSpread(null)).toBeNull();
    expect(fmtChange(0.0521)).toBe('+5.2%');
    expect(fmtChange(-0.03)).toBe('−3.0%');
  });

  it('formats durations and stability', () => {
    expect(fmtDuration(45)).toBe('45 s');
    expect(fmtDuration(131)).toBe('2 min 11 s');
    expect(fmtDuration(731)).toBe('12 min');
    expect(fmtDuration(180)).toBe('3 min');
    expect(stabilityLabel(98.4)).toBe('Steady');
    expect(stabilityLabel(93)).toBe('Some drop');
    expect(stabilityLabel(80)).toBe('Drops under load');
  });
});

describe('comparing runs', () => {
  it('counts a change only beyond the noise', () => {
    const base = metric('cpu.multi', 40000, 0.01);
    expect(compareMetric(metric('cpu.multi', 40800, 0.01), 'cpuMulti', base)?.verdict).toBe('same');
    expect(compareMetric(metric('cpu.multi', 42000, 0.01), 'cpuMulti', base)?.verdict).toBe('faster');
    // A noisy run widens the band.
    expect(compareMetric(metric('cpu.multi', 42000, 0.08), 'cpuMulti', base)?.verdict).toBe('same');
    // Lower is better for latency.
    const lat = compareMetric(metric('mem.latency', 70, 0.005, 'lower', 'ns'), 'memory', metric('mem.latency', 80, 0.005, 'lower', 'ns'));
    expect(lat?.verdict).toBe('faster');
    expect(lat && verdictText(lat)).toBe('faster');
    expect(verdictText({ key: 'sustain.stability', verdict: 'slower' })).toBe('worse');
    expect(verdictText({ key: 'eff.cpu', verdict: 'faster' })).toBe('better');
    // Shown-only figures and missing baselines are not compared.
    expect(compareMetric(metric('cpu.scaling', 10, null, 'shown'), 'cpuMulti', metric('cpu.scaling', 9, null, 'shown'))).toBeNull();
    expect(compareMetric(metric('cpu.multi', 1, null), 'cpuMulti', undefined)).toBeNull();
  });

  it('compares a run with a summary and sums it up', () => {
    const now = run(5, 500, [metric('cpu.multi', 44000, 0.01), metric('cpu.scaling', 10, null, 'shown')]);
    const before = summary(4, 400, { metrics: [{ key: 'cpu.multi', value: 40000, spread: 0.01 }] });
    const rows = compare(now, before);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ key: 'cpu.multi', verdict: 'faster', base: 40000 });
    expect(summaryText(rows)).toBe('1 faster');
    expect(summaryText([])).toBe('Nothing to compare');
    expect(summaryText([{ ...rows[0]!, verdict: 'same' }])).toBe('All within noise');
    expect(compare(now, null)).toEqual([]);
  });

  it('picks the newest earlier completed run of the same method', () => {
    const runs = [
      summary(6, 600),
      summary(5, 500, { status: 'stopped' }),
      summary(4.5, 450, { busy: true }),
      summary(4, 400, { method: 2 }),
      summary(3, 300, { mode: 'full' }),
      summary(2, 200),
    ];
    expect(baselineFor({ id: 7, ts: 700, method: 1 }, runs)?.id).toBe(6);
    expect(baselineFor({ id: 6, ts: 600, method: 1 }, runs)?.id).toBe(3);
    expect(baselineFor({ id: 6, ts: 600, method: 1 }, runs, 'quick')?.id).toBe(2);
    expect(baselineFor({ id: 2, ts: 200, method: 1 }, runs)).toBeNull();
  });

  it('lists what changed in the setup', () => {
    const a = run(1, 100, []).setup;
    const b = { ...a, gpuDriver: '577.00', powerPlan: 'High performance', board: 'MS-7E24' };
    expect(setupChanges(b, a)).toEqual([
      { label: 'GPU driver', from: '576.02', to: '577.00' },
      { label: 'Power plan', from: 'Balanced', to: 'High performance' },
    ]);
    expect(setupRows(a).map((r) => r.label)).toContain('RAM speed (MT/s)');
    expect(setupRows(a).some((r) => r.label === 'Board')).toBe(false);
  });

  it('reads headline figures from runs and summaries', () => {
    expect(headline(run(1, 1, [metric('cpu.multi', 7, null)]), 'cpu.multi')).toBe(7);
    expect(headline(summary(1, 1, { metrics: [{ key: 'gfx.fps', value: 200, spread: null }] }), 'gfx.fps')).toBe(200);
    expect(headline(summary(1, 1), 'gfx.fps')).toBeNull();
  });
});
