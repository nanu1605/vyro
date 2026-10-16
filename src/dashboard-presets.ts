// Dashboard presets (P24): the layouts Vyro ships, made into saved layouts on first run and used
// by "Reset". Loaded on first run and with the editor.

import { DEFAULT_SPEC, r3, widgetsOf, type Spec } from './dashboard';
import type { Dashboard, Layout } from './types';

export type PresetId = 'default' | 'gaming' | 'work' | 'streaming' | 'benchmark' | 'monitoring' | 'minimal';

export const PRESETS: Record<PresetId, { name: string; widgets: Spec[] }> = {
  default: { name: 'Default', widgets: DEFAULT_SPEC },
  gaming: {
    name: 'Gaming',
    widgets: [
      ['game', 8, 3],
      ['gauge', 4, 3, 'gpu.0'],
      r3('game.fps'),
      r3('game.low1'),
      r3('game.frametime'),
      r3('gpu.0.temp'),
      r3('gpu.0.hotspot'),
      r3('gpu.0.power'),
      r3('cpu.temp'),
      r3('cpu.usage'),
      ['graph', 6, 2, 'gpu.0.temp'],
      ['graph', 6, 2, 'cpu.temp'],
    ],
  },
  work: {
    name: 'Work',
    widgets: [
      ['gauge', 6, 3, 'cpu'],
      ['health', 6, 3],
      r3('cpu.usage'),
      r3('mem.used'),
      r3('disk.read'),
      r3('disk.write'),
      r3('net.down'),
      r3('net.up'),
      r3('net.ping'),
      r3('mem.load'),
      ['card', 6, 2, 'cores'],
      ['events', 6, 2],
    ],
  },
  streaming: {
    name: 'Streaming',
    widgets: [
      ['gauge', 6, 3, 'cpu'],
      ['gauge', 6, 3, 'gpu.0'],
      r3('net.up'),
      r3('net.ping'),
      r3('game.fps'),
      r3('mem.used'),
      r3('cpu.temp'),
      r3('gpu.0.temp'),
      r3('cpu.usage'),
      r3('gpu.0.usage'),
      ['graph', 6, 2, 'net.up'],
      ['graph', 6, 2, 'net.ping'],
    ],
  },
  benchmark: {
    name: 'Benchmark',
    widgets: [
      ['gauge', 6, 3, 'cpu'],
      ['gauge', 6, 3, 'gpu.0'],
      r3('cpu.clock'),
      r3('cpu.temp'),
      r3('cpu.power'),
      r3('power.system'),
      r3('gpu.0.clock'),
      r3('gpu.0.temp'),
      r3('gpu.0.power'),
      r3('fan.cpu'),
      ['graph', 6, 2, 'cpu.temp'],
      ['graph', 6, 2, 'gpu.0.temp'],
      ['card', 12, 2, 'board'],
    ],
  },
  monitoring: {
    name: 'Monitoring',
    widgets: [
      ['health', 4, 2],
      ['events', 8, 2],
      r3('cpu.temp'),
      r3('gpu.0.temp'),
      r3('disk.temp'),
      r3('mem.load'),
      ['history', 6, 3, 'cpu.temp.package'],
      ['history', 6, 3, 'gpu.0.temp.edge'],
      ['card', 4, 2, 'power'],
      ['card', 4, 2, 'board'],
      ['card', 4, 2, 'memory'],
    ],
  },
  minimal: {
    name: 'Minimal',
    widgets: [r3('cpu.temp'), r3('gpu.0.temp'), r3('cpu.usage'), r3('mem.used'), ['health', 12, 1]],
  },
};

export const PRESET_IDS = Object.keys(PRESETS) as PresetId[];
export const isPreset = (p: string | null | undefined): p is PresetId => p != null && p in PRESETS;

export function presetLayout(id: PresetId): Layout {
  return { id, name: PRESETS[id].name, preset: id, locked: false, widgets: widgetsOf(PRESETS[id].widgets) };
}

/** What the Dashboard starts with before the first save: every preset, Default shown. */
export function seed(): Dashboard {
  return { version: 1, active: 'default', layouts: PRESET_IDS.map(presetLayout) };
}
