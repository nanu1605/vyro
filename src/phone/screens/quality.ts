// HD or SD for the PC's screen (1.7): HD as the connection carries it, SD to spare a phone's
// mobile data. The phone asks once on mobile data and remembers; it can switch at any time. No
// DOM here: the Screen tab reads the connection and storage and passes them in.

import type { Input } from './gestures';

export type Quality = 'hd' | 'sd';

/** The most each sends, megabytes a minute (the PC's caps: 5 and 1 Mbit/s; a still screen sends
 * far less). */
export const MB_A_MINUTE: Record<Quality, number> = { hd: 35, sd: 7 };

/** What the browser says of its connection (Android Chrome; iPhone says nothing). */
export interface Connection {
  type?: string;
  saveData?: boolean;
}

/** On mobile data: true or false, or null when the phone does not say. Asking to save data
 * counts as mobile data. */
export function onMobileData(c: Connection | null | undefined): boolean | null {
  if (!c) return null;
  if (c.saveData) return true;
  if (typeof c.type !== 'string') return null;
  return c.type === 'cellular';
}

/** Where a choice is remembered: per PC, and one for mobile data. */
export const KEY_MOBILE = 'vyro.quality.mobile';
export const keyFor = (pc: string) => `vyro.quality.${pc}`;

export function readQuality(v: string | null): Quality | null {
  return v === 'hd' || v === 'sd' ? v : null;
}

/** The quality to start with, and whether to ask first: on mobile data with no choice made for
 * it, ask; otherwise the choice for mobile data there, or this PC's last one (HD at first). */
export function startWith(mobile: boolean | null, forMobile: Quality | null, forPc: Quality | null): { quality: Quality; ask: boolean } {
  if (mobile) return forMobile ? { quality: forMobile, ask: false } : { quality: 'sd', ask: true };
  return { quality: forPc ?? 'hd', ask: false };
}

/** Ctrl+V on the PC: what the Paste key sends. */
export function pasteKeys(): Input[] {
  return [
    { t: 'key', k: 'Control', down: true },
    { t: 'press', c: 'v' },
    { t: 'key', k: 'Control', down: false },
  ];
}

/** Bytes as the phone says data used: "0.4 MB", "12 MB", "1.2 GB". */
export function dataText(bytes: number): string {
  const mb = bytes / 1e6;
  if (mb >= 1000) return `${(mb / 1000).toFixed(1)} GB`;
  if (mb >= 10) return `${Math.round(mb)} MB`;
  return `${mb.toFixed(1)} MB`;
}
