import { describe, expect, it } from 'vitest';
import { NA, fmt, fmtGB } from './format';

describe('fmt', () => {
  it('renders missing and non-finite values as unavailable', () => {
    expect(fmt(null)).toBe(NA);
    expect(fmt(undefined)).toBe(NA);
    expect(fmt(Number.NaN)).toBe(NA);
  });

  it('groups thousands and fixes decimals', () => {
    expect(fmt(4821.4)).toBe('4,821');
    expect(fmt(88.46, 1)).toBe('88.5');
    expect(fmtGB(32768)).toBe('32.0');
  });
});
