// Bridge to the Rust core. Inside Tauri this listens to the `sensors` event and invokes
// commands; in a plain browser it falls back to the in-page simulator.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { HealthEvent, DisplayInfo, DriveInfo, HardwareId, NetAdapterInfo, HistoryRange, PawnIoStatus, HistorySeries, SensorSnapshot, SensorTree, SystemInfo } from './types';

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

export async function queryHistory(metric: string, range: HistoryRange): Promise<HistorySeries> {
  if (!inTauri) return (await import('./sim-tree')).simHistory(metric, range);
  return invoke<HistorySeries>('query_history', { metric, range });
}

/** The full sensor registry; in a browser it is derived from the simulated `snap`. */
export async function getSensorTree(snap: SensorSnapshot | null): Promise<SensorTree> {
  if (!inTauri) return snap ? (await import('./sim-tree')).simTree(snap) : { groups: [] };
  return invoke<SensorTree>('get_sensor_tree');
}

/** Recent health changes, oldest first. */
export async function getHealthEvents(): Promise<HealthEvent[]> {
  if (!inTauri) {
    const now = Math.floor(Date.now() / 1000);
    return [
      { ts: now - 5400, subsystem: 'Storage', status: 'warn', message: 'ST2000DM008-2UB102 reports a health warning (details in Storage)' },
      { ts: now - 3900, subsystem: 'CPU', status: 'warn', message: 'CPU running hot: 91 °C' },
      { ts: now - 3720, subsystem: 'CPU', status: 'ok', message: 'Cleared: CPU running hot: 91 °C' },
      { ts: now - 600, subsystem: 'GPU', status: 'warn', message: 'Simulated GPU (browser) is thermal throttling' },
      { ts: now - 540, subsystem: 'GPU', status: 'ok', message: 'Cleared: Simulated GPU (browser) is thermal throttling' },
    ];
  }
  return invoke<HealthEvent[]>('get_health_events');
}

/** Restarts min / max / avg for every sensor. */
export async function resetSensorStats(): Promise<void> {
  if (!inTauri) return (await import('./sim-tree')).resetSimStats();
  await invoke('reset_sensor_stats');
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

/** Adapters with addresses and Wi-Fi details (rates and ping arrive in `sensors`). */
export async function getNetwork(): Promise<NetAdapterInfo[]> {
  if (!inTauri) return (await import('./sample-network')).sampleNetwork;
  return invoke<NetAdapterInfo[]>('get_network');
}

/** Changes the ping target; rejects with a message for an invalid host. */
export async function setPingHost(host: string): Promise<void> {
  if (!inTauri) {
    if (!/^[A-Za-z0-9.-]{1,253}$/.test(host.trim())) throw new Error('Enter an IPv4 address or a host name');
    return;
  }
  await invoke('set_ping_host', { host });
}

/** PawnIO driver state; the browser preview behaves as if it were installed. */
export async function getPawnIoStatus(): Promise<PawnIoStatus> {
  if (!inTauri) return { state: 'ready', version: '2.0.0 (simulated)', detail: null };
  return invoke<PawnIoStatus>('get_pawnio_status');
}
