// Devices tab (P22): labels, groups and the battery view. Pure, so it is unit-tested.

import type {
  Battery,
  BatterySource,
  Connection,
  Control,
  DeviceInfo,
  DeviceKind,
  Devices,
  LinkState,
} from './types';

export const KIND_LABEL: Record<DeviceKind, string> = {
  keyboard: 'Keyboard',
  mouse: 'Mouse',
  input: 'Keyboard / mouse',
  controller: 'Controller',
  receiver: 'Wireless receiver',
  headset: 'Headset',
  headphones: 'Headphones',
  speakers: 'Speakers',
  microphone: 'Microphone',
  camera: 'Camera',
  printer: 'Printer',
  phone: 'Phone',
  storage: 'USB storage',
  other: 'Other',
};

const CONNECTION_LABEL: Record<Connection, string> = {
  usb: 'USB',
  bluetooth: 'Bluetooth',
  bluetoothLe: 'Bluetooth LE',
  network: 'Network',
  receiver: 'Through a receiver',
  other: '',
};

export const SOURCE_LABEL: Record<BatterySource, string> = {
  windows: 'as Windows reports it',
  bluetoothLe: 'from the Bluetooth LE Battery Service',
  xinput: 'from XInput (four levels)',
  logitech: "from Logitech's own protocol",
  playStation: 'from the controller',
  hidBattery: 'as the device reports it',
};

const GROUPS: Array<{ id: string; title: string; kinds: DeviceKind[] }> = [
  { id: 'input', title: 'Input', kinds: ['keyboard', 'mouse', 'input', 'controller', 'receiver'] },
  { id: 'audio', title: 'Audio', kinds: ['headset', 'headphones', 'speakers', 'microphone'] },
  { id: 'other', title: 'Other devices', kinds: ['camera', 'printer', 'phone', 'storage', 'other'] },
];

/** Non-empty groups in tab order, devices in the engine's order (connected first). */
export function grouped(list: DeviceInfo[]): Array<{ id: string; title: string; devices: DeviceInfo[] }> {
  return GROUPS.map((g) => ({ id: g.id, title: g.title, devices: list.filter((d) => g.kinds.includes(d.kind)) })).filter(
    (g) => g.devices.length > 0,
  );
}

/** "Bluetooth LE", "USB · wireless", "Through USB Receiver", ... */
export function connectionText(d: DeviceInfo, all: DeviceInfo[] = []): string {
  if (d.connection === 'receiver') {
    const parent = all.find((x) => x.id === d.parent)?.name;
    return parent ? `Through ${parent}` : CONNECTION_LABEL.receiver;
  }
  if (d.connection === 'other') return d.wireless ? 'Wireless' : 'Wired';
  if (d.connection === 'usb' && d.wireless) return 'USB · wireless';
  return CONNECTION_LABEL[d.connection];
}

export function stateText(d: DeviceInfo): string {
  // Paired, and Windows says nothing about its connection (1.4.1).
  if (d.state === 'unknown') return 'Paired';
  // A device behind a receiver that stopped answering is asleep or switched off.
  if (!d.connected) return d.connection === 'receiver' ? 'Asleep or off' : 'Not connected';
  if (d.problem === 22) return 'Disabled';
  if (d.problem != null) return `Problem (code ${d.problem})`;
  return 'Connected';
}

export type Tone = 'ok' | 'warn' | 'hot' | 'none';

/** Under 20 % is amber, under 10 % red (P22 defaults). */
export const LOW = 20;
export const CRITICAL = 10;

const LEVEL: Record<NonNullable<Battery['level']>, { text: string; pct: number; tone: Tone }> = {
  full: { text: 'Full', pct: 100, tone: 'ok' },
  medium: { text: 'Medium', pct: 60, tone: 'ok' },
  low: { text: 'Low', pct: 20, tone: 'warn' },
  empty: { text: 'Empty', pct: 5, tone: 'hot' },
};

export interface BatteryView {
  text: string;
  /** Bar fill, 0–100; null: no bar. */
  pct: number | null;
  tone: Tone;
  /** Where the reading comes from, for the tooltip and screen readers. */
  source: string | null;
  /** On a charger and charging. */
  charging: boolean;
}

export function batteryView(d: DeviceInfo): BatteryView {
  const b = d.state === 'notConnected' ? null : d.battery;
  const none: BatteryView = { text: '—', pct: null, tone: 'none', source: null, charging: false };
  if (b == null) return none;
  const source = SOURCE_LABEL[b.source];
  const charging = b.charging === true;
  if (b.percent != null) {
    const p = Math.max(0, Math.min(100, b.percent));
    return { text: `${p}%`, pct: p, tone: p < CRITICAL ? 'hot' : p < LOW ? 'warn' : 'ok', source, charging };
  }
  if (b.level != null) return { ...LEVEL[b.level], source, charging };
  return none;
}

/** Devices whose battery needs charging soon (the engine's rule, 1.4.1), lowest first. */
export function lowBattery(list: DeviceInfo[]): DeviceInfo[] {
  return list
    .filter((d) => d.low != null && d.state !== 'notConnected')
    .sort((a, b) => (batteryView(a).pct ?? 0) - (batteryView(b).pct ?? 0));
}

/** The low-battery notice (Dashboard strip, Devices dot): the worst tone and one line. */
export function lowNotice(list: DeviceInfo[]): { tone: 'warn' | 'fail'; text: string } | null {
  const low = lowBattery(list);
  if (low.length === 0) return null;
  const tone = low.some((d) => d.low === 'critical') ? 'fail' : 'warn';
  const named = low.slice(0, 2).map((d) => `${d.name} ${batteryView(d).text}`);
  const more = low.length - named.length;
  return { tone, text: named.join(', ') + (more > 0 ? ` and ${more} more` : '') };
}

export function summary(d: Devices): string {
  const n = d.devices.length;
  const on = d.devices.filter((x) => x.connected).length;
  const batt = d.devices.filter((x) => batteryView(x).pct != null).length;
  const parts = [`${n} device${n === 1 ? '' : 's'}`];
  if (on !== n) parts.push(`${on} connected`);
  if (batt > 0) parts.push(`${batt} with battery`);
  return parts.join(' · ');
}

/** The buttons a paired Bluetooth device gets (1.3): Connect while it is off, Disconnect
 * while it is on, and both when Windows does not say (1.4.1). */
export function actions(d: DeviceInfo): Array<'Connect' | 'Disconnect'> {
  if (d.control == null) return [];
  if (d.state === 'unknown') return ['Connect', 'Disconnect'];
  return d.connected ? ['Disconnect'] : ['Connect'];
}

/** A Connect or Disconnect: what was asked, and the connection Windows showed once it had
 * time to change (null while under way). */
export interface Pending {
  want: boolean;
  control: Control;
  /** Which click this is, so an earlier click's timers leave it alone. */
  token: number;
  seen: LinkState | null;
  error: string | null;
}

/** What became of a Connect or Disconnect; null while it is under way. A device that drops its
 * link can connect again by itself as soon as it is used. */
export function outcome(p: Pending, now: DeviceInfo | undefined): string | null {
  if (p.error) return p.error;
  if (p.seen == null) return null;
  if (p.seen === (p.want ? 'connected' : 'notConnected')) {
    if (!p.want && now?.state === 'connected') return 'It came back: the device connected again by itself';
    return p.want ? 'Connected' : 'Disconnected';
  }
  if (p.seen === 'unknown') return "Asked. Windows doesn't say whether it is connected";
  if (p.want) return "Didn't connect: turn it on, or press a button on it";
  return p.control === 'link'
    ? 'Still connected: Windows may connect it again as soon as it is used'
    : "Didn't disconnect";
}

/** Name of the listed device an audio endpoint belongs to. */
export function ownerName(d: Devices, id: string | null): string | null {
  return id == null ? null : (d.devices.find((x) => x.id === id)?.name ?? null);
}

/** Reads the list one at a time (1.4.1 field check): a read asked for while one runs follows
 * it, so every read shows and the newest comes last. Dropping all but the newest kept the
 * Devices tab on "Reading devices…" while new reads kept starting. Returns the function that
 * asks for a read; `busy` is true from the first read until the last one ends. */
export function oneAtATime<T>(
  read: () => Promise<T>,
  show: (value: T) => void,
  fail: (error: unknown) => void,
  busy: (reading: boolean) => void,
): () => void {
  let reading = false;
  let again = false;
  const load = (): void => {
    if (reading) {
      again = true;
      return;
    }
    reading = true;
    busy(true);
    read()
      .then(show, fail)
      .finally(() => {
        reading = false;
        if (again) {
          again = false;
          load();
        } else busy(false);
      });
  };
  return load;
}
