// Browser preview of the Performance Score: the worked example of docs/performance-score.md
// (reference PC) and a year of daily scores with a dip some months ago.

import type { ScoreInput, ScorePart, ScorePoint, ScoreReport } from './types';

const D = 86_400;

const part = (label: string, value: number, unit: string, score: number, difference = false): ScorePart => ({
  label, value, unit, difference, score,
});
const input = (id: ScoreInput['id'], weight: number, parts: ScorePart[], basis: ScoreInput['basis'] = 'measured'): ScoreInput => ({
  id, weight, basis, parts, missing: null, score: Math.min(...parts.map((p) => p.score)),
});

const INPUTS: ScoreInput[] = [
  input('thermals', 15, [part('CPU headroom under load', 17, '°C', 88, true), part('GPU #0 headroom under load', 23, '°C', 97, true)]),
  input('sustainedClocks', 10, [part('CPU clock sag over long full load', 1.4, '%', 100), part('GPU #0 clock sag over long full load', 0.8, '%', 100)]),
  input('throttling', 15, [part('Loaded time limited by heat or current', 0, '%', 100)]),
  input('cooling', 10, [part('CPU hotter than usual for the load', 1, '°C', 100, true), part('CPU fan speed against usual', 98, '%', 100)]),
  input('storage', 10, [
    part('Worst drive health', 0, 'health', 100),
    part('SSD life left', 96, '%', 100),
    part('Free on the system drive', 41, '%', 100),
  ]),
  input('memory', 10, [part('RAM in use (95th percentile)', 61, '%', 100)]),
  input('network', 5, [part('Ping (95th percentile)', 18, 'ms', 100), part('Lost replies', 0.1, '%', 98)]),
  input('stability', 15, [part('Unexpected shutdowns (30 days)', 0, '', 100), part('Corrected hardware errors (30 days)', 0, '', 100)]),
  input('deviation', 10, [part('Insights warnings now', 0, '', 100)], 'inferred'),
];

/** A year of daily scores: steady in the high 90s, with three weeks in the low 80s about four
 * months ago (a BIOS update had reset the fan curve). */
export function simScoreHistory(now: number): ScorePoint[] {
  const out: ScorePoint[] = [];
  for (let i = 364; i >= 0; i--) {
    const dip = i >= 118 && i <= 139;
    const score = dip ? 82 + (i % 3) : 96 + Math.round(1.6 * Math.sin(i / 9));
    out.push({ ts: now - i * D, score: Math.min(100, score), formula: 1 });
  }
  return out;
}

export function simScore(): ScoreReport {
  const now = Math.floor(Date.now() / 1000);
  return {
    latest: { ts: now, formula: 1, score: 98, band: 'excellent', coverage: 1, windowSecs: 7 * D, inputs: INPUTS },
    history: simScoreHistory(now - D),
  };
}
