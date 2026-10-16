import { describe, expect, it } from 'vitest';
import { findingTime, fmtEvidence, fmtTempDelta, fmtVital, markSeen, thinHistory, unseen, vitalTone } from './insights';
import { SIM_ATTENTION, simInsights } from './sim-insights';
import type { Vital } from './types';

const vital = (v: Partial<Vital>): Vital => ({
  kind: 'headroomCpu', value: null, usual: null, unit: '°C', basis: 'measured', detail: null, metric: null, ...v,
});

describe('Insights text', () => {
  it('writes temperature differences without the °F offset', () => {
    expect(fmtTempDelta(10, 'c')).toBe('10 °C');
    expect(fmtTempDelta(10, 'f')).toBe('18 °F');
    expect(fmtTempDelta(9, 'c', true)).toBe('+9 °C');
    expect(fmtTempDelta(-0.2, 'c', true)).toBe('0 °C');
    expect(fmtVital(vital({ value: 27, usual: 36 }), 'f')).toEqual({ value: '49 °F', usual: '65 °F' });
    expect(fmtVital(vital({ kind: 'tempForLoad', value: 9.4, usual: 0 }), 'c')).toEqual({ value: '+9 °C', usual: null });
    expect(fmtVital(vital({ kind: 'fanResponse', value: 820, usual: 1310, unit: 'RPM' }), 'c').usual).toBe('1,310 RPM');
    expect(fmtVital(vital({ kind: 'efficiency' }), 'c').value).toBe('—');
  });

  it('flags the vital signs worth a look', () => {
    expect(vitalTone(vital({ value: 3 }))).toBe('warn');
    expect(vitalTone(vital({ value: 30 }))).toBe('ok');
    expect(vitalTone(vital({}))).toBe('unknown');
    expect(vitalTone(vital({ kind: 'fanResponse', value: 600, usual: 1300 }))).toBe('warn');
    expect(vitalTone(vital({ kind: 'clockStability', value: 2.5, usual: 1 }))).toBe('ok');
  });

  it('shows evidence temperatures as readings and the rest with their units', () => {
    expect(fmtEvidence({ label: '', metric: null, from: 0, to: 0, value: 71, baseline: 59, unit: '°C' }, 'f')).toEqual({
      value: '160 °F', usual: '138 °F',
    });
    expect(fmtEvidence({ label: '', metric: null, from: 0, to: 0, value: 2, baseline: 0.13, unit: '/min' }, 'c')).toEqual({
      value: '2.0 a minute', usual: '0.1 a minute',
    });
    expect(fmtEvidence({ label: '', metric: null, from: 0, to: 0, value: 1310.4, baseline: null, unit: 'RPM' }, 'c').value).toBe('1,310 RPM');
  });

  it('dates findings and says when history is thin', () => {
    const f = simInsights().findings[0]!;
    expect(findingTime({ ...f, since: 100 }, () => 'Oct 12')).toBe('since Oct 12');
    expect(findingTime({ ...f, since: null, ts: null }, () => '')).toBe('last 30 minutes');
    expect(thinHistory(2.5 * 86_400)).toContain('2 days so far');
    expect(thinHistory(3 * 86_400)).toBeNull();
  });

  it('remembers which warnings were shown', () => {
    expect(unseen(['a', 'b'], ['a'])).toEqual(['b']);
    const many = Array.from({ length: 120 }, (_, i) => `k${i}`);
    const seen = markSeen([], many);
    expect(seen).toHaveLength(100);
    expect(seen.at(-1)).toBe('k119');
  });

  it('the preview covers every kind of cause and its dot keys are its warnings', () => {
    const i = simInsights();
    const warn = i.findings.filter((f) => f.status === 'warn').map((f) => f.key);
    expect(SIM_ATTENTION).toEqual(warn);
    const roles = new Set(i.findings.flatMap((f) => [...f.causes, ...f.related]).flatMap((c) => [c.role, ...c.because.map((b) => b.role)]));
    expect([...roles].sort()).toEqual(['likely', 'possible', 'related']);
    expect(new Set(i.findings.map((f) => f.basis))).toEqual(new Set(['measured', 'inferred']));
    expect(i.vitals).toHaveLength(6);
  });
});
