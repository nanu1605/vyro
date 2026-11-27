// Browser preview of the Lighting view (2.6): OpenRGB with three devices (constructed), and the
// colour following a screen that cycles slowly through a few scenes.

import type { LightingSettings, LightingStatus, Rgb8 } from './types';
import type { LightingView } from './ipc';

let settings: LightingSettings = {
  on: false,
  monitor: 0,
  devices: ['a1b2c3d4e5f60718'],
  speed: 'auto',
  brightness: 100,
  noColour: 'white',
};

const DEVICES = [
  { id: 'a1b2c3d4e5f60718', name: 'Constructed RGB Keyboard', kind: 'Keyboard', leds: 104, direct: true, slow: false },
  { id: 'b2c3d4e5f6071829', name: 'Constructed Gaming Mouse', kind: 'Mouse', leds: 3, direct: true, slow: false },
  { id: 'c3d4e5f607182930', name: 'Constructed DDR5 Module', kind: 'Memory', leds: 8, direct: true, slow: true },
  { id: 'd4e5f60718293041', name: 'Constructed Light Strip', kind: 'LED strip', leds: 30, direct: false, slow: false },
];

const SCENES: Rgb8[] = [
  { r: 255, g: 40, b: 20 },
  { r: 20, g: 120, b: 255 },
  { r: 40, g: 220, b: 90 },
  { r: 255, g: 170, b: 20 },
];

function status(): LightingStatus {
  const scene = SCENES[Math.floor(Date.now() / 3000) % SCENES.length]!;
  return {
    link: settings.on ? 'connected' : 'off',
    devices: settings.on ? DEVICES.map((d) => ({ ...d, picked: settings.devices.includes(d.id) })) : [],
    colour: settings.on ? scene : null,
    capture: settings.on ? 'Desktop Duplication' : null,
    held: null,
  };
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
