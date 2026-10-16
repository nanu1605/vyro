import { describe, expect, it } from 'vitest';
import { coverageText, fmtPart, INPUTS, trendPath, trendText } from './score';
import { simScore, simScoreHistory } from './sim-score';
import type { ScorePart } from './types';

const part = (value: number, unit: string, difference = false): ScorePart => ({ label: '', value, unit, difference, score: 0 });

describe('Performance Score text', () => {
  it('writes parts in the chosen unit', () => {
    expect(fmtPart(part(17, '°C', true), 'c')).toBe('17 °C');
    expect(fmtPart(part(17, '°C', true), 'f')).toBe('31 °F');
    expect(fmtPart(part(78, '°C'), 'f')).toBe('172 °F');
    expect(fmtPart(part(0.1, '%'), 'c')).toBe('0.1%');
    expect(fmtPart(part(61, '%'), 'c')).toBe('61%');
    expect(fmtPart(part(18, 'ms'), 'c')).toBe('18 ms');
    expect(fmtPart(part(2, ''), 'c')).toBe('2');
    expect(fmtPart(part(1, 'health'), 'c')).toBe('caution');
  });

  it('says how much of the weight it rests on', () => {
    const s = simScore().latest;
    expect(coverageText(s)).toBe('Based on all 9 inputs');
    const fewer = { ...s, coverage: 0.85, inputs: s.inputs.map((i) => (i.id === 'network' || i.id === 'sustainedClocks' ? { ...i, score: null } : i)) };
    expect(coverageText(fewer)).toBe('Based on 7 of 9 inputs (85% of the weight)');
  });

  it('describes the trend over about a month', () => {
    const now = 400 * 86_400;
    const h = [
      { ts: now - 40 * 86_400, score: 80, formula: 1 },
      { ts: now - 30 * 86_400, score: 84, formula: 1 },
      { ts: now, score: 95, formula: 1 },
    ];
    expect(trendText(h, now)).toBe('Up 11 over 30 days');
    expect(trendText(h.slice(-1), now)).toBeNull();
    // A new formula version is not compared with the old one.
    expect(trendText([...h.slice(0, 2), { ts: now, score: 95, formula: 2 }], now)).toBeNull();
    expect(trendPath(h, 100, 50).startsWith('M0.0 10.0')).toBe(true);
  });

  it('previews the worked example of the formula', () => {
    const r = simScore();
    const s = r.latest;
    expect(s.inputs.map((i) => i.id)).toEqual(Object.keys(INPUTS));
    expect(s.inputs.reduce((n, i) => n + i.weight, 0)).toBe(100);
    const avg = s.inputs.reduce((n, i) => n + i.weight * (i.score ?? 0), 0) / 100;
    expect(Math.round(avg)).toBe(s.score);
    expect(r.history).toHaveLength(365);
    expect(simScoreHistory(1000 * 86_400).some((p) => p.score < 85)).toBe(true);
  });
});
