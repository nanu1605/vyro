<script lang="ts">
  import { onMount } from 'svelte';
  import AppStatus from './components/AppStatus.svelte';
  import HardwarePanel from './components/HardwarePanel.svelte';
  import SystemPanel from './components/SystemPanel.svelte';
  import SettingsPanel from './components/SettingsPanel.svelte';
  import DisplaysPanel from './components/DisplaysPanel.svelte';
  import DashboardGrid from './components/DashboardGrid.svelte';
  import StoragePanel from './components/StoragePanel.svelte';
  import NetworkPanel from './components/NetworkPanel.svelte';
  import PawnIoBanner from './components/PawnIoBanner.svelte';
  import ServiceBanner from './components/ServiceBanner.svelte';
  import ProcessesPanel from './components/ProcessesPanel.svelte';
  import SensorsPanel from './components/SensorsPanel.svelte';
  import HistoryPanel from './components/HistoryPanel.svelte';
  import HealthPanel from './components/HealthPanel.svelte';
  import InsightsPanel from './components/InsightsPanel.svelte';
  import GamingPanel from './components/GamingPanel.svelte';
  import CoolingPanel from './components/CoolingPanel.svelte';
  import { animator } from './anim';
  import { loadUpdates, update } from './updates.svelte';
  import { getBackendStatus, getHardwareChanges, getPawnIoStatus, inTauri, onBackendStatus, onHardwareChanged, onSensors, onWindowOpened, restoreFans, setUiVisible } from './ipc';
  import { loadSeen, newestId, saveSeen } from './changes';
  import { loadSeenKeys, markSeen, unseen } from './insights';
  import { loadPrefs, prefs, profileSpec, tempText } from './prefs.svelte';
  import { applyLook, savedLook, uiState } from './themes';
  import type { AlertFired, BackendStatus, PawnIoStatus, SensorSnapshot } from './types';

  const TABS = [
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'cooling', label: 'Cooling' },
    { id: 'history', label: 'History' },
    { id: 'health', label: 'Health' },
    { id: 'insights', label: 'Insights' },
    { id: 'gaming', label: 'Gaming' },
    { id: 'benchmark', label: 'Benchmark' },
    { id: 'processes', label: 'Processes' },
    { id: 'hardware', label: 'Hardware' },
    { id: 'storage', label: 'Storage' },
    { id: 'network', label: 'Network' },
    { id: 'displays', label: 'Displays' },
    { id: 'devices', label: 'Devices' },
    { id: 'system', label: 'System' },
    { id: 'sensors', label: 'Sensors' },
    { id: 'settings', label: 'Settings' },
  ] as const;
  type Tab = (typeof TABS)[number]['id'];

  function lastTab(): Tab {
    try {
      const t = localStorage.getItem('vyro.tab');
      return TABS.some((x) => x.id === t) ? (t as Tab) : 'dashboard';
    } catch {
      return 'dashboard';
    }
  }

  // "Open on" (P25): the Dashboard unless the owner chose the last tab. The page's copy of the
  // choice decides the first paint; the settings file corrects it once loaded.
  let tab = $state<Tab>(savedLook().openOn === 'last' ? lastTab() : 'dashboard');
  /** Set by the first tab the owner picks, so loading the settings never moves them. */
  let navigated = false;
  // Panels mount on first visit and then stay mounted (keeps fetched data and live history).
  let visited = $state<Record<Tab, boolean>>({
    dashboard: true,
    cooling: false,
    history: false,
    health: false,
    insights: false,
    gaming: false,
    benchmark: false,
    processes: false,
    hardware: false,
    storage: false,
    network: false,
    displays: false,
    devices: false,
    system: false,
    sensors: false,
    settings: false,
  });

  /** Alerts that fired while the window is open, shown briefly in the corner. */
  let toasts = $state<Array<AlertFired & { key: number }>>([]);
  let toastSeq = 0;
  function showToast(a: AlertFired): void {
    // Same policy as Windows notifications: the profile decides what interrupts.
    const policy = profileSpec().notifications;
    if (policy === 'off' || (policy === 'critical' && a.ruleId !== 'health-problems')) return;
    const key = ++toastSeq;
    toasts = [...toasts.slice(-2), { ...a, key }];
    setTimeout(() => (toasts = toasts.filter((t) => t.key !== key)), 10_000);
  }

  // PC change history (P19): a dot on Hardware until its new changes have been shown.
  let newestChange = $state(0);
  let changesSeen = $state(loadSeen());
  let changesNewSince = $state(Number.POSITIVE_INFINITY);
  $effect(() => {
    if (tab === 'hardware' && newestChange > changesSeen) {
      changesNewSince = changesSeen;
      changesSeen = newestChange;
      saveSeen(newestChange);
    }
  });

  /** Sensor opened from the explorer or Insights, charted alone at the top of History, with
   * the moment it was opened for. */
  let historyFocus = $state<string | null>(null);
  let historyAt = $state<number | null>(null);
  function openHistory(id: string, at: number | null = null): void {
    historyFocus = id;
    historyAt = at;
    select('history');
  }

  function select(t: Tab): void {
    navigated = true;
    tab = t;
    visited[t] = true;
    try {
      localStorage.setItem('vyro.tab', t);
    } catch {
      /* storage unavailable: tab just isn't remembered */
    }
  }

  /** Tab list keys (WAI-ARIA tabs pattern): arrows move and wrap, Home/End jump to the ends. */
  function onTabKey(e: KeyboardEvent): void {
    const i = TABS.findIndex((x) => x.id === tab);
    const at: Record<string, number> = {
      ArrowRight: (i + 1) % TABS.length,
      ArrowLeft: (i + TABS.length - 1) % TABS.length,
      Home: 0,
      End: TABS.length - 1,
    };
    const to = at[e.key];
    if (to == null) return;
    e.preventDefault();
    const next = TABS[to];
    if (next) {
      select(next.id);
      document.getElementById(`tab-${next.id}`)?.focus();
    }
  }

  // Replaced wholesale every tick; no deep reactivity needed.
  let snap = $state.raw<SensorSnapshot | null>(null);

  // Insights (P16): a dot on the tab until the findings worth a look have been shown.
  let insightsSeen = $state(loadSeenKeys());
  let insightsFresh = $state<string[]>([]);
  const insightsNew = $derived(unseen(snap?.insights?.attention ?? [], insightsSeen));
  $effect(() => {
    if (tab === 'insights' && insightsNew.length) {
      insightsFresh = insightsNew;
      insightsSeen = markSeen(insightsSeen, insightsNew);
    }
  });
  const overall = $derived(snap?.health.overall ?? 'unknown');
  /** Warnings and problems for the dashboard strip, worst first. */
  const attention = $derived(
    (snap?.health.subsystems ?? [])
      .filter((x) => x.status === 'warn' || x.status === 'fail')
      .sort((a, b) => (a.status === b.status ? 0 : a.status === 'fail' ? -1 : 1))
      .flatMap((x) => (x.messages.length ? x.messages.map(tempText) : [x.name])),
  );

  let uiFps = $state(0);

  // Theme and accent (P25), from the settings; the page state colours the header rail.
  $effect(() => applyLook({ theme: prefs.theme, accent: prefs.accent, openOn: prefs.openOn }));
  const pageState = $derived(uiState(snap));
  $effect(() => {
    document.documentElement.dataset.state = pageState;
  });

  // Profile: animation level and dashboard density.
  const spec = $derived(profileSpec());
  const compact = $derived(spec.dashboard === 'compact');
  $effect(() => {
    animator.setMotion(spec.motion);
    document.documentElement.dataset.motion = spec.motion;
  });

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

  // Fan control (P12): the header shows whenever Vyro drives fans, from any tab. Each snapshot
  // says so; nothing is polled.
  const fans = $derived(snap?.fanControl ?? null);
  async function fansToBios(): Promise<void> {
    await restoreFans().catch(() => {});
  }

  onMount(() => {
    visited[tab] = true;
    loadPawnIo();
    void loadPrefs()
      .then(() => {
        if (!navigated && prefs.openOn === 'last' && tab === 'dashboard') {
          const t = lastTab();
          if (t !== 'dashboard') select(t);
        }
      })
      .catch(() => {});
    void loadUpdates().catch(() => {});
    let stop: (() => void) | undefined;
    let stopBackend: (() => void) | undefined;
    let stopChanges: (() => void) | undefined;
    let stopOpened: (() => void) | undefined;
    let disposed = false;
    // Opened again from the tray: back to the Dashboard unless "Last tab" is chosen.
    void onWindowOpened(() => {
      if (prefs.openOn === 'dashboard') select('dashboard');
    }).then((fn) => {
      if (disposed) fn();
      else stopOpened = fn;
    });
    getHardwareChanges(1)
      .then((v) => (newestChange = Math.max(newestChange, newestId(v))))
      .catch(() => {});
    void onHardwareChanged((found) => (newestChange = Math.max(newestChange, newestId(found)))).then((fn) => {
      if (disposed) fn();
      else stopChanges = fn;
    });
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
      stopChanges?.();
      stopOpened?.();
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
            <span class="alert-dot {overall}" aria-hidden="true"></span>
            <span class="sr-only">({overall === 'fail' ? 'problem' : 'warning'})</span>
          {/if}
          {#if t.id === 'hardware' && newestChange > changesSeen}
            <span class="alert-dot new" aria-hidden="true"></span>
            <span class="sr-only">(new PC changes)</span>
          {/if}
          {#if t.id === 'insights' && insightsNew.length}
            <span class="alert-dot new" aria-hidden="true"></span>
            <span class="sr-only">(new findings)</span>
          {/if}
        </button>
      {/each}
    </div>
    {#if fans?.active}
      <div class="fan-chip" role="status">
        <button type="button" class="chip-open" onclick={() => select('cooling')}>
          <span class="fan-dot" class:hot={fans.fullSpeed} aria-hidden="true"></span>
          {fans.fullSpeed ? 'Fans at full speed' : 'Fan control active'}
        </button>
        <button type="button" class="chip-bios" onclick={fansToBios} title="Hand every fan back to the BIOS">
          Return to BIOS
        </button>
      </div>
    {/if}
  </header>
  <div class="rail" aria-hidden="true"></div>

  <main>
    {#if visited.cooling}
      <div id="panel-cooling" role="tabpanel" aria-labelledby="tab-cooling" hidden={tab !== 'cooling'}>
        <CoolingPanel {snap} {backend} active={tab === 'cooling'} />
      </div>
    {/if}
    <div id="panel-dashboard" role="tabpanel" aria-labelledby="tab-dashboard" hidden={tab !== 'dashboard'}>
      <ServiceBanner status={backend} />
      {#if !snap}
        <p class="waiting" role="status">
          {backend && !backend.connected ? 'Connecting to the Vyro Sensor Service…' : 'Waiting for the first reading…'}
        </p>
      {/if}
      <PawnIoBanner status={pawnioShown} />
      {#if update.status?.available && !update.status.installing}
        <button type="button" class="health-strip update" onclick={() => select('settings')}>
          <span class="msg">Vyro {update.status.available.version} is available</span>
          <span class="go">Settings →</span>
        </button>
      {/if}
      {#if attention.length}
        <button type="button" class="health-strip {overall}" onclick={() => select('health')}>
          <span class="alert-dot {overall}" aria-hidden="true"></span>
          <span class="msg">{attention[0]}{attention.length > 1 ? ` (+${attention.length - 1} more)` : ''}</span>
          <span class="go">Health →</span>
        </button>
      {/if}
      <DashboardGrid
        {snap}
        active={tab === 'dashboard'}
        needs={NEEDS_DRIVER}
        {uiFps}
        {compact}
        profileName={spec.name}
        onselect={select}
      />
    </div>

    {#if visited.history}
      <div id="panel-history" role="tabpanel" aria-labelledby="tab-history" hidden={tab !== 'history'}>
        <HistoryPanel
          {snap}
          active={tab === 'history'}
          focus={historyFocus}
          focusAt={historyAt}
          onclearfocus={() => ((historyFocus = null), (historyAt = null))}
        />
      </div>
    {/if}

    {#if visited.health}
      <div id="panel-health" role="tabpanel" aria-labelledby="tab-health" hidden={tab !== 'health'}>
        <HealthPanel {snap} active={tab === 'health'} oninsights={() => select('insights')} onhistory={openHistory} />
      </div>
    {/if}

    {#if visited.insights}
      <div id="panel-insights" role="tabpanel" aria-labelledby="tab-insights" hidden={tab !== 'insights'}>
        <InsightsPanel active={tab === 'insights'} fresh={insightsFresh} onhistory={openHistory} />
      </div>
    {/if}

    {#if visited.gaming}
      <div id="panel-gaming" role="tabpanel" aria-labelledby="tab-gaming" hidden={tab !== 'gaming'}>
        <GamingPanel {snap} {backend} active={tab === 'gaming'} onsettings={() => select('settings')} />
      </div>
    {/if}

    {#if visited.benchmark}
      <div id="panel-benchmark" role="tabpanel" aria-labelledby="tab-benchmark" hidden={tab !== 'benchmark'}>
        <!-- Loaded on first visit: most sessions never open it. -->
        {#await import('./components/BenchmarkPanel.svelte') then { default: BenchmarkPanel }}
          <BenchmarkPanel active={tab === 'benchmark'} />
        {/await}
      </div>
    {/if}

    {#if visited.processes}
      <div id="panel-processes" role="tabpanel" aria-labelledby="tab-processes" hidden={tab !== 'processes'}>
        <ProcessesPanel processes={snap?.processes ?? []} />
      </div>
    {/if}

    {#if visited.hardware}
      <div id="panel-hardware" role="tabpanel" aria-labelledby="tab-hardware" hidden={tab !== 'hardware'}>
        <HardwarePanel newSince={changesNewSince} />
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
    {#if visited.devices}
      <div id="panel-devices" role="tabpanel" aria-labelledby="tab-devices" hidden={tab !== 'devices'}>
        <!-- Loaded on first visit, like Benchmark. -->
        {#await import('./components/DevicesPanel.svelte') then { default: DevicesPanel }}
          <DevicesPanel active={tab === 'devices'} />
        {/await}
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
    margin-bottom: 0.55rem;
  }
  /* The page state (idle, gaming, warning, critical) as a thin line under the header. */
  .rail {
    height: 2px;
    margin-bottom: 0.85rem;
    border-radius: 2px;
    background: linear-gradient(90deg, transparent, var(--state) 20%, var(--state) 80%, transparent);
    opacity: 0.35;
  }
  :global(:root:not([data-state='idle'])) .rail {
    opacity: 1;
  }
  /* Critical pulses four times, then stays lit; not at all with the profile's motion off. */
  :global(:root[data-state='critical']) .rail {
    animation: rail-pulse 1.2s ease-in-out 8 alternate;
  }
  :global(:root[data-motion='off']) .rail {
    animation: none;
  }
  @keyframes rail-pulse {
    to {
      opacity: 0.3;
    }
  }
  .fan-chip {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin-left: auto;
    padding: 0.15rem;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--line));
    border-radius: 999px;
    font-size: 0.78rem;
  }
  .fan-chip button {
    padding: 0.2rem 0.6rem;
    border: 0;
    border-radius: 999px;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .fan-chip .chip-bios {
    background: color-mix(in srgb, var(--accent) 14%, var(--bg));
  }
  .fan-dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.35rem;
    border-radius: 50%;
    background: var(--accent);
  }
  .fan-dot.hot {
    background: var(--hot);
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
    /* Holds the tab's screen-reader note, so it scrolls with the tab instead of widening the page. */
    position: relative;
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
  .alert-dot.new {
    background: var(--accent);
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
    box-shadow: var(--shadow);
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
  .health-strip.update {
    border-color: color-mix(in srgb, var(--accent-gpu) 45%, var(--line));
    background: color-mix(in srgb, var(--accent-gpu) 8%, var(--panel));
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
  .waiting {
    margin: 0 0 0.9rem;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
