import { describe, expect, it } from 'vitest';
import {
  actionLabel,
  batteryView,
  connectionText,
  grouped,
  lowBattery,
  outcome,
  ownerName,
  stateText,
  summary,
  type Pending,
} from './devices';
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
  control: null,
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

  it('offers Connect or Disconnect to paired Bluetooth devices and says what became of it', () => {
    expect(actionLabel(dev())).toBeNull();
    const buds = dev({ connection: 'bluetooth', wireless: true, control: 'audio' });
    expect(actionLabel(buds)).toBe('Disconnect');
    expect(actionLabel({ ...buds, connected: false })).toBe('Connect');

    const asked = (want: boolean, control: Pending['control'] = 'audio'): Pending => ({
      want,
      control,
      seenOff: false,
      settled: false,
      error: null,
    });
    // Done as soon as a listing shows it; nothing to say yet before that.
    expect(outcome(asked(false), { ...buds, connected: false })).toBe('Disconnected');
    expect(outcome(asked(true), buds)).toBe('Connected');
    expect(outcome(asked(false), buds)).toBeNull();
    // Back by itself after a listing showed it off.
    expect(outcome({ ...asked(false, 'link'), seenOff: true }, buds)).toMatch(/came back/);
    // Given time and nothing changed.
    expect(outcome({ ...asked(true), settled: true }, { ...buds, connected: false })).toMatch(/turn it on/);
    expect(outcome({ ...asked(false, 'link'), settled: true }, buds)).toMatch(/as soon as it is used/);
    expect(outcome({ ...asked(false), settled: true }, buds)).toBe("Didn't disconnect");
    // An error says so at once.
    expect(outcome({ ...asked(false), error: 'Not a paired Bluetooth device' }, buds)).toBe(
      'Not a paired Bluetooth device',
    );
    // The preview sample has both kinds.
    expect(new Set(sampleDevices.devices.map((d) => d.control).filter(Boolean))).toEqual(new Set(['audio', 'link']));
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
