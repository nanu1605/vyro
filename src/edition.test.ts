import { describe, expect, it } from 'vitest';
import { trialDays } from './edition.svelte';
import type { Edition } from './types';

const base: Edition = { tier: 'trial', trialEnds: 1_000_000, key: null, note: null, buying: false, canBuy: true, endedUnseen: false, price: '$100' };

describe('the Pro trial', () => {
  it('counts the days left, rounded up, only during the trial', () => {
    expect(trialDays(base, 1_000_000 - 7 * 86_400)).toBe(7);
    expect(trialDays(base, 1_000_000 - 86_400 - 1)).toBe(2);
    expect(trialDays(base, 1_000_000 - 60)).toBe(1);
    expect(trialDays(base, 1_000_000 + 60)).toBe(0);
    expect(trialDays({ ...base, tier: 'pro' }, 0)).toBeNull();
    expect(trialDays({ ...base, tier: 'free' }, 0)).toBeNull();
    expect(trialDays({ ...base, trialEnds: null }, 0)).toBeNull();
    expect(trialDays(null)).toBeNull();
  });
});
