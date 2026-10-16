import { describe, expect, it } from 'vitest';
import type { RemoteView } from '../types';
import { canonCode, codeFromHash, deviceName, driveRows, fanRows, level, notices, pageTitle, showCode, tempRows, tiles } from './companion';

const VIEW: RemoteView = {
  ts: 1_760_000_000,
  cpu: { usage: 37.4, temp: 92, mhz: 4812, powerW: 88 },
  gpus: [
    { name: 'AMD Radeon Graphics', integrated: true, usage: 2, temp: 45, hotspot: null, mhz: null, powerW: null, vramUsedMb: null, vramTotalMb: null },
    { name: 'NVIDIA GeForce RTX 4070', integrated: false, usage: 91, temp: 70, hotspot: 101, mhz: 2610, powerW: 190, vramUsedMb: 6246, vramTotalMb: 12282 },
  ],
  memory: { usedMb: 16384, totalMb: 32768 },
  drives: [
    { model: 'Samsung SSD 990 PRO 2TB', kind: 'NVMe', temp: 66, usedPct: 25.2, health: 'ok' },
    { model: 'WDC WD20EZRZ', kind: 'SATA', temp: 66, usedPct: null, health: 'warn' },
  ],
  fans: [
    { name: 'CPU Fan', rpm: 1180, pct: 42, pump: false },
    { name: 'AIO', rpm: 2900, pct: null, pump: true },
  ],
  temps: [
    { name: 'System', value: 33 },
    { name: 'VRM', value: 96 },
    { name: 'RAM DIMM_A2', value: 49 },
    { name: 'Socket', value: 50 },
  ],
  power: { totalW: 320, cpuW: 88, gpuW: 190 },
  game: null,
  network: { downMbps: 12.25, upMbps: 1.5, pingMs: 14, lossPct: 0 },
  health: {
    overall: 'fail',
    issues: [
      { part: 'CPU', status: 'fail', message: 'Package at 92 °C' },
      { part: 'Storage', status: 'warn', message: null },
    ],
  },
  alerts: [{ name: 'CPU hot', since: 1_760_000_000 }],
  recent: [],
};

describe('pairing codes', () => {
  it('reads codes however they are typed', () => {
    expect(canonCode('k7qm-4xpa')).toBe('K7QM4XPA');
    expect(canonCode(' K7QM 4XPA ')).toBe('K7QM4XPA');
    expect(canonCode('K7QM–4XPA')).toBe('K7QM4XPA');
    expect(canonCode('O0I1ABCD')).toBeNull();
    expect(canonCode('K7QM')).toBeNull();
    expect(showCode('k7qm4x')).toBe('K7QM-4X');
    expect(showCode('k7q')).toBe('K7Q');
    expect(showCode('K7QM-4XPA-EXTRA')).toBe('K7QM-4XPA');
  });

  it('takes the code from the address only in its own form', () => {
    expect(codeFromHash('#pair=K7QM4XPA')).toBe('K7QM4XPA');
    expect(codeFromHash('#pair=k7qm-4xpa')).toBe('K7QM4XPA');
    expect(codeFromHash('#pair=K7QM4XPA&x=1')).toBeNull();
    expect(codeFromHash('#overlay')).toBeNull();
    expect(codeFromHash('')).toBeNull();
  });

  it('guesses a name from the browser', () => {
    const pixel = 'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Mobile Safari/537.36';
    const iphone = 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1';
    const tab = 'Mozilla/5.0 (Linux; Android 13; SM-X700) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0 Safari/537.36';
    const edge = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36 Edg/129.0';
    expect(deviceName(pixel)).toBe('Android phone · Chrome');
    expect(deviceName(iphone)).toBe('iPhone · Safari');
    expect(deviceName(tab)).toBe('Android tablet · Samsung Internet');
    expect(deviceName(edge)).toBe('Windows PC · Edge');
    expect(deviceName('curl/8.0')).toBe('Device');
  });
});

describe('readings', () => {
  it('grades temperatures against Health’s limits', () => {
    expect(level(89, [90, 99])).toBe('ok');
    expect(level(90, [90, 99])).toBe('warn');
    expect(level(99, [90, 99])).toBe('hot');
    expect(level(null, [90, 99])).toBeNull();
  });

  it('shows the big readings, skipping integrated graphics beside a card', () => {
    const t = tiles(VIEW, 'c');
    expect(t.map((x) => x.id)).toEqual(['cpu', 'gpu0', 'memory', 'power', 'network']);
    expect(t[0]).toMatchObject({ value: '37', unit: '%', level: 'warn' });
    expect(t[0]!.lines[0]).toEqual(['Temperature', '92 °C']);
    expect(t[1]).toMatchObject({ title: 'GPU', sub: 'NVIDIA GeForce RTX 4070', level: 'warn' });
    expect(t[1]!.lines).toContainEqual(['VRAM', '6.1 / 12.0 GB']);
    expect(t[2]).toMatchObject({ value: '50', bar: 50 });
    expect(t[4]!.lines.map((l) => l[0])).toEqual(['Up', 'Ping']);
    expect(tiles(VIEW, 'f')[0]!.lines[0]).toEqual(['Temperature', '198 °F']);
    const igpu = tiles({ ...VIEW, gpus: [VIEW.gpus[0]!] }, 'c');
    expect(igpu[1]).toMatchObject({ title: 'GPU', sub: 'AMD Radeon Graphics' });
  });

  it('lists temperatures, fans and drives with their levels', () => {
    expect(tempRows(VIEW, 'c').map((r) => [r.name, r.level])).toEqual([
      ['System', 'ok'],
      ['VRM', 'warn'],
      ['RAM DIMM_A2', 'ok'],
      ['Socket', null],
    ]);
    expect(fanRows(VIEW)).toEqual([
      { name: 'CPU Fan', value: '1,180 rpm', level: null, note: '42 %' },
      { name: 'AIO (pump)', value: '2,900 rpm', level: null, note: null },
    ]);
    const drives = driveRows(VIEW, 'c');
    expect(drives[0]).toMatchObject({ level: 'ok', note: '25 % full' });
    // SATA at 66 °C, and Health warns.
    expect(drives[1]).toMatchObject({ level: 'warn' });
  });

  it('heads the page with alerts, then problems, and counts alerts in the title', () => {
    expect(notices(VIEW)).toEqual([
      { level: 'hot', text: 'Alert: CPU hot' },
      { level: 'hot', text: 'CPU: Package at 92 °C' },
      { level: 'warn', text: 'Storage: warning' },
    ]);
    expect(pageTitle('GAMING-PC', VIEW)).toBe('(1) GAMING-PC · Vyro');
    expect(pageTitle('', null)).toBe('Vyro');
  });
});
