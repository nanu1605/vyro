import { describe, expect, it } from 'vitest';
import { batteryView, connectionText, grouped, lowBattery, ownerName, stateText, summary } from './devices';
import { sampleDevices } from './sample-devices';
import type { DeviceInfo } from './types';

const dev = (over: Partial<DeviceInfo> = {}): DeviceInfo => ({
  id: 'a',
  name: 'Thing',
  kind: 'mouse',
  manufacturer: null,
  connection: 'usb',
  wireless: false,
  connected: true,
  battery: null,
  problem: null,
  parent: null,
  ...over,
});

describe('devices', () => {
  it('groups input, audio and the rest, keeping order and dropping empty groups', () => {
    const g = grouped([
      dev({ id: '1', kind: 'headset' }),
      dev({ id: '2', kind: 'keyboard' }),
      dev({ id: '3', kind: 'receiver' }),
      dev({ id: '4', kind: 'headphones' }),
    ]);
    expect(g.map((x) => [x.id, x.devices.map((d) => d.id)])).toEqual([
      ['input', ['2', '3']],
      ['audio', ['1', '4']],
    ]);
    expect(grouped([])).toEqual([]);
  });

  it('says how a device connects and its state', () => {
    expect(connectionText(dev({ connection: 'bluetoothLe', wireless: true }))).toBe('Bluetooth LE');
    expect(connectionText(dev({ connection: 'usb', wireless: true }))).toBe('USB · wireless');
    expect(connectionText(dev({ connection: 'other', wireless: true }))).toBe('Wireless');
    expect(connectionText(dev({ connection: 'other' }))).toBe('Wired');
    expect(stateText(dev())).toBe('Connected');
    expect(stateText(dev({ connected: false }))).toBe('Not connected');
    expect(stateText(dev({ problem: 22 }))).toBe('Disabled');
    expect(stateText(dev({ problem: 43 }))).toBe('Problem (code 43)');
  });

  it('names the receiver a device is paired to, and calls a silent one asleep', () => {
    const receiver = dev({ id: 'r', name: 'USB Receiver', kind: 'receiver', wireless: true });
    const mouse = dev({ id: 'm', connection: 'receiver', wireless: true, parent: 'r' });
    expect(connectionText(mouse, [receiver, mouse])).toBe('Through USB Receiver');
    expect(connectionText(mouse)).toBe('Through a receiver');
    expect(stateText({ ...mouse, connected: false })).toBe('Asleep or off');
  });

  it('shows battery as a number or a level, coloured by how low, and never for a device that is off', () => {
    const pct = (p: number) =>
      batteryView(dev({ battery: { percent: p, level: null, charging: null, source: 'windows' } }));
    expect(pct(64)).toMatchObject({ text: '64%', pct: 64, tone: 'ok' });
    expect(pct(19)).toMatchObject({ text: '19%', tone: 'warn' });
    expect(pct(20).tone).toBe('ok');
    expect(pct(9).tone).toBe('hot');
    expect(pct(10).tone).toBe('warn');
    expect(pct(64).source).toMatch(/Windows/);
    expect(batteryView(dev({ battery: { percent: null, level: 'low', charging: null, source: 'xinput' } }))).toMatchObject({
      text: 'Low',
      tone: 'warn',
    });
    expect(batteryView(dev())).toEqual({ text: '—', pct: null, tone: 'none', source: null, charging: false });
    expect(
      batteryView(dev({ connected: false, battery: { percent: 50, level: null, charging: null, source: 'windows' } }))
        .text,
    ).toBe('—');
    const charging = batteryView(dev({ battery: { percent: 46, level: null, charging: true, source: 'logitech' } }));
    expect(charging).toMatchObject({ text: '46%', charging: true });
    expect(charging.source).toMatch(/Logitech/);
  });

  it('lists low batteries lowest first and sums the tab up', () => {
    const list = [
      dev({ id: 'a', battery: { percent: 15, level: null, charging: null, source: 'windows' } }),
      dev({ id: 'b', battery: { percent: 5, level: null, charging: null, source: 'bluetoothLe' } }),
      dev({ id: 'c', battery: { percent: 80, level: null, charging: null, source: 'windows' } }),
      dev({ id: 'd', connected: false }),
      // Low but on its charger: not a warning.
      dev({ id: 'e', battery: { percent: 8, level: null, charging: true, source: 'logitech' } }),
    ];
    expect(lowBattery(list).map((d) => d.id)).toEqual(['b', 'a']);
    expect(summary({ devices: list, audio: [] })).toBe('5 devices · 4 connected · 4 with battery');
    expect(summary({ devices: [dev()], audio: [] })).toBe('1 device');
  });

  it('names the device an endpoint belongs to', () => {
    const s = sampleDevices;
    const mic = s.audio.find((a) => a.name === 'Microphone (Arctis Nova 7)')!;
    expect(ownerName(s, mic.device)).toBe('Arctis Nova 7');
    expect(ownerName(s, null)).toBeNull();
    // The preview sample covers every battery source and a device that is off.
    expect(new Set(s.devices.flatMap((d) => (d.battery ? [d.battery.source] : [])))).toEqual(
      new Set(['windows', 'bluetoothLe', 'xinput', 'logitech', 'playStation']),
    );
    expect(s.devices.some((d) => d.connection === 'receiver' && d.parent != null)).toBe(true);
    expect(s.devices.some((d) => !d.connected)).toBe(true);
  });
});
