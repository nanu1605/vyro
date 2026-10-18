// GPU tuning (1.6), the window's pure logic: words for settings, sliders kept within what the
// driver allows, and the owner's preset list. The service checks every value again.

import type { Profile, TuneConfig, TuneLimits, TunePreset, TunePresetView, TuneRange, TuneSettings, TuneStatus } from './types';

export const STOCK: TuneSettings = { coreMhz: 0, memMhz: 0, powerPct: 100 };
/** Most presets the owner can keep, and the longest name (as the service checks). */
export const MAX_PRESETS = 16;
export const MAX_NAME = 32;

export const isStock = (s: TuneSettings): boolean => s.coreMhz === 0 && s.memMhz === 0 && s.powerPct === 100;

export const same = (a: TuneSettings | null | undefined, b: TuneSettings | null | undefined): boolean =>
  a != null && b != null && a.coreMhz === b.coreMhz && a.memMhz === b.memMhz && a.powerPct === b.powerPct;

const signed = (n: number): string => (n > 0 ? `+${n}` : `${n}`).replace('-', '−');

/** "core +105 MHz, memory +600 MHz, power 110 %", or "stock". */
export function describe(s: TuneSettings): string {
  if (isStock(s)) return 'stock';
  const parts: string[] = [];
  if (s.coreMhz !== 0) parts.push(`core ${signed(s.coreMhz)} MHz`);
  if (s.memMhz !== 0) parts.push(`memory ${signed(s.memMhz)} MHz`);
  if (s.powerPct !== 100) parts.push(`power ${s.powerPct} %`);
  return parts.join(', ');
}

const clampTo = (r: TuneRange | null, v: number, stock: number): number =>
  r ? Math.min(r.max, Math.max(r.min, Math.round(v))) : stock;

/** `s` within the driver's ranges; a setting it does not allow stays stock. */
export function clamp(s: TuneSettings, l: TuneLimits): TuneSettings {
  return {
    coreMhz: clampTo(l.core, s.coreMhz, 0),
    memMhz: clampTo(l.mem, s.memMhz, 0),
    powerPct: clampTo(l.power, s.powerPct, 100),
  };
}

/** The settings of the preset in force for `profile`. */
export function inForce(st: TuneStatus, profile: Profile = st.profile): TunePresetView | null {
  const id = st.config.profiles[profile] ?? st.config.preset;
  return st.presets.find((p) => p.id === id) ?? null;
}

/** A new id for an owner's preset ("custom-3"), not yet taken. */
export function newId(presets: Array<{ id: string }>): string {
  for (let n = 1; ; n++) {
    const id = `custom-${n}`;
    if (!presets.some((p) => p.id === id)) return id;
  }
}

/** Why a preset name will not do, or null. */
export function nameProblem(name: string, others: TunePreset[], id: string | null): string | null {
  const n = name.trim();
  if (!n) return 'Give it a name.';
  if ([...n].length > MAX_NAME) return `At most ${MAX_NAME} characters.`;
  if (others.some((p) => p.id !== id && p.name.toLowerCase() === n.toLowerCase())) return 'Another preset has that name.';
  return null;
}

/** The configuration with `preset` added, or replacing the owner's preset of the same id. */
export function withPreset(cfg: TuneConfig, preset: TunePreset): TuneConfig {
  const presets = cfg.presets.some((p) => p.id === preset.id)
    ? cfg.presets.map((p) => (p.id === preset.id ? preset : p))
    : [...cfg.presets, preset];
  return { ...cfg, presets };
}

/** The configuration without the owner's preset `id`; whatever used it goes back to Stock. */
export function withoutPreset(cfg: TuneConfig, id: string): TuneConfig {
  const profiles = Object.fromEntries(
    Object.entries(cfg.profiles).map(([k, v]) => [k, v === id ? 'stock' : v]),
  ) as TuneConfig['profiles'];
  return {
    ...cfg,
    presets: cfg.presets.filter((p) => p.id !== id),
    preset: cfg.preset === id ? 'stock' : cfg.preset,
    profiles,
  };
}

/** What the card runs, in a line for the header ("Performance · core +100 MHz, …"). */
export function runningText(st: TuneStatus): string {
  if (!st.config.enabled) return 'Stock (tuning is off)';
  if (st.trial) return `On test: ${describe(st.trial.settings)}`;
  const applied = st.presets.find((p) => p.id === st.applied);
  if (applied) return `${applied.name} · ${describe(applied.settings)}`;
  return 'Stock';
}

/** When settings passed the stability test, in words ("tested 18 Oct"). */
export function testedText(at: number | null): string | null {
  if (at == null) return null;
  if (at === 0) return 'stock';
  return `tested ${new Date(at * 1000).toLocaleDateString([], { day: 'numeric', month: 'short' })}`;
}
