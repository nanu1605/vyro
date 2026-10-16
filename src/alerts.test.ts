import { describe, expect, it } from 'vitest';
import {
  actionsText,
  conditionsOf,
  conditionText,
  DEFAULT_ACTIONS,
  describeRule,
  fmtAmount,
  kindFor,
  newTest,
  ruleProblem,
  shownAmount,
  storedAmount,
} from './alerts';
import { SIM_RULES, simAlertLog, simAlertState } from './sim-alerts';
import type { AlertRule, Condition } from './types';

const cpu = { name: 'CPU (Tdie)', unit: '°C' };
const sensors: Record<string, { name: string; unit: string }> = {
  'cpu.temp.package': cpu,
  'gpu.0.usage': { name: 'GPU #0 Usage', unit: '%' },
  'fan.0': { name: 'CPU Fan', unit: 'RPM' },
};
const info = (id: string) => sensors[id];

const rule = (patch: Partial<AlertRule>): AlertRule => ({
  id: 'r', name: 'Rule', enabled: true, kind: { type: 'conditions', any: false, conditions: [] },
  forSecs: 30, cooldownSecs: 600, sound: true, actions: { ...DEFAULT_ACTIONS }, ...patch,
});

describe('alert rules', () => {
  it('keeps a single plain limit as a threshold rule and edits it as a condition', () => {
    const plain: Condition = { sensor: 'cpu.temp.package', test: { type: 'above', value: 90 }, hysteresis: 0 };
    const kind = kindFor(false, [plain]);
    expect(kind).toEqual({ type: 'threshold', sensor: 'cpu.temp.package', above: true, value: 90 });
    expect(conditionsOf(kind)).toEqual({ any: false, conditions: [plain] });
    expect(kindFor(false, [{ ...plain, hysteresis: 3 }]).type).toBe('conditions');
    expect(kindFor(true, [plain, plain]).type).toBe('conditions');
  });

  it('converts limits with the °F offset and differences without it', () => {
    expect(shownAmount(85, true, '°C', 'f')).toBe(185);
    expect(shownAmount(10, false, '°C', 'f')).toBe(18);
    expect(storedAmount(185, true, '°C', 'f')).toBeCloseTo(85);
    expect(storedAmount(18, false, '°C', 'f')).toBeCloseTo(10);
    expect(shownAmount(500, true, 'RPM', 'f')).toBe(500);
    expect(fmtAmount(1200, true, 'RPM', 'c')).toBe('1,200 RPM');
    expect(fmtAmount(95, true, '%', 'c')).toBe('95%');
  });

  it('says each condition in words', () => {
    const c = (test: Condition['test'], hysteresis = 0): Condition => ({ sensor: 'cpu.temp.package', test, hysteresis });
    expect(conditionText(c({ type: 'above', value: 85 }, 3), cpu, 'f')).toBe('CPU (Tdie) above 185 °F (clears 5.4 °F back)');
    expect(conditionText(c({ type: 'rises', by: 15, secs: 30 }), cpu, 'c')).toBe('CPU (Tdie) rises 15 °C within 30 s');
    expect(conditionText(c({ type: 'aboveUsual', by: 8 }), cpu, 'f')).toBe('CPU (Tdie) 14.4 °F above usual');
    expect(conditionText({ sensor: 'fan.0', test: { type: 'falls', by: 400, secs: 10 }, hysteresis: 0 }, info('fan.0'), 'c')).toBe(
      'CPU Fan falls 400 RPM within 10 s',
    );
    expect(newTest('rises', '°C', { type: 'falls', by: 3, secs: 90 })).toEqual({ type: 'rises', by: 15, secs: 90 });
  });

  it('describes compound rules and what they do', () => {
    const r = SIM_RULES.find((x) => x.id === 'hot-under-load')!;
    expect(describeRule(r, info, 'c')).toBe(
      'CPU (Tdie) above 80 °C (clears 3 °C back) and GPU #0 Usage above 90% for 30 s · quiet 10 min after firing',
    );
    expect(actionsText(r.actions, (p) => p)).toEqual(['fans +15 %', 'named in the tray', 'diagnostic record']);
    expect(describeRule(rule({ kind: { type: 'conditions', any: true, conditions: [{ sensor: 'x.y', test: { type: 'below', value: 1 }, hysteresis: 0 }] } }), info, 'c')).toContain(
      'sensor not present',
    );
  });

  it('refuses what the engine would refuse', () => {
    const cond: Condition = { sensor: 'cpu.temp.package', test: { type: 'rises', by: 10, secs: 30 }, hysteresis: 0 };
    const kind = (c: Condition) => ({ type: 'conditions' as const, any: false, conditions: [c] });
    expect(ruleProblem(rule({ kind: kind(cond) }))).toBeNull();
    expect(ruleProblem(rule({}))).toBe('Add a condition.');
    expect(ruleProblem(rule({ kind: kind({ ...cond, test: { type: 'rises', by: 10, secs: 900 } }) }))).toContain('5 to 600');
    expect(ruleProblem(rule({ kind: kind({ ...cond, test: { type: 'aboveUsual', by: 0 } }) }))).toContain('above 0');
    expect(ruleProblem(rule({ kind: kind(cond), actions: { ...DEFAULT_ACTIONS, fanBoost: 80 } }))).toContain('5 to 50');
    expect(ruleProblem(rule({ kind: { type: 'health', problems: true }, actions: { ...DEFAULT_ACTIONS, profile: 'silent' } }))).toContain(
      'only notify and record',
    );
  });

  it('previews a rule that holds and an alert that was not shown again', () => {
    for (const r of SIM_RULES) expect(ruleProblem(r), r.id).toBeNull();
    const state = simAlertState(SIM_RULES, 1000);
    expect(state.active.map((a) => [a.ruleId, a.fanBoost, a.since])).toEqual([['hot-under-load', 15, 810]]);
    const off = SIM_RULES.map((r) => ({ ...r, enabled: false }));
    expect(simAlertState(off, 1000).active).toEqual([]);
    const log = simAlertLog(10_000);
    expect(log.filter((a) => !a.notify).map((a) => a.also)).toEqual(['Hot under load']);
    expect(log.some((a) => a.diagnostic?.programs.length)).toBe(true);
  });
});
