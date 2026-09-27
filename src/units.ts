// Temperature display units. The engine measures and stores °C; °F is only how it is shown.
// Mirrors `vyro_core::units` (toasts are converted there).

import type { TempUnit } from './types';

export const cToF = (c: number): number => (c * 9) / 5 + 32;
export const fToC = (f: number): number => ((f - 32) * 5) / 9;

export function tempSymbol(unit: TempUnit): '°C' | '°F' {
  return unit === 'f' ? '°F' : '°C';
}

/** A °C value in `unit`. */
export function toUnit(c: number, unit: TempUnit): number {
  return unit === 'f' ? cToF(c) : c;
}

/** A value typed in `unit`, back to °C. */
export function fromUnit(v: number, unit: TempUnit): number {
  return unit === 'f' ? fToC(v) : v;
}

/** Rewrites each "<number> °C" in a message in `unit`, keeping its decimals. */
export function tempsIn(text: string, unit: TempUnit): string {
  if (unit === 'c') return text;
  return text.replace(/(-?\d+(?:\.(\d+))?)( ?)°C/g, (_m, num: string, dec: string | undefined, space: string) => {
    return `${cToF(Number(num)).toFixed(dec?.length ?? 0)}${space}°F`;
  });
}
