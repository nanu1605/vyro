<script lang="ts">
  import { onMount } from 'svelte';
  import GaugePanel from './components/GaugePanel.svelte';
  import StatTile from './components/StatTile.svelte';
  import AppStatus from './components/AppStatus.svelte';
  import HardwarePanel from './components/HardwarePanel.svelte';
  import SystemPanel from './components/SystemPanel.svelte';
  import SettingsPanel from './components/SettingsPanel.svelte';
  import DisplaysPanel from './components/DisplaysPanel.svelte';
  import StoragePanel from './components/StoragePanel.svelte';
  import MemoryCard from './components/MemoryCard.svelte';
  import NetworkPanel from './components/NetworkPanel.svelte';
  import PowerCard from './components/PowerCard.svelte';
  import SelfCard from './components/SelfCard.svelte';
  import CoresCard from './components/CoresCard.svelte';
  import CpuSensorsCard from './components/CpuSensorsCard.svelte';
  import BoardCard from './components/BoardCard.svelte';
  import PawnIoBanner from './components/PawnIoBanner.svelte';
  import ServiceBanner from './components/ServiceBanner.svelte';
  import GpuCard from './components/GpuCard.svelte';
  import ProcessesPanel from './components/ProcessesPanel.svelte';
  import SensorsPanel from './components/SensorsPanel.svelte';
  import HistoryPanel from './components/HistoryPanel.svelte';
  import HealthPanel from './components/HealthPanel.svelte';
  import { animator } from './anim';
  import { fmt, fmtGB } from './format';
  import { getBackendStatus, getPawnIoStatus, inTauri, onBackendStatus, onSensors, setUiVisible } from './ipc';
  import { loadPrefs, temp, tempOrNull, tempText, tempUnit } from './prefs.svelte';
  import type { AlertFired, BackendStatus, PawnIoStatus, SensorSnapshot } from './types';

  const TABS = [
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'history', label: 'History' },
    { id: 'health', label: 'Health' },
    { id: 'processes', label: 'Processes' },
    { id: 'hardware', label: 'Hardware' },
    { id: 'storage', label: 'Storage' },
    { id: 'network', label: 'Network' },
    { id: 'displays', label: 'Displays' },
    { id: 'system', label: 'System' },
    { id: 'sensors', label: 'Sensors' },
    { id: 'settings', label: 'Settings' },
  ] as const;
  type Tab = (typeof TABS)[number]['id'];

  function savedTab(): Tab {
    try {
      const t = localStorage.getItem('vyro.tab');
      return TABS.some((x) => x.id === t) ? (t as Tab) : 'dashboard';
    } catch {
      return 'dashboard';
    }
  }

  let tab = $state<Tab>(savedTab());
  // Panels mount on first visit and then stay mounted (keeps fetched data and live history).
  let visited = $state<Record<Tab, boolean>>({
    dashboard: true,
    history: false,
    health: false,
    processes: false,
    hardware: false,
    storage: false,
    network: false,
    displays: false,
    system: false,
    sensors: false,
    settings: false,
  });

  /** Alerts that fired while the window is open, shown briefly in the corner. */
  let toasts = $state<Array<AlertFired & { key: number }>>([]);
  let toastSeq = 0;
  function showToast(a: AlertFired): void {
    const key = ++toastSeq;
    toasts = [...toasts.slice(-2), { ...a, key }];
    setTimeout(() => (toasts = toasts.filter((t) => t.key !== key)), 10_000);
  }

  /** Sensor opened from the explorer, charted alone at the top of History. */
  let historyFocus = $state<string | null>(null);
  function openHistory(id: string): void {
    historyFocus = id;
    select('history');
  }

  function select(t: Tab): void {
    tab = t;
    visited[t] = true;
    try {
      localStorage.setItem('vyro.tab', t);
    } catch {
      /* storage unavailable: tab just isn't remembered */
    }
  }

  function onTabKey(e: KeyboardEvent): void {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const i = TABS.findIndex((x) => x.id === tab);
    const next = TABS[(i + (e.key === 'ArrowRight' ? 1 : TABS.length - 1)) % TABS.length];
    if (next) {
      select(next.id);
      document.getElementById(`tab-${next.id}`)?.focus();
    }
  }

  // Replaced wholesale every tick; no deep reactivity needed.
  let snap = $state.raw<SensorSnapshot | null>(null);
  const overall = $derived(snap?.health.overall ?? 'unknown');
  /** Warnings and problems for the dashboard strip, worst first. */
  const attention = $derived(
    (snap?.health.subsystems ?? [])
      .filter((x) => x.status === 'warn' || x.status === 'fail')
      .sort((a, b) => (a.status === b.status ? 0 : a.status === 'fail' ? -1 : 1))
      .flatMap((x) => (x.messages.length ? x.messages.map(tempText) : [x.name])),
  );

  let uiFps = $state(0);

  const cpu = $derived(snap?.cpu ?? null);
  const gpu = $derived(snap?.gpu[0] ?? null);
  const mem = $derived(snap?.memory ?? null);
  // CPU temperature / power and board fans come through PawnIO (Phase 6).
  let pawnio = $state.raw<PawnIoStatus | null>(null);
  // Sensor service connection (P8); without it the app reads unprivileged sensors itself.
  let backend = $state.raw<BackendStatus | null>(null);
  // Without the service, PawnIO's "no access" is explained by the service banner.
  const pawnioShown = $derived(backend?.mode === 'local' && pawnio?.state === 'noAccess' ? null : pawnio);
  const loadPawnIo = () =>
    getPawnIoStatus()
      .then((s) => (pawnio = s))
      .catch(() => {});
  const NEEDS_DRIVER = $derived(pawnio?.state === 'ready' ? 'not reported' : 'needs PawnIO');
  const cpuFan = $derived(snap?.fans.find((f) => f.kind === 'fan') ?? null);

  onMount(() => {
    visited[tab] = true;
    loadPawnIo();
    void loadPrefs().catch(() => {});
    let stop: (() => void) | undefined;
    let stopBackend: (() => void) | undefined;
    let disposed = false;
    getBackendStatus()
      .then((s) => (backend = s))
      .catch(() => {});
    void onBackendStatus((s) => {
      backend = s;
      // A restarted service may see PawnIO differently.
      if (s.connected) loadPawnIo();
    }).then((fn) => {
      if (disposed) fn();
      else stopBackend = fn;
    });
    void onSensors((s) => {
      snap = s;
      uiFps = animator.fps;
      for (const a of s.alerts) showToast(a);
    }).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });

    const onVisibility = () => {
      animator.setHidden(document.hidden);
      setUiVisible(!document.hidden);
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      disposed = true;
      stop?.();
      stopBackend?.();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });
</script>

<div class="app">
  <header>
    <h1>VYRO</h1>
    <span class="sub">hardware monitor</span>
    <div class="tabs" role="tablist" aria-label="Views" tabindex="-1" onkeydown={onTabKey}>
      {#each TABS as t (t.id)}
        <button
          id="tab-{t.id}"
          role="tab"
          type="button"
          aria-selected={tab === t.id}
          aria-controls="panel-{t.id}"
          tabindex={tab === t.id ? 0 : -1}
          onclick={() => select(t.id)}
        >
          {t.label}
          {#if t.id === 'health' && (overall === 'warn' || overall === 'fail')}
            <span class="alert-dot {overall}" aria-label={overall === 'fail' ? 'problem' : 'warning'}></span>
          {/if}
        </button>
      {/each}
    </div>
  </header>

  <main>
    <div id="panel-dashboard" role="tabpanel" aria-labelledby="tab-dashboard" hidden={tab !== 'dashboard'}>
      <ServiceBanner status={backend} />
      <PawnIoBanner status={pawnioShown} />
      {#if attention.length}
        <button type="button" class="health-strip {overall}" onclick={() => select('health')}>
          <span class="alert-dot {overall}" aria-hidden="true"></span>
          <span class="msg">{attention[0]}{attention.length > 1 ? ` (+${attention.length - 1} more)` : ''}</span>
          <span class="go">Health →</span>
        </button>
      {/if}
      <div class="gauges">
        <GaugePanel
          label="CPU"
          ts={snap?.ts ?? 0}
          usage={cpu?.usageTotal ?? null}
          mhz={cpu?.mhzCurrent ?? null}
          detail={cpu?.mhzBoost != null ? `base ${fmt(cpu.mhzBase)} · boost ${fmt(cpu.mhzBoost)} MHz` : null}
        />
        <div class="gpu">
          <GaugePanel
            label="GPU"
            ts={snap?.ts ?? 0}
            usage={gpu?.usage ?? null}
            mhz={gpu?.mhzCore ?? null}
            detail={gpu?.name ?? 'No GPU detected'}
          />
        </div>
      </div>

      <div class="tiles">
        <StatTile label="CPU package" value={tempOrNull(cpu?.tempPackage)} unit={tempUnit()} warn={temp(80)} hot={temp(95)} sub={cpu?.tempPackage == null ? NEEDS_DRIVER : null} />
        <StatTile label="CPU power" value={cpu?.powerPkgW} unit="W" digits={1} sub={cpu?.powerPkgW == null ? NEEDS_DRIVER : null} />
        <StatTile label="GPU temp" value={tempOrNull(gpu?.tempEdge)} unit={tempUnit()} warn={temp(80)} hot={temp(90)} />
        <StatTile label="GPU hotspot" value={tempOrNull(gpu?.tempHotspot)} unit={tempUnit()} warn={temp(90)} hot={temp(105)} />
        <StatTile
          label="VRAM"
          value={gpu?.vramUsedMB != null ? gpu.vramUsedMB / 1024 : null}
          unit="GB"
          digits={1}
          sub={gpu?.vramTotalMB != null ? `of ${fmtGB(gpu.vramTotalMB)} GB` : null}
        />
        <StatTile
          label="Memory"
          value={mem?.usedMB != null ? mem.usedMB / 1024 : null}
          unit="GB"
          digits={1}
          sub={mem?.totalMB != null ? `of ${fmtGB(mem.totalMB)} GB` : null}
        />
        <StatTile label="System power" value={snap?.power.estTotalW} unit="W" sub={snap?.power.estTotalW == null ? NEEDS_DRIVER : 'estimated'} />
        <StatTile label={cpuFan?.name ?? 'CPU fan'} value={cpuFan?.rpm} unit="RPM" sub={cpuFan?.rpm == null ? NEEDS_DRIVER : null} />
      </div>

      <div class="cards">
        <CoresCard {cpu} />
        <CpuSensorsCard {cpu} />
        {#each snap?.gpu ?? [] as g, i (i)}
          <GpuCard gpu={g} />
        {/each}
        <MemoryCard {mem} />
        <PowerCard power={snap?.power ?? null} gpus={snap?.gpu ?? []} />
        <BoardCard board={snap?.board ?? null} fans={snap?.fans ?? []} voltages={snap?.voltages ?? []} pending={NEEDS_DRIVER} />
        <SelfCard app={snap?.app ?? null} {uiFps} />
      </div>
    </div>

    {#if visited.history}
      <div id="panel-history" role="tabpanel" aria-labelledby="tab-history" hidden={tab !== 'history'}>
        <HistoryPanel {snap} active={tab === 'history'} focus={historyFocus} onclearfocus={() => (historyFocus = null)} />
      </div>
    {/if}

    {#if visited.health}
      <div id="panel-health" role="tabpanel" aria-labelledby="tab-health" hidden={tab !== 'health'}>
        <HealthPanel {snap} active={tab === 'health'} />
      </div>
    {/if}

    {#if visited.processes}
      <div id="panel-processes" role="tabpanel" aria-labelledby="tab-processes" hidden={tab !== 'processes'}>
        <ProcessesPanel processes={snap?.processes ?? []} />
      </div>
    {/if}

    {#if visited.hardware}
      <div id="panel-hardware" role="tabpanel" aria-labelledby="tab-hardware" hidden={tab !== 'hardware'}>
        <HardwarePanel />
      </div>
    {/if}
    {#if visited.storage}
      <div id="panel-storage" role="tabpanel" aria-labelledby="tab-storage" hidden={tab !== 'storage'}>
        <StoragePanel live={snap?.storage ?? []} />
      </div>
    {/if}
    {#if visited.network}
      <div id="panel-network" role="tabpanel" aria-labelledby="tab-network" hidden={tab !== 'network'}>
        <NetworkPanel ts={snap?.ts ?? 0} live={snap?.network ?? []} ping={snap?.ping ?? null} />
      </div>
    {/if}
    {#if visited.displays}
      <div id="panel-displays" role="tabpanel" aria-labelledby="tab-displays" hidden={tab !== 'displays'}>
        <DisplaysPanel />
      </div>
    {/if}
    {#if visited.system}
      <div id="panel-system" role="tabpanel" aria-labelledby="tab-system" hidden={tab !== 'system'}>
        <SystemPanel />
      </div>
    {/if}
    {#if visited.sensors}
      <div id="panel-sensors" role="tabpanel" aria-labelledby="tab-sensors" hidden={tab !== 'sensors'}>
        <SensorsPanel {snap} active={tab === 'sensors'} onhistory={openHistory} />
      </div>
    {/if}
    {#if visited.settings}
      <div id="panel-settings" role="tabpanel" aria-labelledby="tab-settings" hidden={tab !== 'settings'}>
        <SettingsPanel {snap} {backend} active={tab === 'settings'} />
      </div>
    {/if}
  </main>

  <AppStatus app={snap?.app ?? null} {uiFps} source={inTauri ? 'engine' : 'browser sim'} />

  <div class="toasts" role="status" aria-live="polite">
    {#each toasts as t (t.key)}
      <button type="button" class="toast" onclick={() => select('health')}>
        <b>{tempText(t.title)}</b>
        <span>{tempText(t.message)}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .app {
    max-width: 72rem;
    margin: 0 auto;
    padding: 1rem 1rem 0.8rem;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin-bottom: 0.9rem;
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
    max-width: 100%;
    margin-left: auto;
    overflow-x: auto;
    padding: 0.2rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .tabs button {
    padding: 0.35rem 0.85rem;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.82rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button[aria-selected='true'] {
    background: var(--panel-edge);
    color: var(--text);
  }
  .alert-dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-left: 0.3rem;
    border-radius: 50%;
    background: var(--warn);
    vertical-align: 0.1rem;
  }
  .alert-dot.fail {
    background: var(--hot);
  }
  .toasts {
    position: fixed;
    right: 1rem;
    bottom: 1rem;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    width: min(22rem, calc(100vw - 2rem));
  }
  .toast {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.6rem 0.8rem;
    border: 1px solid color-mix(in srgb, var(--hot) 50%, var(--line));
    border-radius: 10px;
    background: var(--panel);
    box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
  }
  .toast span {
    color: var(--muted);
  }
  .health-strip {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    margin: 0 0 0.9rem;
    padding: 0.55rem 0.85rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 10px;
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
  }
  .health-strip.fail {
    border-color: color-mix(in srgb, var(--hot) 55%, var(--line));
    background: color-mix(in srgb, var(--hot) 10%, var(--panel));
  }
  .health-strip .alert-dot {
    flex: none;
    margin: 0;
    vertical-align: 0;
  }
  .health-strip .msg {
    flex: 1;
    min-width: 0;
  }
  .health-strip .go {
    flex: none;
    color: var(--muted);
    font-size: 0.8rem;
  }
  [hidden] {
    display: none !important;
  }
  h1 {
    margin: 0;
    font-size: 1.1rem;
    letter-spacing: 0.35em;
  }
  .sub {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .gauges {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
  }
  .gpu {
    --accent: var(--accent-gpu);
    display: contents;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10.5rem, 1fr));
    gap: 0.75rem;
    margin-top: 1rem;
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
    gap: 0.75rem;
    margin-top: 1rem;
  }
  @media (max-width: 720px) {
    .gauges {
      grid-template-columns: 1fr;
    }
  }
</style>
