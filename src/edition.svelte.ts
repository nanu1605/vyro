// Clokrix Free and Pro in the page (2.5): what this PC is, kept current. Until it is known, as
// Pro, so nothing flickers shut at start.

import { getEdition, onEdition } from './ipc';
import { FREE_VIEWS, PRO_SECTIONS, type Tab } from './nav';
import type { Edition } from './types';

export const edition = $state<{ e: Edition | null }>({ e: null });

/** Everything is open: Pro, or the trial. */
export function full(): boolean {
  return edition.e?.tier !== 'free';
}

/** The view shows a Pro screen instead. */
export function locked(t: Tab): boolean {
  return !full() && !FREE_VIEWS.includes(t);
}

/** Settings' section shows a Pro screen instead. */
export function lockedSection(id: string): boolean {
  return !full() && PRO_SECTIONS.includes(id);
}

/** Days left in the trial, rounded up; null outside it. */
export function trialDays(e: Edition | null, now = Date.now() / 1000): number | null {
  if (e?.tier !== 'trial' || e.trialEnds === null) return null;
  return Math.max(0, Math.ceil((e.trialEnds - now) / 86_400));
}

let started = false;
export async function loadEdition(): Promise<void> {
  if (started) return;
  started = true;
  await onEdition((e) => (edition.e = e));
  edition.e = await getEdition().catch(() => null);
}
