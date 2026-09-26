import { describe, expect, it } from 'vitest';
import { NA, fmt, fmtDuration, fmtGB, fmtGBTotal, fmtKB, fmtMB, fmtRate, fmtStorage, fmtUnit, hex4 } from './format';

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

describe('storage', () => {
  it('formats capacities, traffic and rates', () => {
    expect(fmtStorage(null)).toBe(NA);
    expect(fmtStorage(953_870)).toBe('932 GB');
    expect(fmtStorage(1_907_729)).toBe('1.82 TB');
    expect(fmtStorage(40 * 1024)).toBe('40.0 GB');
    expect(fmtGBTotal(812.4)).toBe('812 GB');
    expect(fmtGBTotal(18_540)).toBe('18.5 TB');
    expect(fmtRate(0)).toBe('0 KB/s');
    expect(fmtRate(0.5)).toBe('512 KB/s');
    expect(fmtRate(12.44)).toBe('12.4 MB/s');
    expect(fmtRate(2048)).toBe('2.0 GB/s');
  });
});

describe('fmtUnit', () => {
  it('uses per-unit precision and spacing', () => {
    expect(fmtUnit(41.26, '°C')).toBe('41.3 °C');
    expect(fmtUnit(12.54, '%')).toBe('12.5%');
    expect(fmtUnit(1480.4, 'RPM')).toBe('1,480 RPM');
    expect(fmtUnit(1.2345, 'V')).toBe('1.235 V');
    expect(fmtUnit(null, 'W')).toBe(NA);
  });
});
