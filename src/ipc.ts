// Bridge to the Rust core. Inside Tauri this listens to the `sensors` event and invokes
// commands; in a plain browser it falls back to the in-page simulator.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AlertFired, AlertRule, BackendStatus, GameSession, GameSnapshot, Insight, Profile, ProfileSpec, TempUnit, HealthEvent, DisplayInfo, DriveInfo, HardwareId, NetAdapterInfo, HistoryRange, PawnIoStatus, HistorySeries, SensorSnapshot, SensorTree, SystemInfo } from './types';

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

/** Where sensor data comes from: the Vyro service, or the app itself without it. `null` in the
 * browser preview. */
export async function getBackendStatus(): Promise<BackendStatus | null> {
  if (!inTauri) return null;
  return invoke<BackendStatus>('get_backend_status');
}

/** Service connection changes (lost, reconnected). */
export async function onBackendStatus(cb: (s: BackendStatus) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<BackendStatus>('backend-status', (e) => cb(e.payload));
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

/** Logged game sessions, newest first. */
export async function getGameSessions(): Promise<GameSession[]> {
  if (!inTauri) return (await import('./sim-game')).SAMPLE_SESSIONS;
  return invoke<GameSession[]>('get_game_sessions');
}

/** Findings from history (unusual temperatures, throttling, slow fans, power changes). */
export async function getInsights(): Promise<Insight[]> {
  if (!inTauri) {
    const now = Math.floor(Date.now() / 1000);
    return [
      {
        kind: 'unusualTemp', status: 'warn', title: 'CPU running hotter than usual', ts: null,
        detail: 'Over the last 30 minutes: 68 °C at 22 % load. At that load it usually runs near 58 °C (past week). Dust, a changed fan curve, a warmer room or a cooler problem can cause this.',
        metrics: ['cpu.temp.package', 'cpu.usage.total'],
      },
      {
        kind: 'powerChange', status: 'ok', title: 'Power rose to 188 W', ts: now - 5400,
        detail: 'CPU + GPU power went from 45 W to 188 W: GPU #0 +135 W (load 3 % → 97 %), CPU +8 W (load 5 % → 12 %).',
        metrics: ['cpu.power.package', 'gpu.0.power'],
      },
    ];
  }
  return invoke<Insight[]>('get_insights');
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

// Browser preview only: rules live in memory, mirroring the engine's first-run defaults.
let browserRules: AlertRule[] = [
  { id: 'cpu-temp', name: 'CPU temperature', enabled: true, kind: { type: 'threshold', sensor: 'cpu.temp.package', above: true, value: 95 }, forSecs: 30, cooldownSecs: 600, sound: true },
  { id: 'gpu-temp', name: 'GPU temperature', enabled: true, kind: { type: 'threshold', sensor: 'gpu.0.temp.edge', above: true, value: 90 }, forSecs: 30, cooldownSecs: 600, sound: true },
  { id: 'health-problems', name: 'Hardware problems', enabled: true, kind: { type: 'health', problems: true }, forSecs: 0, cooldownSecs: 600, sound: true },
  { id: 'health-warnings', name: 'Health warnings', enabled: false, kind: { type: 'health', problems: false }, forSecs: 0, cooldownSecs: 1800, sound: false },
];

export async function getAlertRules(): Promise<AlertRule[]> {
  if (!inTauri) return JSON.parse(JSON.stringify(browserRules)) as AlertRule[];
  return invoke<AlertRule[]>('get_alert_rules');
}

/** Replaces all rules; the engine validates and saves them. */
export async function setAlertRules(rules: AlertRule[]): Promise<void> {
  if (!inTauri) {
    browserRules = JSON.parse(JSON.stringify(rules)) as AlertRule[];
    return;
  }
  await invoke('set_alert_rules', { rules });
}

export async function getAlertLog(): Promise<AlertFired[]> {
  if (!inTauri) {
    const now = Math.floor(Date.now() / 1000);
    return [{ ts: now - 3890, ruleId: 'cpu-temp', title: 'CPU temperature', message: 'CPU (Tdie) at 95.4 °C (above 95.0 °C for 10 s)', sound: true }];
  }
  return invoke<AlertFired[]>('get_alert_log');
}

export interface AppSettings {
  /** Closing the window keeps Vyro running in the tray. */
  closeToTray: boolean;
  /** How temperatures are shown; the engine always works in °C. */
  temperatureUnit: TempUnit;
  /** Sensor ids left out of the Sensors and History views. */
  hiddenSensors: string[];
  /** Poll rate, notifications, animation and dashboard density. */
  profile: Profile;
  /** Measure frame rates of fullscreen games. */
  gameCapture: boolean;
  /** The companion overlay over the captured game. */
  overlay: OverlaySettings;
}
export type OverlayCorner = 'topLeft' | 'topRight' | 'bottomLeft' | 'bottomRight';
export interface OverlaySettings {
  enabled: boolean;
  corner: OverlayCorner;
  /** Frame-time graph and CPU / GPU readings besides the FPS figures. */
  detailed: boolean;
}
export const DEFAULT_OVERLAY: OverlaySettings = { enabled: false, corner: 'topLeft', detailed: true };
let browserSettings: AppSettings = {
  closeToTray: true, temperatureUnit: 'c', hiddenSensors: [], profile: 'desktop', gameCapture: true, overlay: { ...DEFAULT_OVERLAY },
};

/** What the game overlay shows (`src-tauri/src/overlay.rs`). */
export interface OverlayData {
  game: GameSnapshot;
  detailed: boolean;
  cpuTemp: number | null;
  cpuUsage: number | null;
  gpuTemp: number | null;
  gpuUsage: number | null;
  gpuPower: number | null;
}

/** Overlay readings, every tick while the overlay shows; in a browser, from the simulator. */
export async function onOverlay(cb: (d: OverlayData) => void): Promise<() => void> {
  if (!inTauri) {
    return onSensors((s) => {
      const g = s.gpu[0];
      if (s.game) {
        cb({
          game: s.game, detailed: browserSettings.overlay.detailed, cpuTemp: s.cpu.tempPackage, cpuUsage: s.cpu.usageTotal,
          gpuTemp: g?.tempEdge ?? null, gpuUsage: g?.usage ?? null, gpuPower: g?.powerW ?? null,
        });
      }
    });
  }
  return listen<OverlayData>('overlay', (e) => cb(e.payload));
}
const settingsListeners = new Set<(s: AppSettings) => void>();

/** Settings changed elsewhere (the tray's Profile menu, another view). */
export async function onSettingsChanged(cb: (s: AppSettings) => void): Promise<() => void> {
  if (!inTauri) {
    settingsListeners.add(cb);
    return () => settingsListeners.delete(cb);
  }
  return listen<AppSettings>('settings-changed', (e) => cb(e.payload));
}

// Browser preview: mirrors `vyro_core::profiles`.
const BROWSER_PROFILES: ProfileSpec[] = [
  { id: 'desktop', name: 'Desktop', description: 'Everyday use: live gauges every second, every alert.', pollMs: 1000, notifications: 'all', motion: 'full', dashboard: 'full' },
  { id: 'gaming', name: 'Gaming', description: 'Light while you play: readings every 2 s, only hardware problems notify.', pollMs: 2000, notifications: 'critical', motion: 'reduced', dashboard: 'compact' },
  { id: 'work', name: 'Work', description: 'Readings every 2 s, alerts without sound, calmer animation.', pollMs: 2000, notifications: 'quiet', motion: 'reduced', dashboard: 'full' },
  { id: 'benchmark', name: 'Benchmark', description: 'Readings twice a second for test runs; alerts silent, no animation.', pollMs: 500, notifications: 'quiet', motion: 'off', dashboard: 'full' },
  { id: 'silent', name: 'Silent', description: 'Smallest footprint: readings every 5 s, no notifications (alerts still logged).', pollMs: 5000, notifications: 'off', motion: 'off', dashboard: 'compact' },
];

/** The profile presets. */
export async function getProfiles(): Promise<ProfileSpec[]> {
  if (!inTauri) return BROWSER_PROFILES;
  return invoke<ProfileSpec[]>('get_profiles');
}

export async function getSettings(): Promise<AppSettings> {
  if (!inTauri) return { ...browserSettings };
  return invoke<AppSettings>('get_settings');
}

export async function setSettings(settings: AppSettings): Promise<void> {
  if (!inTauri) {
    browserSettings = { ...settings };
    for (const cb of settingsListeners) cb({ ...browserSettings });
    return;
  }
  await invoke('set_settings', { settings });
}

let browserAutostart = false;

/** Whether Vyro starts (in the tray) at sign-in. */
export async function getAutostart(): Promise<boolean> {
  if (!inTauri) return browserAutostart;
  return invoke<boolean>('get_autostart');
}

export async function setAutostart(enabled: boolean): Promise<void> {
  if (!inTauri) {
    browserAutostart = enabled;
    return;
  }
  await invoke('set_autostart', { enabled });
}

/** Sends a sample Windows notification. */
export async function testAlert(sound: boolean): Promise<void> {
  if (inTauri) await invoke('test_alert', { sound });
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

/** Drives with health data, read fresh by the engine (SMART needs the sensor service). */
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
