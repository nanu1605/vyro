// Bridge to the Rust core. Inside Tauri this listens to the `sensors` event and invokes
// commands; in a plain browser it falls back to the in-page simulator.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { DisplayInfo, DriveInfo, HardwareId, HistoryRange, HistorySeries, SensorSnapshot, SensorTree, SystemInfo } from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Subscribes to live snapshots, delivering the latest one immediately if available. */
export async function onSensors(cb: (s: SensorSnapshot) => void): Promise<() => void> {
  if (!inTauri) {
    const { startSim } = await import('./sim');
    return startSim(cb);
  }
  const unlisten = await listen<SensorSnapshot>('sensors', (e) => cb(e.payload));
  const latest = await invoke<SensorSnapshot | null>('get_latest');
  if (latest) cb(latest);
  return unlisten;
}

/** Tells the engine whether the UI is visible, so it can poll slower while hidden. */
export function setUiVisible(visible: boolean): void {
  if (inTauri) void invoke('set_ui_visible', { visible }).catch(() => {});
}

export function queryHistory(metric: string, range: HistoryRange): Promise<HistorySeries> {
  return invoke<HistorySeries>('query_history', { metric, range });
}

export function getSensorTree(): Promise<SensorTree> {
  return invoke<SensorTree>('get_sensor_tree');
}

export async function getHardwareId(): Promise<HardwareId> {
  if (!inTauri) return (await import('./sample')).sampleHardware;
  return invoke<HardwareId>('get_hardware_id');
}

export async function getSystemInfo(): Promise<SystemInfo> {
  if (!inTauri) {
    const { sampleSystem } = await import('./sample');
    const now = Math.floor(Date.now() / 1000);
    return { ...sampleSystem, uptimeSecs: sampleSystem.bootTime == null ? null : now - sampleSystem.bootTime };
  }
  return invoke<SystemInfo>('get_system_info');
}

export async function getDisplays(): Promise<DisplayInfo[]> {
  if (!inTauri) return (await import('./sample-displays')).sampleDisplays;
  return invoke<DisplayInfo[]>('get_displays');
}

/** Fires after monitors are plugged, unplugged or reconfigured (debounced by the engine). */
export async function onDisplaysChanged(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen('displays-changed', () => cb());
}

/** Drives with health data, read fresh by the engine (SMART needs the app elevated). */
export async function getDrives(): Promise<DriveInfo[]> {
  if (!inTauri) return (await import('./sample-drives')).sampleDrives;
  return invoke<DriveInfo[]>('get_drives');
}
