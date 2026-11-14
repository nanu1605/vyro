// Clokrix Free and Pro in the browser preview (2.5): `?edition=free`, `?edition=trial` or
// `?edition=ended` (Free, the trial's end not said yet); Pro otherwise. Buy Pro is paid a few
// seconds later; any key that looks like one is taken.

import type { Edition } from './bindings/Edition';

const DAY = 86_400;
const now = () => Math.floor(Date.now() / 1000);
const listeners = new Set<(e: Edition) => void>();

function initial(): Edition {
  const base: Edition = { tier: 'pro', trialEnds: null, key: null, note: null, buying: false, canBuy: true, endedUnseen: false, price: '$100' };
  const asked = typeof location === 'undefined' ? null : new URLSearchParams(location.search).get('edition');
  if (asked === 'trial') return { ...base, tier: 'trial', trialEnds: now() + 5 * DAY + 3600 };
  if (asked === 'free') return { ...base, tier: 'free', trialEnds: now() - 2 * DAY };
  if (asked === 'ended') return { ...base, tier: 'free', trialEnds: now() - 3600, endedUnseen: true };
  return { ...base, trialEnds: now() - 20 * DAY, key: 'CLKX-•••••-•••••-•••••-7QW4M' };
}

let state = initial();

function set(next: Edition): Edition {
  state = next;
  for (const cb of listeners) cb(state);
  return state;
}

export function simEdition(): Edition {
  return state;
}

export function simBuyPro(): Edition {
  setTimeout(() => set({ ...state, tier: 'pro', buying: false, key: 'CLKX-•••••-•••••-•••••-7QW4M', note: null }), 4000);
  return set({ ...state, buying: true, note: null });
}

export function simEnterKey(key: string): Edition {
  const k = key.trim().toUpperCase().replace(/[\s-]/g, '');
  if (!/^CLKX[0-9A-Z]{20}$/.test(k)) throw new Error('That is not a Clokrix Pro key. Check it against the one you were given.');
  return set({ ...state, tier: 'pro', key: `CLKX-•••••-•••••-•••••-${k.slice(-5)}`, note: null, buying: false });
}

export function simReleasePro(): Edition {
  return set({ ...state, tier: 'free', key: null, note: 'Pro was freed on this PC; its key can be used on another.' });
}

export function simEditionSeen(): Edition {
  return set({ ...state, endedUnseen: false, note: null });
}

export function onSimEdition(cb: (e: Edition) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}
