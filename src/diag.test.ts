import { describe, expect, it } from 'vitest';
import { countsText, fmtAgo, fmtCitation, ordered, sourceText, stamp } from './diag';
import type { Citation, DiagReport } from './types';

const num = (value: number, unit: string, digits: number, extra: Partial<Citation> = {}): Citation => ({
  label: 'x', value, unit, difference: false, digits, text: null, source: 'history', metric: null, ago: null, ...extra,
});

describe('diagnostics text (same cases as vyro_core::diag)', () => {
  it('formats citations like the exports', () => {
    expect(fmtCitation(num(78, '°C', 0), 'c')).toBe('78 °C');
    expect(fmtCitation(num(78, '°C', 0), 'f')).toBe('172 °F');
    expect(fmtCitation(num(17, '°C', 0, { difference: true }), 'f')).toBe('31 °F');
    expect(fmtCitation(num(0.06, '%', 1), 'c')).toBe('0.1%');
    expect(fmtCitation(num(3, '', 0), 'c')).toBe('3');
    expect(fmtCitation(num(78.5, '°C', 0), 'c')).toBe('79 °C');
    expect(fmtCitation(num(-0.25, '°C', 1, { difference: true }), 'c')).toBe('-0.3 °C');
    expect(fmtCitation({ ...num(0, '', 0), value: null, text: 'CPU at 91 °C' }, 'f')).toBe('CPU at 196 °F');
    expect(fmtAgo(30)).toBe('1 min before');
    expect(fmtAgo(7200)).toBe('2 h before');
    expect(fmtAgo(3 * 86_400)).toBe('3 d before');
    expect(sourceText(num(1, '', 0, { source: 'healthLog', ago: 3 * 86_400 }))).toBe('Health log, 3 d before');
  });

  it('counts and orders checks', () => {
    const r: DiagReport = {
      id: 1, ts: 0, version: 1, appVersion: '0.4.0', engine: 'Vyro service 0.4.0', pc: [],
      checks: [
        { id: 'thermals', status: 'pass', summary: '', advice: null, citations: [] },
        { id: 'memory', status: 'attention', summary: '', advice: null, citations: [] },
        { id: 'drivers', status: 'info', summary: '', advice: null, citations: [] },
      ],
    };
    expect(countsText(r)).toBe('1 attention · 1 pass · 1 info · 0 unavailable');
    expect(ordered(r).map((c) => c.id)).toEqual(['memory', 'thermals', 'drivers']);
    expect(stamp(new Date(2026, 9, 15, 10, 4).getTime() / 1000)).toBe('20261015-1004');
  });
});
