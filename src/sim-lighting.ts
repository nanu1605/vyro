// Browser preview of the Lighting view (2.6): OpenRGB with three devices (constructed), and the
// colour following a screen that cycles slowly through a few scenes. `?openrgb=missing` (2.6.1):
// OpenRGB is not on the PC until Set up OpenRGB, which downloads, installs and starts it.
// `?others=msi` (2.6.2): MSI Center's Mystic Light runs, and a board's fan header has no LEDs set.

import type { LightDevice, LightingSettings, LightingStatus, LightLink, OtherLighting, Rgb8, SetupStep } from './types';
import type { LightingView } from './ipc';

let settings: LightingSettings = {
  on: false,
  monitor: 0,
  devices: ['a1b2c3d4e5f60718'],
  speed: 'auto',
  brightness: 100,
  noColour: 'white',
};

const DEVICES: Array<Omit<LightDevice, 'picked'>> = [
  { id: 'a1b2c3d4e5f60718', name: 'Constructed RGB Keyboard', kind: 'Keyboard', leds: 104, direct: true, slow: false, empty: [] },
  { id: 'b2c3d4e5f6071829', name: 'Constructed Gaming Mouse', kind: 'Mouse', leds: 3, direct: true, slow: false, empty: [] },
  { id: 'c3d4e5f607182930', name: 'Constructed DDR5 Module', kind: 'Memory', leds: 8, direct: true, slow: true, empty: [] },
  { id: 'd4e5f60718293041', name: 'Constructed Light Strip', kind: 'LED strip', leds: 30, direct: false, slow: false, empty: [] },
];

const query = typeof location !== 'undefined' ? new URLSearchParams(location.search) : null;
const msi = query?.get('others') === 'msi';
const BOARD: Omit<LightDevice, 'picked'> = {
  id: 'e5f6071829304152',
  name: 'Constructed Motherboard',
  kind: 'Motherboard',
  leds: 6,
  direct: true,
  slow: false,
  empty: ['JRAINBOW1'],
};
const MSI: OtherLighting = {
  name: "MSI Center's Mystic Light",
  services: ['LightKeeperService', 'Mystic_Light_Service'],
  processes: [],
  remedy: "remove Mystic Light in MSI Center's Features",
};

const SCENES: Rgb8[] = [
  { r: 255, g: 40, b: 20 },
  { r: 20, g: 120, b: 255 },
  { r: 40, g: 220, b: 90 },
  { r: 255, g: 170, b: 20 },
];

type Phase = 'missing' | SetupStep | 'starting' | 'connected';
let phase: Phase = query?.get('openrgb') === 'missing' ? 'missing' : 'connected';

function status(): LightingStatus {
  const scene = SCENES[Math.floor(Date.now() / 3000) % SCENES.length]!;
  const connected = settings.on && phase === 'connected';
  let link: LightLink = 'off';
  if (settings.on) {
    link = phase === 'downloading' || phase === 'installing' ? 'settingUp' : phase;
  }
  return {
    link,
    devices: connected ? [...DEVICES, ...(msi ? [BOARD] : [])].map((d) => ({ ...d, picked: settings.devices.includes(d.id) })) : [],
    colour: settings.on ? scene : null,
    capture: settings.on ? 'Desktop Duplication' : null,
    held: null,
    setup: phase === 'downloading' || phase === 'installing' ? phase : null,
    offer: '1.1',
    others: settings.on && msi ? [MSI] : [],
  };
}

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function simSetUpOpenRgb(): Promise<string> {
  if (phase !== 'missing') return 'OpenRGB is on this PC already.';
  phase = 'downloading';
  await pause(1200);
  phase = 'installing';
  await pause(1200);
  phase = 'starting';
  setTimeout(() => (phase = 'connected'), 1500);
  return 'OpenRGB 1.1 is set up: Clokrix finds your devices in a moment.';
}

const view = (): LightingView => ({ settings: { ...settings }, status: status(), monitors: 2 });

export function simLighting(): LightingView {
  return view();
}

export function simSetLighting(next: LightingSettings): LightingView {
  settings = { ...next, brightness: Math.min(100, Math.max(10, next.brightness)) };
  return view();
}

export function simWatchLighting(): LightingStatus {
  return status();
}

export function simOnLighting(cb: (s: LightingStatus) => void): () => void {
  const t = setInterval(() => cb(status()), 500);
  return () => clearInterval(t);
}
