// The Performance Score (P17) in words: input names, bands and the parts behind each sub-score.
// The engine works in °C; differences convert to °F without the offset.

import type { PerformanceScore, ScoreBand, ScoreInputId, ScorePart, ScorePoint, TempUnit } from './types';
import { cToF, tempSymbol } from './units';

export const INPUTS: Record<ScoreInputId, { label: string; hint: string }> = {
  thermals: { label: 'Thermals', hint: 'Degrees left below the CPU’s and GPU’s limits under load' },
  sustainedClocks: { label: 'Sustained clocks', hint: 'How much the clock sags over long full load' },
  throttling: { label: 'Throttling', hint: 'Time under load the hardware reported limiting for heat or current' },
  cooling: { label: 'Cooling', hint: 'Temperature for the load and fan speed against their usual' },
  storage: { label: 'Storage health', hint: 'Drive health, SSD life left, free space on the system drive' },
  memory: { label: 'Memory pressure', hint: 'How full the RAM gets' },
  network: { label: 'Network quality', hint: 'Ping and lost replies to the ping target' },
  stability: { label: 'Stability', hint: 'Unexpected shutdowns and hardware errors in 30 days' },
  deviation: { label: 'Change from usual', hint: 'Insights warnings now' },
};

export const BANDS: Record<ScoreBand, { label: string; tone: 'ok' | 'warn' | 'fail' }> = {
  excellent: { label: 'Excellent', tone: 'ok' },
  good: { label: 'Good', tone: 'ok' },
  fair: { label: 'Fair', tone: 'warn' },
  needsAttention: { label: 'Needs attention', tone: 'fail' },
};

/** "17 °C", "31 °F" (a difference), "61%", "18 ms", "2". */
export function fmtPart(p: ScorePart, unit: TempUnit): string {
  if (p.unit === '°C') {
    const v = unit === 'f' ? (p.difference ? p.value * 1.8 : cToF(p.value)) : p.value;
    return `${Math.round(v)} ${tempSymbol(unit)}`;
  }
  if (p.unit === 'health') return ['good', 'caution', 'bad'][Math.round(p.value)] ?? '—';
  const digits = p.unit === '%' && Math.abs(p.value) < 10 ? 1 : 0;
  const n = Number(p.value.toFixed(digits)).toLocaleString('en-US');
  if (p.unit === '%') return `${n}%`;
  return p.unit ? `${n} ${p.unit}` : n;
}

/** "Based on 7 of 9 inputs (85% of the weight)". */
export function coverageText(s: PerformanceScore): string {
  const have = s.inputs.filter((i) => i.score != null).length;
  const pct = Math.round(s.coverage * 100);
  return have === s.inputs.length ? `Based on all ${have} inputs` : `Based on ${have} of ${s.inputs.length} inputs (${pct}% of the weight)`;
}

/** What the stored scores say about the trend: the change over about 30 days, if known. */
export function trendText(history: ScorePoint[], now: number): string | null {
  const same = history.filter((p) => p.formula === history.at(-1)?.formula);
  const last = same.at(-1);
  if (!last || same.length < 2) return null;
  const month = same.filter((p) => p.ts <= now - 25 * 86_400).at(-1) ?? same[0]!;
  if (month === last) return null;
  const days = Math.max(1, Math.round((last.ts - month.ts) / 86_400));
  const d = last.score - month.score;
  return d === 0 ? `Unchanged over ${days} days` : `${d > 0 ? 'Up' : 'Down'} ${Math.abs(d)} over ${days} days`;
}

/** SVG path of the daily scores over `width` × `height` (0–100 on the y axis). */
export function trendPath(history: ScorePoint[], width: number, height: number): string {
  if (history.length < 2) return '';
  const t0 = history[0]!.ts;
  const span = Math.max(1, history.at(-1)!.ts - t0);
  return history
    .map((p, i) => {
      const x = ((p.ts - t0) / span) * width;
      const y = height - (p.score / 100) * height;
      return `${i ? 'L' : 'M'}${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(' ');
}
