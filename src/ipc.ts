// Bridge to the Rust core. Inside Tauri this listens to the `sensors` event and invokes
// commands; in a plain browser it falls back to the in-page simulator.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { evaluate } from './curve';
import { DEFAULT_GAMES, effectiveFor } from './games';
import { DEFAULT_OVERLAY, overlayData, type OverlayCorner, type OverlayData, type OverlaySettings } from './overlay';
import { savedLook, type AccentChoice, type OpenOn, type ThemeId } from './themes';
import { EMPTY as NO_WIDGETS, type Widgets } from './widgets';
import type { AlertFired, AlertRule, BackendStatus, DiagReport, DiagSummary, BenchMode, BenchProgress, BenchRun, Preflight, RunSummary, Effective, GameAutomation, FanConfig, FanEvent, FanEventKind, FanStatus, HeaderStatus, GameSession, GameSnapshot, GameTimeline, WindowSeries, Insights, Profile, ProfileSpec, ScoreReport, TempUnit, HealthEvent, Control, Dashboard, DeviceInfo, Devices, LinkState, PhoneAction, PowerState, WakeReadiness, DisplayInfo, DriveInfo, HardwareChange, HardwareId, NetAdapterInfo, HistoryRange, PawnIoStatus, HistorySeries, RemoteBrief, RemoteStatus, SensorSnapshot, SensorTree, SystemInfo } from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Subscribes to live snapshots, delivering the latest one immediately if available. */
export async function onSensors(cb: (s: SensorSnapshot) => void): Promise<() => void> {
  if (!inTauri) {
    const { startSim } = await import('./sim');
    const { SIM_ATTENTION } = await import('./sim-insights');
    const { simAlertState } = await import('./sim-alerts');
    const rules = await simRules();
    // The simulated board's fan control, the preview's findings and the alert rules that hold,
    // as the service reports them in each snapshot.
    return startSim((s) => {
      const withAlerts = { ...s, alertState: simAlertState(rules(), s.ts) };
      const f = simFanStatus(withAlerts);
      cb({
        ...withAlerts,
        fanControl: { active: f.active, fullSpeed: f.active && f.overrideReason != null },
        insights: { attention: SIM_ATTENTION },
      });
    });
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

/** A logged session with its samples and markers; `null` once it is gone. */
export async function getGameTimeline(id: number): Promise<GameTimeline | null> {
  if (!inTauri) return (await import('./sim-replay')).simTimeline(id);
  return invoke<GameTimeline | null>('get_game_timeline', { id });
}

/** Sensor history from `from` to `to` (unix seconds): the context of a moment in a replay. */
export async function getHistoryWindow(metrics: string[], from: number, to: number): Promise<WindowSeries[]> {
  if (!inTauri) {
    const { simWindow } = await import('./sim-tree');
    return metrics.map((m) => simWindow(m, from, to));
  }
  return invoke<WindowSeries[]>('get_history_window', { metrics, from: Math.floor(from), to: Math.ceil(to) });
}

// Browser preview only: a simulated reference board (MS-7E24) whose fan configuration lives in
// memory. Duties follow the configuration at once (no ramp, no hysteresis).
let browserFans: FanConfig = {
  enabled: false, headers: [], emergencyCpuC: 95, emergencyGpuC: 90, pumps: [], labels: {}, profiles: {},
};
let browserFanEvents: FanEvent[] = [];
function simFanEvent(kind: FanEventKind, message: string): void {
  browserFanEvents = [...browserFanEvents.slice(-99), { ts: Math.floor(Date.now() / 1000), kind, message }];
}
const SIM_PUMP_MIN = 60;
const SIM_HEADERS = [
  { index: 0, name: 'CPU Fan', kind: 'fan', minPct: 20 },
  { index: 1, name: 'Pump Fan', kind: 'pump', minPct: 60 },
  { index: 2, name: 'System Fan 1', kind: 'fan', minPct: 20 },
  { index: 3, name: 'System Fan 2', kind: 'fan', minPct: 20 },
  { index: 4, name: 'System Fan 3', kind: 'fan', minPct: 20 },
] as const;

function simFanStatus(snap: SensorSnapshot | null): FanStatus {
  const cpu = snap?.cpu.tempPackage ?? null;
  // An alert's fan boost (P18): the largest any rule that holds asks for.
  const boost = (snap?.alertState?.active ?? [])
    .filter((a) => a.fanBoost != null)
    .reduce<{ pct: number; rule: string } | null>((b, a) => (b && b.pct >= a.fanBoost! ? b : { pct: a.fanBoost!, rule: a.name }), null);
  const hot = cpu != null && cpu >= browserFans.emergencyCpuC;
  const headers: HeaderStatus[] = SIM_HEADERS.map((h) => {
    const list = browserFans.profiles[browserSettings.profile] ?? browserFans.headers;
    const mode = list.find((c) => c.index === h.index)?.mode ?? { mode: 'bios' as const };
    const pump = h.kind === 'pump' || browserFans.pumps.includes(h.index);
    const driven =
      browserFans.enabled && mode.mode !== 'bios' && (h.kind !== 'pump' || browserFans.pumps.includes(h.index));
    let target: number | null = null;
    let source: number | null = null;
    if (driven) {
      if (hot) target = 100;
      else if (mode.mode === 'fixed') target = Math.min(100, mode.pct + (boost?.pct ?? 0));
      else if (mode.mode === 'curve') {
        source = mode.curve.source === 'cpu.temp.package' ? cpu : 45;
        target = source == null ? 100 : Math.min(100, evaluate(mode.curve.points, source) + (boost?.pct ?? 0));
      }
      if (target != null) target = Math.max(pump ? Math.max(h.minPct, SIM_PUMP_MIN) : h.minPct, target);
    }
    const duty = target ?? (h.kind === 'pump' ? 100 : h.index === 0 ? 58 + (cpu ?? 40) * 0.1 : 60);
    const rpm = h.index === 0 ? Math.round(300 + duty * 17) : h.index === 1 ? Math.round(1500 + duty * 29.6) : 0;
    return {
      ...h, controllable: true, rpm, dutyPct: duty, software: driven, targetPct: target,
      sourceC: source, connected: rpm > 0,
    };
  });
  return {
    capability: { state: 'writable', board: 'MSI B650M GAMING PLUS WIFI' },
    config: JSON.parse(JSON.stringify(browserFans)) as FanConfig,
    active: headers.some((h) => h.software),
    overrideReason: hot && browserFans.enabled ? `CPU at ${Math.round(cpu)} °C (limit ${browserFans.emergencyCpuC} °C): fans at full speed` : null,
    lastError: null,
    headers,
    pumpMinPct: SIM_PUMP_MIN,
    profile: browserSettings.profile,
    boost: headers.some((h) => h.software) ? boost : null,
  };
}

/** Fan control: capability, configuration and each header's state. The browser preview
 * simulates the reference board from `snap`. */
export async function getFanStatus(snap: SensorSnapshot | null): Promise<FanStatus> {
  if (!inTauri) return simFanStatus(snap);
  return invoke<FanStatus>('get_fan_status');
}

/** Validates, saves and applies a fan configuration (the engine refuses invalid ones). */
export async function setFanConfig(config: FanConfig): Promise<void> {
  if (!inTauri) {
    const lists = [config.headers, ...Object.values(config.profiles).filter((l) => l != null)];
    if (lists.some((l) => l.some((h) => h.index === 1 && h.mode.mode !== 'bios')) && !config.pumps.includes(1)) {
      throw new Error('confirm that Pump Fan drives your pump before Vyro controls it');
    }
    if (!browserFans.enabled && config.enabled) simFanEvent('on', 'Fan control turned on');
    if (browserFans.enabled && !config.enabled) simFanEvent('off', 'Fan control turned off');
    browserFans = JSON.parse(JSON.stringify(config)) as FanConfig;
    return;
  }
  await invoke('set_fan_config', { config });
}

/** Turns fan control off and hands every header back to the BIOS. */
export async function restoreFans(): Promise<void> {
  if (!inTauri) {
    browserFans = { ...browserFans, enabled: false };
    simFanEvent('bios', 'Returned to BIOS: fan control turned off');
    return;
  }
  await invoke('restore_fans');
}

/** The fan control log, newest last. */
export async function getFanEvents(): Promise<FanEvent[]> {
  if (!inTauri) return [...browserFanEvents];
  return invoke<FanEvent[]>('get_fan_events');
}

/** The intelligence engine's findings and vital signs (P16). */
export async function getInsights(): Promise<Insights> {
  if (!inTauri) return (await import('./sim-insights')).simInsights();
  return invoke<Insights>('get_insights');
}

/** The Performance Score now with its inputs, and the daily scores of the past year. */
export async function getPerformanceScore(): Promise<ScoreReport> {
  if (!inTauri) return (await import('./sim-score')).simScore();
  return invoke<ScoreReport>('get_performance_score');
}

// Diagnostics (P21): the engine works out and keeps reports; the app writes the exports.

/** Runs the diagnostics (about a second) and returns the kept report. */
export async function runDiagnostics(): Promise<DiagReport> {
  if (!inTauri) return (await import('./sim-diag')).simRunDiagnostics();
  return invoke<DiagReport>('run_diagnostics');
}

export async function getDiagnosticRuns(): Promise<DiagSummary[]> {
  if (!inTauri) return (await import('./sim-diag')).simDiagnosticRuns();
  return invoke<DiagSummary[]>('get_diagnostic_runs');
}

export async function getDiagnosticRun(id: number): Promise<DiagReport | null> {
  if (!inTauri) return (await import('./sim-diag')).simDiagnosticRun(id);
  return invoke<DiagReport | null>('get_diagnostic_run', { id });
}

export async function deleteDiagnostic(id: number): Promise<boolean> {
  if (!inTauri) return (await import('./sim-diag')).simDeleteDiagnostic(id);
  return invoke<boolean>('delete_diagnostic', { id });
}

/** Writes an export to Documents\Vyro Reports; returns its path. */
export async function exportDiagnostics(
  id: number,
  format: 'pdf' | 'html' | 'md' | 'json',
  serialNumbers: boolean,
  unit: TempUnit,
  when: string,
  stamp: string,
): Promise<string> {
  if (!inTauri) return `C:\\Users\\owner\\Documents\\Vyro Reports\\vyro-diagnostics-${stamp}.${format}`;
  return invoke<string>('export_diagnostics', { id, format, serialNumbers, unit, when, stamp });
}

/** Shows an exported report in Explorer. */
export async function revealReport(path: string): Promise<void> {
  if (!inTauri) return;
  return invoke('reveal_report', { path });
}

/** Saves a share card PNG to `Pictures\Vyro` and returns its path; in a browser, downloads it. */
export async function saveShareCard(png: Uint8Array, name: string): Promise<string> {
  if (!inTauri) {
    const url = URL.createObjectURL(new Blob([png as BlobPart], { type: 'image/png' }));
    const a = Object.assign(document.createElement('a'), { href: url, download: name });
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    return name;
  }
  return invoke<string>('save_share_card', png, { headers: { 'x-file-name': name } });
}

/** Shows a saved share card in Explorer. */
export async function revealCard(path: string): Promise<void> {
  if (!inTauri) return;
  return invoke('reveal_card', { path });
}

// Benchmark mode (P20): the run happens in the app; results are kept by the engine.

/** What a run would use, and what stands in its way now. */
export async function getBenchmarkPreflight(): Promise<Preflight> {
  if (!inTauri) return (await import('./sim-bench')).simPreflight();
  return invoke<Preflight>('benchmark_preflight');
}

export async function startBenchmark(mode: BenchMode): Promise<void> {
  if (!inTauri) return (await import('./sim-bench')).simStart(mode);
  return invoke('start_benchmark', { mode });
}

export async function stopBenchmark(): Promise<void> {
  if (!inTauri) return (await import('./sim-bench')).simStop();
  return invoke('stop_benchmark');
}

/** Where the run is, while one goes on. */
export async function getBenchmarkProgress(): Promise<BenchProgress | null> {
  if (!inTauri) return (await import('./sim-bench')).simProgress();
  return invoke<BenchProgress | null>('get_benchmark_progress');
}

/** Kept runs, newest first. */
export async function getBenchmarkRuns(): Promise<RunSummary[]> {
  if (!inTauri) return (await import('./sim-bench')).simRuns();
  return invoke<RunSummary[]>('get_benchmark_runs');
}

export async function getBenchmarkRun(id: number): Promise<BenchRun | null> {
  if (!inTauri) return (await import('./sim-bench')).simRun(id);
  return invoke<BenchRun | null>('get_benchmark_run', { id });
}

export async function deleteBenchmark(id: number): Promise<boolean> {
  if (!inTauri) return (await import('./sim-bench')).simDelete(id);
  return invoke<boolean>('delete_benchmark', { id });
}

/** About four times a second while a run goes on (and the window shows). */
export async function onBenchProgress(cb: (p: BenchProgress) => void): Promise<() => void> {
  if (!inTauri) return (await import('./sim-bench')).onSimProgress(cb);
  return listen<BenchProgress>('bench-progress', (e) => cb(e.payload));
}

/** The finished or stopped run, once kept. */
export async function onBenchDone(cb: (r: BenchRun) => void): Promise<() => void> {
  if (!inTauri) return (await import('./sim-bench')).onSimDone(cb);
  return listen<BenchRun>('bench-done', (e) => cb(e.payload));
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

// Browser preview only: rules live in memory, starting from the preview's (`sim-alerts.ts`).
let browserRules: AlertRule[] | null = null;
async function simRules(): Promise<() => AlertRule[]> {
  if (!browserRules) browserRules = JSON.parse(JSON.stringify((await import('./sim-alerts')).SIM_RULES)) as AlertRule[];
  return () => browserRules ?? [];
}

export async function getAlertRules(): Promise<AlertRule[]> {
  if (!inTauri) return JSON.parse(JSON.stringify((await simRules())())) as AlertRule[];
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
  if (!inTauri) return (await import('./sim-alerts')).simAlertLog(Math.floor(Date.now() / 1000));
  return invoke<AlertFired[]>('get_alert_log');
}

/** A sensor's usual reading now, for "above usual" conditions; `null` until there is about a
 * day of history. */
export async function getAlertUsual(sensor: string): Promise<number | null> {
  if (!inTauri) return (await import('./sim-alerts')).simUsual(sensor);
  return invoke<number | null>('get_alert_usual', { sensor });
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
  /** Look for a new release daily (release builds only). */
  checkUpdates: boolean;
  /** Automatic game profiles and each saved game's own settings. */
  games: GameAutomation;
  /** Colours (P25). */
  theme: ThemeId;
  accent: AccentChoice;
  /** Which tab the window opens on, at launch and from the tray. */
  openOn: OpenOn;
  /** My PC (P23): 3-D or the flat plan, and the owner's GPU shapes by card key. */
  myPc: MyPcSettings;
}
export interface MyPcSettings {
  threeD: boolean;
  /** Checked with `validShape` when read; kept as written. */
  gpuShapes: Record<string, unknown>;
  /** Desktop or laptop model (1.1); `auto`: as the firmware says. */
  form: 'auto' | 'desktop' | 'laptop';
}
export const DEFAULT_MY_PC: MyPcSettings = { threeD: true, gpuShapes: {}, form: 'auto' };
export { DEFAULT_OVERLAY };
export type { OverlayCorner, OverlayData, OverlaySettings };
let browserSettings: AppSettings = {
  closeToTray: true, temperatureUnit: 'c', hiddenSensors: [], profile: 'desktop', gameCapture: true, overlay: { ...DEFAULT_OVERLAY }, checkUpdates: true,
  games: { ...DEFAULT_GAMES }, ...savedLook(), myPc: { ...DEFAULT_MY_PC },
};

/** Overlay readings, at the overlay's update rate while it shows; in a browser, from the
 * simulator (with the game's own mode, as the app does). */
export async function onOverlay(cb: (d: OverlayData) => void): Promise<() => void> {
  if (!inTauri) {
    // Preview pages can ask for a look: ?mode=graph&compact&scale=150&corner=topRight&metrics=fps,vram#overlay.
    const q = new URLSearchParams(location.search);
    const look: Partial<OverlaySettings> = {
      ...(q.get('mode') ? { mode: q.get('mode') as OverlaySettings['mode'] } : {}),
      ...(q.has('compact') ? { compact: true } : {}),
      ...(q.get('scale') ? { scale: Number(q.get('scale')) } : {}),
      ...(q.get('corner') ? { corner: q.get('corner') as OverlayCorner } : {}),
      ...(q.get('metrics') ? { metrics: q.get('metrics')!.split(',') as OverlaySettings['metrics'] } : {}),
    };
    return onSensors((s) => {
      if (!s.game) return;
      const mode = browserEffective().overlayMode ?? browserSettings.overlay.mode;
      cb(overlayData({ ...browserSettings.overlay, mode, ...look }, s, s.game));
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
    if (settings.profile !== browserSettings.profile) browserSuppressed = true;
    browserSettings = { ...settings };
    for (const cb of settingsListeners) cb({ ...browserSettings });
    notifyBrowserGame();
    return;
  }
  await invoke('set_settings', { settings });
}

// Browser preview: the simulated game (`sim-game.ts`) always runs.
const BROWSER_GAME = 'cyberpunk2077.exe';
let browserSuppressed = false;
const gameListeners = new Set<(e: Effective) => void>();
function browserEffective(): Effective {
  const poll = (p: Profile) => BROWSER_PROFILES.find((x) => x.id === p)?.pollMs ?? 1000;
  return effectiveFor(browserSettings.profile, browserSettings.games, BROWSER_GAME, browserSuppressed, poll);
}
function notifyBrowserGame(): void {
  const e = browserEffective();
  for (const cb of gameListeners) cb(e);
}

/** The profile, fan settings and update rate in force, and whether a game chose them. */
export async function getGameState(): Promise<Effective> {
  if (!inTauri) return browserEffective();
  return invoke<Effective>('get_game_state');
}

export async function onGameState(cb: (e: Effective) => void): Promise<() => void> {
  if (!inTauri) {
    gameListeners.add(cb);
    return () => gameListeners.delete(cb);
  }
  return listen<Effective>('profile-state', (e) => cb(e.payload));
}

/** A profile picked by the owner: saved, and it wins over a running game's. */
export async function pickProfile(profile: Profile): Promise<void> {
  if (!inTauri) {
    browserSuppressed = true;
    await setSettings({ ...browserSettings, profile });
    notifyBrowserGame();
    return;
  }
  await invoke('pick_profile', { profile });
}

/** Keeps the owner's profile while the current game runs. */
export async function keepProfile(): Promise<void> {
  if (!inTauri) {
    browserSuppressed = true;
    notifyBrowserGame();
    return;
  }
  await invoke('keep_profile');
}

/** Updates (`src-tauri/src/updates.rs`). */
export interface AvailableUpdate {
  version: string;
  notes: string | null;
  /** Unix seconds. */
  date: number | null;
}
export interface UpdateStatus {
  /** This build can update itself (release builds). */
  configured: boolean;
  current: string;
  checking: boolean;
  available: AvailableUpdate | null;
  /** Unix seconds of the last completed check. */
  lastCheck: number | null;
  error: string | null;
  installing: boolean;
}
// Browser preview: a release build that finds 0.2.0 when asked.
let browserUpdate: UpdateStatus = {
  configured: true, current: '0.1.0', checking: false, available: null, lastCheck: null, error: null, installing: false,
};
const updateListeners = new Set<(s: UpdateStatus) => void>();

export async function getUpdateStatus(): Promise<UpdateStatus> {
  if (!inTauri) return { ...browserUpdate };
  return invoke<UpdateStatus>('get_update_status');
}

/** Status changes (checks, installing, errors). */
export async function onUpdateStatus(cb: (s: UpdateStatus) => void): Promise<() => void> {
  if (!inTauri) {
    updateListeners.add(cb);
    return () => updateListeners.delete(cb);
  }
  return listen<UpdateStatus>('update-status', (e) => cb(e.payload));
}

export async function checkForUpdates(): Promise<UpdateStatus> {
  if (!inTauri) {
    const set = (s: Partial<UpdateStatus>) => {
      browserUpdate = { ...browserUpdate, ...s };
      for (const cb of updateListeners) cb({ ...browserUpdate });
    };
    set({ checking: true, error: null });
    await new Promise((r) => setTimeout(r, 600));
    const now = Math.floor(Date.now() / 1000);
    set({
      checking: false,
      lastCheck: now,
      available: { version: '0.2.0', notes: 'Frame-time graph scrolls without gaps; update checks.', date: now - 86400 },
    });
    return { ...browserUpdate };
  }
  return invoke<UpdateStatus>('check_for_updates');
}

/** Downloads and runs the installer; Vyro exits and the installer starts it again. */
export async function installUpdate(): Promise<void> {
  if (!inTauri) throw new Error('not available in the browser preview');
  await invoke('install_update');
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
  if (!inTauri) {
    const { sampleHardware, sampleLaptop } = await import('./sample');
    let form: string | null = null;
    try {
      form = localStorage.getItem('vyro.sim.form');
    } catch {
      /* the desktop sample */
    }
    return form === 'laptop' ? sampleLaptop : sampleHardware;
  }
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

/** The browser preview's Connect and Disconnect: each device's state, and who to tell. */
const previewLinks = new Map<string, boolean>();
const previewChanged = new Set<() => void>();

/** Peripherals, wireless battery and sound devices (P22), read fresh. */
export async function getDevices(): Promise<Devices> {
  if (!inTauri) {
    const s = (await import('./sample-devices')).sampleDevices;
    const connected = (d: DeviceInfo) => previewLinks.get(d.id) ?? d.connected;
    return {
      ...s,
      devices: s.devices.map((d) => ({
        ...d,
        connected: connected(d),
        state: connected(d) ? ('connected' as const) : ('notConnected' as const),
      })),
    };
  }
  return invoke<Devices>('get_devices');
}

/** Connects or disconnects a paired Bluetooth device (1.3), by the id and control the list gave
 * it, and answers with the connection Windows showed once it had time to change (1.4.1). The
 * browser preview pretends, after a moment. */
export async function setDeviceConnected(id: string, control: Control, connect: boolean): Promise<LinkState> {
  if (!inTauri) {
    await new Promise((r) => setTimeout(r, 600));
    previewLinks.set(id, connect);
    previewChanged.forEach((cb) => cb());
    return connect ? 'connected' : 'notConnected';
  }
  return invoke<LinkState>('set_device_connected', { id, control, connect });
}

/** Devices whose battery needs charging soon (1.4.1), as the app last read them in the
 * background; the preview works them out from its sample. */
export async function getLowBatteries(): Promise<DeviceInfo[]> {
  if (!inTauri) {
    const { devices } = await getDevices();
    return devices.filter((d) => d.low != null && d.state !== 'notConnected');
  }
  return invoke<DeviceInfo[]>('get_low_batteries');
}

/** Fires with the low batteries whenever they change. */
export async function onLowBattery(cb: (low: DeviceInfo[]) => void): Promise<() => void> {
  if (!inTauri) {
    const again = () => void getLowBatteries().then(cb);
    previewChanged.add(again);
    return () => previewChanged.delete(again);
  }
  return listen<DeviceInfo[]>('low-battery', (e) => cb(e.payload));
}

/** Fires once things settle after a device arrives, leaves, connects or disconnects. */
export async function onDevicesChanged(cb: () => void): Promise<() => void> {
  if (!inTauri) {
    previewChanged.add(cb);
    return () => previewChanged.delete(cb);
  }
  return listen('devices-changed', () => cb());
}

/** Saved Dashboard layouts (P24); null before the first save (the Dashboard then starts from
 * the presets). The browser preview keeps them in this page's storage. */
export async function getDashboard(): Promise<Dashboard | null> {
  if (!inTauri) {
    try {
      const t = localStorage.getItem('vyro.dashboard');
      return t ? (JSON.parse(t) as Dashboard) : null;
    } catch {
      return null;
    }
  }
  return invoke<Dashboard | null>('get_dashboard');
}

/** Saves the layouts; returns them as stored (checked and tidied by the app). */
export async function setDashboard(dashboard: Dashboard): Promise<Dashboard> {
  if (!inTauri) {
    try {
      localStorage.setItem('vyro.dashboard', JSON.stringify(dashboard));
    } catch {
      /* storage unavailable: kept for this page only */
    }
    return dashboard;
  }
  return invoke<Dashboard>('set_dashboard', { dashboard });
}

/** The PC change history (P19), newest first. */
export async function getHardwareChanges(limit = 1000): Promise<HardwareChange[]> {
  if (!inTauri) return (await import('./sim-changes')).SAMPLE_CHANGES.slice(0, limit);
  return invoke<HardwareChange[]>('get_hardware_changes', { limit });
}

/** Fires with the new changes when a check finds any (at start, or when a monitor or drive comes). */
export async function onHardwareChanged(cb: (found: HardwareChange[]) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<HardwareChange[]>('hardware-changed', (e) => cb(e.payload));
}

/** Fires when the window is shown again from the tray. */
export async function onWindowOpened(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen('window-opened', () => cb());
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

// Remote companion (P26). The browser preview keeps a pretend companion in memory.

/** Settings → Remote: on or off, addresses, paired devices, the open pairing code. */
export async function getRemoteStatus(): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRemoteStatus();
  return invoke<RemoteStatus>('get_remote_status');
}

/** On, and how many live views are open (the header chip). */
export async function getRemoteBrief(): Promise<RemoteBrief> {
  if (!inTauri) return (await import('./sim-remote')).simRemoteBrief();
  return invoke<RemoteBrief>('get_remote_brief');
}

/** Fires when the companion starts or stops, a phone opens or closes its view, or a device is
 * paired or signed out. */
export async function onRemote(cb: (b: RemoteBrief) => void): Promise<() => void> {
  if (!inTauri) return (await import('./sim-remote')).onSimRemote(cb);
  return listen<RemoteBrief>('remote-status', (e) => cb(e.payload));
}

export async function setRemoteEnabled(enabled: boolean): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simSetEnabled(enabled);
  return invoke<RemoteStatus>('set_remote_enabled', { enabled });
}

export async function setRemoteOptions(port: number, sessionDays: number): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simSetOptions(port, sessionDays);
  return invoke<RemoteStatus>('set_remote_options', { port, sessionDays });
}

/** Opens a pairing code (5 minutes, one device). */
export async function startRemotePairing(): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simStartPairing();
  return invoke<RemoteStatus>('start_remote_pairing');
}

export async function cancelRemotePairing(): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simCancelPairing();
  return invoke<RemoteStatus>('cancel_remote_pairing');
}

/** Signs one device out, or every device with `null`; their live views close at once. */
export async function signOutRemote(id: string | null): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simSignOut(id);
  return invoke<RemoteStatus>('sign_out_remote', { id });
}

// Reach from anywhere (1.4): the relay connection and the phones paired through it.

export async function setRelayEnabled(enabled: boolean): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRelayEnabled(enabled);
  return invoke<RemoteStatus>('set_relay_enabled', { enabled });
}

/** The owner's own relay; `null` or empty: the one built into Vyro. Phones paired through the
 * old relay are signed out. */
export async function setRelayUrl(url: string | null): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRelayUrl(url);
  return invoke<RemoteStatus>('set_relay_url', { url });
}

/** Opens a pairing; the status carries its link and QR code once the relay can take it. */
/** `control`: the phone may change the PC (1.5); otherwise it only looks. */
export async function startRelayPairing(control: boolean): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRelayPairing();
  return invoke<RemoteStatus>('start_relay_pairing', { control });
}

/** Lets a phone paired from anywhere change the PC, or only look (1.5). */
export async function setPhoneControl(id: string, control: boolean): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simPhoneControl(id, control);
  return invoke<RemoteStatus>('set_phone_control', { id, control });
}

/** "Wake from phone" (1.5): phones are told this PC's wired address. */
export async function setWakeFromPhone(on: boolean): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simWake(on);
  return invoke<RemoteStatus>('set_wake_from_phone', { on });
}

export async function getWakeReadiness(): Promise<WakeReadiness> {
  if (!inTauri) return (await import('./sim-remote')).simWakeReadiness();
  return invoke<WakeReadiness>('get_wake_readiness');
}

/** What phones changed on this PC, newest first (1.5). */
export async function getPhoneActions(): Promise<PhoneAction[]> {
  if (!inTauri) return (await import('./sim-remote')).simPhoneActions();
  return invoke<PhoneAction[]>('get_phone_actions');
}

export async function onPhoneActions(cb: () => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen('phone-actions', () => cb());
}

/** A shutdown or restart a phone asked for, while it waits (1.5). */
export async function getPowerState(): Promise<PowerState> {
  if (!inTauri) return { pending: null, canSleep: true, canHibernate: false };
  return invoke<PowerState>('get_power_state');
}

export async function cancelPowerAction(): Promise<PowerState> {
  if (!inTauri) return { pending: null, canSleep: true, canHibernate: false };
  return invoke<PowerState>('cancel_power_action');
}

export async function onPowerPending(cb: (s: PowerState) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<PowerState>('power-pending', (e) => cb(e.payload));
}

/** Ends a process (1.5); Windows' own and Vyro's are refused. */
export async function endProcess(pid: number, name: string): Promise<void> {
  if (!inTauri) {
    await new Promise((r) => setTimeout(r, 300));
    return;
  }
  return invoke('end_process', { pid, name });
}

export async function cancelRelayPairing(): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRelayCancel();
  return invoke<RemoteStatus>('cancel_relay_pairing');
}

/** New keys for this PC on the relay; every phone paired from anywhere is signed out. */
export async function resetRelay(): Promise<RemoteStatus> {
  if (!inTauri) return (await import('./sim-remote')).simRelayReset();
  return invoke<RemoteStatus>('reset_relay');
}

// ---------------------------------------------------------------------------------------------
// Desktop widgets (P30)
// ---------------------------------------------------------------------------------------------

/** The browser preview keeps widgets in this page's storage (they cannot open windows here). */
function browserWidgets(): Widgets {
  try {
    const t = localStorage.getItem('vyro.widgets');
    return t ? (JSON.parse(t) as Widgets) : { ...NO_WIDGETS };
  } catch {
    return { ...NO_WIDGETS };
  }
}
const widgetListeners = new Set<(w: Widgets) => void>();

export async function getWidgets(): Promise<Widgets> {
  if (!inTauri) return browserWidgets();
  return invoke<Widgets>('get_widgets');
}

/** Saves the widgets; the app opens, moves or closes their windows and returns them as stored. */
export async function setWidgets(widgets: Widgets): Promise<Widgets> {
  if (!inTauri) {
    try {
      localStorage.setItem('vyro.widgets', JSON.stringify(widgets));
    } catch {
      /* not kept */
    }
    for (const cb of widgetListeners) cb(widgets);
    return widgets;
  }
  return invoke<Widgets>('set_widgets', { widgets });
}

/** A widget's own menu: lock, unlock, remove, or edit it on the Widgets page. */
export async function widgetAction(id: string, action: 'lock' | 'unlock' | 'remove' | 'edit'): Promise<void> {
  if (!inTauri) {
    const w = browserWidgets();
    if (action === 'remove') w.widgets = w.widgets.filter((x) => x.id !== id);
    else if (action !== 'edit') w.widgets = w.widgets.map((x) => (x.id === id ? { ...x, locked: action === 'lock' } : x));
    await setWidgets(w);
    return;
  }
  await invoke('widget_action', { id, action });
}

/** The widgets after any change (the Widgets page, a widget's menu, the tray). */
export async function onWidgets(cb: (w: Widgets) => void): Promise<() => void> {
  if (!inTauri) {
    widgetListeners.add(cb);
    return () => widgetListeners.delete(cb);
  }
  return listen<Widgets>('widgets-changed', (e) => cb(e.payload));
}

/** Snapshots for a widget window (sent also while the main window is in the tray). */
export async function onWidgetSensors(cb: (s: SensorSnapshot) => void): Promise<() => void> {
  if (!inTauri) return onSensors(cb);
  return listen<SensorSnapshot>('widget-sensors', (e) => cb(e.payload));
}

/** Moves the widget's window with the pointer (pressed on an unlocked widget). */
export async function dragWidget(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  await getCurrentWindow().startDragging();
}

/** The app asks the window to show a view (a widget's "Edit…"). */
export async function onOpenView(cb: (view: string) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  return listen<string>('open-view', (e) => cb(e.payload));
}
