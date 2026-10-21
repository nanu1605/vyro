import { describe, expect, it } from 'vitest';
import { dataText, keyFor, onMobileData, pasteKeys, readQuality, startWith } from './quality';

describe('HD or SD', () => {
  it('knows mobile data where the phone says, and only there', () => {
    expect(onMobileData({ type: 'cellular' })).toBe(true);
    expect(onMobileData({ type: 'wifi' })).toBe(false);
    expect(onMobileData({ type: 'wifi', saveData: true })).toBe(true);
    // iPhone: no connection information at all.
    expect(onMobileData(undefined)).toBeNull();
    expect(onMobileData({})).toBeNull();
  });

  it('asks once on mobile data, then keeps the answer', () => {
    expect(startWith(true, null, 'hd')).toEqual({ quality: 'sd', ask: true });
    expect(startWith(true, 'hd', null)).toEqual({ quality: 'hd', ask: false });
    expect(startWith(false, 'sd', null)).toEqual({ quality: 'hd', ask: false });
    expect(startWith(null, null, 'sd')).toEqual({ quality: 'sd', ask: false });
    expect(startWith(null, null, null)).toEqual({ quality: 'hd', ask: false });
  });

  it('reads only what it wrote', () => {
    expect(readQuality('sd')).toBe('sd');
    expect(readQuality('4k')).toBeNull();
    expect(readQuality(null)).toBeNull();
    expect(keyFor('DESKTOP-7E24')).toBe('vyro.quality.DESKTOP-7E24');
  });

  it('pastes with Ctrl+V and lets Ctrl go', () => {
    const keys = pasteKeys();
    expect(keys[0]).toEqual({ t: 'key', k: 'Control', down: true });
    expect(keys[1]).toEqual({ t: 'press', c: 'v' });
    expect(keys.at(-1)).toEqual({ t: 'key', k: 'Control', down: false });
  });

  it('says data used plainly', () => {
    expect(dataText(400_000)).toBe('0.4 MB');
    expect(dataText(12_400_000)).toBe('12 MB');
    expect(dataText(1_234_000_000)).toBe('1.2 GB');
  });
});
