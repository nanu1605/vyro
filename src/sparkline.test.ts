import { describe, expect, it } from 'vitest';
import { Ring } from './sparkline';

describe('Ring', () => {
  it('keeps the newest values up to capacity, oldest first', () => {
    const r = new Ring(3);
    [1, 2, null, 4].forEach((v) => r.push(v));
    expect(r.values()).toEqual([2, null, 4]);
  });
});
