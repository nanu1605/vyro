import { describe, expect, it } from 'vitest';
import { NA, fmt, fmtDuration, fmtGB, fmtKB, fmtMB, hex4 } from './format';

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

describe('durations and sizes', () => {
  it('formats uptime', () => {
    expect(fmtDuration(null)).toBe(NA);
    expect(fmtDuration(65)).toBe('1m 5s');
    expect(fmtDuration(4 * 3600 + 12 * 60)).toBe('4h 12m');
    expect(fmtDuration(3 * 86400 + 4 * 3600 + 12 * 60 + 9)).toBe('3d 4h 12m');
  });

  it('formats cache and memory sizes', () => {
    expect(fmtKB(48)).toBe('48 KB');
    expect(fmtKB(2048)).toBe('2 MB');
    expect(fmtKB(1280)).toBe('1.3 MB');
    expect(fmtMB(16384)).toBe('16 GB');
    expect(fmtMB(512)).toBe('512 MB');
    expect(hex4(0x10de)).toBe('10DE');
  });
});
