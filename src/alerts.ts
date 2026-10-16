// Alert rules (P7, P18) in words and their editor's arithmetic. The engine keeps every amount in
// the sensor's own unit (°C for temperatures); limits convert to °F with the offset, the size
// of a rise or a distance from usual without it.

import type { AlertActions, AlertKind, AlertRule, Condition, ConditionTest, Profile, TempUnit } from './types';
import { cToF, fToC, tempSymbol } from './units';

export type TestType = ConditionTest['type'];

export const MAX_CONDITIONS = 5;
export const FAN_BOOST = { min: 5, max: 50 } as const;
export const RATE_SECS = { min: 5, max: 600 } as const;
/** An alert's profile, fan and tray actions end this long after its conditions clear. */
export const CLEAR_SECS = 30;

export const TEST_LABEL: Record<TestType, string> = {
  above: 'is above',
  below: 'is below',
  rises: 'rises by',
  falls: 'falls by',
  aboveUsual: 'is above usual by',
  belowUsual: 'is below usual by',
};

/** What the helpers need of a sensor. */
export interface SensorInfo {
  name: string;
  unit: string;
}

export const DEFAULT_ACTIONS: AlertActions = { notify: true, tray: false, profile: null, fanBoost: null, record: false };

/** A rule's conditions; a threshold rule is one condition. */
export function conditionsOf(kind: AlertKind): { any: boolean; conditions: Condition[] } {
  if (kind.type === 'conditions') return { any: kind.any, conditions: kind.conditions };
  if (kind.type === 'threshold') {
    const test: ConditionTest = kind.above ? { type: 'above', value: kind.value } : { type: 'below', value: kind.value };
    return { any: false, conditions: [{ sensor: kind.sensor, test, hysteresis: 0 }] };
  }
  return { any: false, conditions: [] };
}

/** One plain limit stays a threshold rule (as earlier versions read it); anything else is a
 * conditions rule. */
export function kindFor(any: boolean, conditions: Condition[]): AlertKind {
  const [c] = conditions;
  if (conditions.length === 1 && c && c.hysteresis === 0 && (c.test.type === 'above' || c.test.type === 'below')) {
    return { type: 'threshold', sensor: c.sensor, above: c.test.type === 'above', value: c.test.value };
  }
  return { type: 'conditions', any, conditions };
}

/** Limits are readings; rises, falls and distances from usual are differences. */
export const isLimit = (t: ConditionTest): boolean => t.type === 'above' || t.type === 'below';
export const isRate = (t: ConditionTest): t is Extract<ConditionTest, { secs: number }> => t.type === 'rises' || t.type === 'falls';
export const amountOf = (t: ConditionTest): number => ('value' in t ? t.value : t.by);

export function withAmount(t: ConditionTest, v: number): ConditionTest {
  return 'value' in t ? { ...t, value: v } : { ...t, by: v };
}

/** A test of `type` with an amount that suits `unit`, keeping `prev`'s window if it had one. */
export function newTest(type: TestType, unit: string, prev?: ConditionTest): ConditionTest {
  const temp = unit === '°C';
  const secs = prev && isRate(prev) ? prev.secs : 30;
  switch (type) {
    case 'above':
      return { type, value: temp ? 85 : 90 };
    case 'below':
      return { type, value: unit === 'RPM' ? 500 : 10 };
    case 'rises':
    case 'falls':
      return { type, by: temp ? 15 : 20, secs };
    case 'aboveUsual':
    case 'belowUsual':
      return { type, by: temp ? 8 : 10 };
  }
}

/** An amount as the editor shows it: °C limits with the °F offset, differences ×1.8. */
export function shownAmount(v: number, limit: boolean, sensorUnit: string, unit: TempUnit): number {
  if (sensorUnit !== '°C' || unit === 'c') return v;
  return limit ? cToF(v) : v * 1.8;
}

/** The inverse of {@link shownAmount}: what the engine keeps. */
export function storedAmount(v: number, limit: boolean, sensorUnit: string, unit: TempUnit): number {
  if (sensorUnit !== '°C' || unit === 'c') return v;
  return limit ? fToC(v) : v / 1.8;
}

/** "185 °F", "1,200 RPM", "95%". */
export function fmtAmount(v: number, limit: boolean, sensorUnit: string, unit: TempUnit): string {
  const u = sensorUnit === '°C' ? tempSymbol(unit) : sensorUnit;
  const n = shownAmount(v, limit, sensorUnit, unit);
  const s = n.toLocaleString('en-US', { maximumFractionDigits: 1 });
  return u === '%' ? `${s}%` : u ? `${s} ${u}` : s;
}

/** "CPU (Tdie) above 185 °F", "rises 27 °F within 30 s", "14.4 °F above usual". */
export function conditionText(c: Condition, s: SensorInfo | undefined, unit: TempUnit): string {
  const name = s?.name ?? c.sensor;
  const su = s?.unit ?? '';
  const t = c.test;
  const amt = fmtAmount(amountOf(t), isLimit(t), su, unit);
  const text = (() => {
    switch (t.type) {
      case 'above':
        return `${name} above ${amt}`;
      case 'below':
        return `${name} below ${amt}`;
      case 'rises':
        return `${name} rises ${amt} within ${t.secs} s`;
      case 'falls':
        return `${name} falls ${amt} within ${t.secs} s`;
      case 'aboveUsual':
        return `${name} ${amt} above usual`;
      case 'belowUsual':
        return `${name} ${amt} below usual`;
    }
  })();
  return c.hysteresis > 0 ? `${text} (clears ${fmtAmount(c.hysteresis, false, su, unit)} back)` : text;
}

/** One line: what the rule watches, its hold and quiet time, sound. */
export function describeRule(r: AlertRule, sensor: (id: string) => SensorInfo | undefined, unit: TempUnit): string {
  const quiet = r.cooldownSecs >= 60 ? `${Math.round(r.cooldownSecs / 60)} min` : `${r.cooldownSecs} s`;
  const tail = ` · quiet ${quiet} after firing${r.sound && r.actions.notify ? ' · sound' : ''}`;
  if (r.kind.type === 'health') {
    return (r.kind.problems ? 'System Health problems: stopped fans, failing drives, limits reached' : 'Any System Health warning or problem') + tail;
  }
  const { any, conditions } = conditionsOf(r.kind);
  const what = conditions.map((c) => conditionText(c, sensor(c.sensor), unit)).join(any ? ' or ' : ' and ');
  const missing = conditions.some((c) => !sensor(c.sensor)) ? ' · sensor not present' : '';
  return `${what}${r.forSecs ? ` for ${r.forSecs} s` : ''}${tail}${missing}`;
}

/** What firing does besides the log: "Gaming profile", "fans +20 %", … */
export function actionsText(a: AlertActions, profileName: (p: Profile) => string): string[] {
  const out: string[] = [];
  if (!a.notify) out.push('no notification');
  if (a.profile) out.push(`${profileName(a.profile)} profile`);
  if (a.fanBoost != null) out.push(`fans +${a.fanBoost} %`);
  if (a.tray) out.push('named in the tray');
  if (a.record) out.push('diagnostic record');
  return out;
}

/** Why the draft cannot be saved, or `null`. Mirrors the engine's checks. */
export function ruleProblem(d: AlertRule): string | null {
  if (!d.name.trim()) return 'Give the rule a name.';
  if (d.name.length > 80) return 'Keep the name under 80 characters.';
  if (!(d.forSecs >= 0 && d.forSecs <= 3600)) return 'Hold time is 0 to 3600 seconds.';
  if (!(d.cooldownSecs >= 0 && d.cooldownSecs <= 86400)) return 'Quiet time is 0 to 1440 minutes.';
  const a = d.actions;
  if (d.kind.type === 'health') {
    if (a.tray || a.profile || a.fanBoost != null) return 'System Health rules can only notify and record.';
  } else {
    const { conditions } = conditionsOf(d.kind);
    if (conditions.length === 0) return 'Add a condition.';
    if (conditions.length > MAX_CONDITIONS) return `At most ${MAX_CONDITIONS} conditions.`;
    for (const c of conditions) {
      if (!c.sensor) return 'Pick a sensor for each condition.';
      const v = amountOf(c.test);
      if (!Number.isFinite(v)) return 'Enter a number for each condition.';
      if (!isLimit(c.test) && v <= 0) return 'A rise, a fall or a distance from usual is above 0.';
      if (isRate(c.test) && !(c.test.secs >= RATE_SECS.min && c.test.secs <= RATE_SECS.max)) {
        return `A rise or fall is measured over ${RATE_SECS.min} to ${RATE_SECS.max} seconds.`;
      }
      if (!(Number.isFinite(c.hysteresis) && c.hysteresis >= 0)) return 'The clearing margin is 0 or more.';
    }
  }
  if (a.fanBoost != null && !(a.fanBoost >= FAN_BOOST.min && a.fanBoost <= FAN_BOOST.max)) {
    return `Raise fans by ${FAN_BOOST.min} to ${FAN_BOOST.max} points.`;
  }
  return null;
}
