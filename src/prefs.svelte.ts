// Display preferences (P9b), shared by every view and saved with the app's settings
// (`settings.json`). Components read them through these helpers so they follow changes live.

import { fmt, fmtUnit, NA } from './format';
import { getProfiles, getSettings, onSettingsChanged, setSettings, type AppSettings } from './ipc';
import type { ProfileSpec } from './types';
import { fromUnit, tempSymbol, tempsIn, toUnit } from './units';

export const prefs = $state<AppSettings>({ closeToTray: true, temperatureUnit: 'c', hiddenSensors: [], profile: 'desktop' });

/** Profile presets, loaded with the preferences. */
export const profiles = $state<{ list: ProfileSpec[] }>({ list: [] });

let listening = false;

/** Loads the settings and follows later changes (tray menu, other views). */
export async function loadPrefs(): Promise<void> {
  if (!listening) {
    listening = true;
    void onSettingsChanged((s) => Object.assign(prefs, s));
  }
  const [s, list] = await Promise.all([getSettings(), getProfiles()]);
  profiles.list = list;
  Object.assign(prefs, s);
}

/** The chosen profile's preset (Desktop until loaded). */
export function profileSpec(): ProfileSpec {
  return (
    profiles.list.find((p) => p.id === prefs.profile) ?? {
      id: 'desktop',
      name: 'Desktop',
      description: '',
      pollMs: 1000,
      notifications: 'all',
      motion: 'full',
      dashboard: 'full',
    }
  );
}

/** Saves a change; the views update once it is stored. */
export async function savePrefs(patch: Partial<AppSettings>): Promise<void> {
  const next: AppSettings = { ...$state.snapshot(prefs), ...patch };
  await setSettings(next);
  Object.assign(prefs, next);
}

/** "°C" or "°F". */
export const tempUnit = (): '°C' | '°F' => tempSymbol(prefs.temperatureUnit);

/** A °C reading (or limit) in the chosen unit. */
export const temp = (c: number): number => toUnit(c, prefs.temperatureUnit);

/** A °C reading in the chosen unit; `null` stays `null`. */
export const tempOrNull = (c: number | null | undefined): number | null => (c == null ? null : temp(c));

/** A value typed in the chosen unit, back to °C. */
export const tempFromUser = (v: number): number => fromUnit(v, prefs.temperatureUnit);

/** "71 °C" / "160 °F", or `null` when missing (KV rows show that as a dash). */
export function fmtTemp(c: number | null | undefined, digits = 0): string | null {
  return c == null ? null : `${fmt(temp(c), digits)} ${tempUnit()}`;
}

/** Engine texts (health findings, alerts) with their temperatures in the chosen unit. */
export const tempText = (text: string): string => tempsIn(text, prefs.temperatureUnit);

/** A registry reading with its unit, temperatures in the chosen unit. */
export function fmtReading(value: number | null | undefined, unit: string): string {
  if (unit !== '°C') return fmtUnit(value, unit);
  return value == null ? NA : fmtUnit(temp(value), tempUnit());
}

/** The display unit for a registry unit. */
export const displayUnit = (unit: string): string => (unit === '°C' ? tempUnit() : unit);

export const isHidden = (id: string): boolean => prefs.hiddenSensors.includes(id);
