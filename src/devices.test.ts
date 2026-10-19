import { describe, expect, it } from 'vitest';
import {
  actions,
  batteryView,
  connectionText,
  grouped,
  lowBattery,
  lowNotice,
  oneAtATime,
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
  state: 'connected',
  battery: null,
  low: null,
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
    expect(stateText(dev({ connected: false, state: 'notConnected' }))).toBe('Not connected');
    // Windows says nothing (1.4.1): paired, not "Connected".
    expect(stateText(dev({ connected: false, state: 'unknown' }))).toBe('Paired');
    expect(stateText(dev({ problem: 22 }))).toBe('Disabled');
    expect(stateText(dev({ problem: 43 }))).toBe('Problem (code 43)');
  });

  it('offers Connect or Disconnect to paired Bluetooth devices and says what became of it', () => {
    expect(actions(dev())).toEqual([]);
    const buds = dev({ connection: 'bluetooth', wireless: true, control: 'audio' });
    const off = { ...buds, connected: false, state: 'notConnected' as const };
    expect(actions(buds)).toEqual(['Disconnect']);
    expect(actions(off)).toEqual(['Connect']);
    // Windows says nothing: both, so neither is out of reach (1.4.1).
    expect(actions({ ...buds, connected: false, state: 'unknown' })).toEqual(['Connect', 'Disconnect']);

    const asked = (want: boolean, control: Pending['control'] = 'audio'): Pending => ({
      want,
      control,
      token: 1,
      seen: null,
      error: null,
    });
    // Under way until the engine answers with what Windows showed.
    expect(outcome(asked(false), buds)).toBeNull();
    expect(outcome({ ...asked(false), seen: 'notConnected' }, off)).toBe('Disconnected');
    expect(outcome({ ...asked(true), seen: 'connected' }, buds)).toBe('Connected');
    // Off when checked, back since.
    expect(outcome({ ...asked(false, 'link'), seen: 'notConnected' }, buds)).toMatch(/came back/);
    // Nothing changed in the time Windows was given.
    expect(outcome({ ...asked(true), seen: 'notConnected' }, off)).toMatch(/turn it on/);
    expect(outcome({ ...asked(false, 'link'), seen: 'connected' }, buds)).toMatch(/as soon as it is used/);
    expect(outcome({ ...asked(false), seen: 'connected' }, buds)).toBe("Didn't disconnect");
    expect(outcome({ ...asked(false), seen: 'unknown' }, buds)).toMatch(/doesn't say/);
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
      batteryView(
        dev({
          connected: false,
          state: 'notConnected',
          battery: { percent: 50, level: null, charging: null, source: 'windows' },
        }),
      ).text,
    ).toBe('—');
    const charging = batteryView(dev({ battery: { percent: 46, level: null, charging: true, source: 'logitech' } }));
    expect(charging).toMatchObject({ text: '46%', charging: true });
    expect(charging.source).toMatch(/Logitech/);
  });

  it('lists low batteries lowest first, words the notice and sums the tab up', () => {
    const list = [
      dev({ id: 'a', name: 'Mouse', low: 'low', battery: { percent: 15, level: null, charging: null, source: 'windows' } }),
      dev({
        id: 'b',
        name: 'Pad',
        low: 'critical',
        battery: { percent: 5, level: null, charging: null, source: 'bluetoothLe' },
      }),
      dev({ id: 'c', battery: { percent: 80, level: null, charging: null, source: 'windows' } }),
      dev({ id: 'd', connected: false, state: 'notConnected' }),
      // Low but on its charger: the engine does not mark it.
      dev({ id: 'e', battery: { percent: 8, level: null, charging: true, source: 'logitech' } }),
    ];
    expect(lowBattery(list).map((d) => d.id)).toEqual(['b', 'a']);
    expect(lowNotice(list)).toEqual({ tone: 'fail', text: 'Pad 5%, Mouse 15%' });
    expect(lowNotice([list[0]!])).toEqual({ tone: 'warn', text: 'Mouse 15%' });
    expect(lowNotice([...list, { ...list[0]!, id: 'f', name: 'Keys' }])?.text).toBe('Pad 5%, Mouse 15% and 1 more');
    expect(lowNotice(list.slice(2))).toBeNull();
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

describe('reading the list one at a time', () => {
  /** A read the test answers by hand. */
  function reads() {
    const started: Array<{ answer: (v: number) => void; refuse: (e: unknown) => void }> = [];
    const read = () =>
      new Promise<number>((answer, refuse) => {
        started.push({ answer, refuse });
      });
    const nth = (i: number) => {
      const r = started[i];
      if (!r) throw new Error(`read ${i + 1} never started`);
      return r;
    };
    return { started, read, nth };
  }
  const settle = () => new Promise((r) => setTimeout(r, 0));

  it('keeps showing reads while new ones keep being asked for (the stuck tab)', async () => {
    const { read, nth } = reads();
    const shown: number[] = [];
    const load = oneAtATime(read, (v) => shown.push(v), () => {}, () => {});
    // Changes keep asking for reads, each before the last one answers: every answer shows.
    for (let i = 1; i <= 4; i++) {
      load();
      load();
      nth(i - 1).answer(i);
      await settle();
      expect(shown).toEqual(Array.from({ length: i }, (_, k) => k + 1));
    }
  });

  it('runs one read at a time and one more for all asked meanwhile', async () => {
    const { started, read, nth } = reads();
    const shown: number[] = [];
    const busy: boolean[] = [];
    const load = oneAtATime(read, (v) => shown.push(v), () => {}, (b) => busy.push(b));
    load();
    load();
    load();
    load();
    expect(started).toHaveLength(1);
    nth(0).answer(1);
    await settle();
    expect(started).toHaveLength(2);
    nth(1).answer(2);
    await settle();
    expect(started).toHaveLength(2);
    expect(shown).toEqual([1, 2]);
    expect(busy).toEqual([true, true, false]);
  });

  it('a read that fails says so and the next one still runs', async () => {
    const { read, nth } = reads();
    const shown: number[] = [];
    const failed: unknown[] = [];
    const load = oneAtATime(read, (v) => shown.push(v), (e) => failed.push(e), () => {});
    load();
    load();
    nth(0).refuse('no');
    await settle();
    nth(1).answer(5);
    await settle();
    expect(failed).toEqual(['no']);
    expect(shown).toEqual([5]);
  });
});
