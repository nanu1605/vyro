// Devices tab (P22): labels, groups and the battery view. Pure, so it is unit-tested.

import type { Battery, BatterySource, Connection, DeviceInfo, DeviceKind, Devices } from './types';

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
  const b = d.connected ? d.battery : null;
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

/** Connected devices whose battery is low and not charging, lowest first. */
export function lowBattery(list: DeviceInfo[]): DeviceInfo[] {
  return list
    .filter((d) => !batteryView(d).charging && (batteryView(d).tone === 'warn' || batteryView(d).tone === 'hot'))
    .sort((a, b) => (batteryView(a).pct ?? 0) - (batteryView(b).pct ?? 0));
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

/** Name of the listed device an audio endpoint belongs to. */
export function ownerName(d: Devices, id: string | null): string | null {
  return id == null ? null : (d.devices.find((x) => x.id === id)?.name ?? null);
}
