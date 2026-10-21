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
  import { cancelPowerAction, getBackendStatus, getScreenStatus, onScreenShare, stopScreenShare, getHardwareChanges, getLowBatteries, getPawnIoStatus, getPowerState, getRemoteBrief, inTauri, onBackendStatus, onHardwareChanged, onLowBattery, onOpenView, onPowerPending, onRemote, onSensors, onWindowOpened, restoreFans, restoreGpu, setUiVisible, getWhatsNew, whatsNewSeen } from './ipc';
  import { lowNotice } from './devices';
  import { loadSeen, newestId, saveSeen } from './changes';
  import { loadSeenKeys, markSeen, unseen } from './insights';
  import { loadPrefs, prefs, profileSpec, tempText } from './prefs.svelte';
  import { applyLook, savedLook, uiState } from './themes';
  import { isTab, VIEWS, type Tab } from './nav';
  import Sidebar, { type Dot } from './components/shell/Sidebar.svelte';
  import TopBar from './components/shell/TopBar.svelte';
  import LiveRail from './components/shell/LiveRail.svelte';
  import type { AlertFired, BackendStatus, DeviceInfo, PawnIoStatus, PendingPower, RemoteBrief, ScreenShare, SensorSnapshot } from './types';

  function lastTab(): Tab {
    try {
      const t = localStorage.getItem('vyro.tab');
      return isTab(t) ? t : 'dashboard';
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
  let visited = $state(Object.fromEntries(VIEWS.map((v) => [v.id, v.id === 'dashboard'])) as Record<Tab, boolean>);

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
  /** The window is hidden (tray): views that draw continuously stop and free their resources. */
  let pageHidden = $state(typeof document !== 'undefined' && document.hidden);

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
  // GPU tuning (1.6): the header shows while the card runs tuned or a guard holds it at stock.
  const gpuTune = $derived(snap?.gpuTune ?? null);
  async function gpuToStock(): Promise<void> {
    await restoreGpu().catch(() => {});
  }

  // Low batteries (1.4.1): a strip on the Dashboard and a dot on Devices while any is low.
  let lowBatteries = $state.raw<DeviceInfo[]>([]);
  const low = $derived(lowNotice(lowBatteries));

  // A shutdown or restart a phone asked for (1.5): shown on every view until it happens, with
  // Cancel.
  let pendingPower = $state.raw<PendingPower | null>(null);
  let powerError = $state<string | null>(null);
  async function cancelPower(): Promise<void> {
    try {
      pendingPower = (await cancelPowerAction()).pending;
      powerError = null;
    } catch (e) {
      powerError = String(e).replace(/^Error: /, '');
    }
  }
  const POWER_WORD: Record<string, string> = { shutDown: 'Shutting down', restart: 'Restarting' };

  // The screen shared with a phone (1.7): shown on every view while it lasts, with Stop.
  let screenShare = $state.raw<ScreenShare | null>(null);
  /** The phone's microphone plays into the cable (1.7). */
  let screenMic = $state(false);
  async function stopScreen(): Promise<void> {
    try {
      screenShare = (await stopScreenShare()).share;
    } catch {
      // The event brings the state.
    }
  }

  // Remote companion (P26): the header says so while it is on, from any tab.
  let remote = $state.raw<RemoteBrief | null>(null);
  function openRemote(): void {
    openSection('settings', 'remote');
  }
  /** A view, scrolled to one of its cards (the card loads with the panel). */
  function openSection(t: Tab, id: string): void {
    select(t);
    setTimeout(() => document.getElementById(id)?.scrollIntoView({ block: 'start', behavior: 'smooth' }), 150);
  }

  // What's new (1.5): after an update, the new versions' notes once; from Settings or the
  // palette, every version's. Loaded when it opens.
  let whatsNew = $state<{ current: string; from: string | null; all: boolean } | null>(null);
  function openWhatsNew(): void {
    whatsNew = { current: inTauri ? (update.status?.current ?? '') : '', from: null, all: true };
  }
  function closeWhatsNew(): void {
    const shown = whatsNew;
    whatsNew = null;
    if (shown && !shown.all) void whatsNewSeen().catch(() => {});
  }

  // Command palette (P28): Ctrl+K from anywhere; loaded the first time it opens.
  let palette = $state(false);
  let paletteLoaded = $state(false);
  function openPalette(): void {
    paletteLoaded = true;
    palette = true;
  }

  // The live rail beside the Dashboard's widgets on a wide window (P28), unless folded away.
  const RAIL_KEY = 'vyro.rail';
  let wide = $state(false);
  let railWanted = $state(
    (() => {
      try {
        return localStorage.getItem(RAIL_KEY) !== '0';
      } catch {
        return true;
      }
    })(),
  );
  function setRail(on: boolean): void {
    railWanted = on;
    try {
      localStorage.setItem(RAIL_KEY, on ? '1' : '0');
    } catch {
      /* not remembered */
    }
  }
  const railOn = $derived(wide && railWanted && snap != null);

  // Dots in the sidebar, with their words for screen readers.
  const dots = $derived.by(() => {
    const d: Partial<Record<Tab, Dot>> = {};
    if (overall === 'warn' || overall === 'fail') d.health = { tone: overall, note: overall === 'fail' ? 'problem' : 'warning' };
    if (newestChange > changesSeen) d.hardware = { tone: 'new', note: 'new PC changes' };
    if (insightsNew.length) d.insights = { tone: 'new', note: 'new findings' };
    if (low) d.devices = { tone: low.tone, note: `low battery: ${low.text}` };
    return d;
  });
  /** Where readings come from, for the sidebar's foot. */
  const source = $derived.by((): { tone: 'ok' | 'warn' | 'off'; text: string } => {
    if (!inTauri) return { tone: 'warn', text: 'Preview, simulated' };
    if (!backend) return { tone: 'off', text: 'Starting…' };
    if (backend.mode === 'local') return { tone: 'warn', text: 'Without the service' };
    return backend.connected ? { tone: 'ok', text: 'Sensor service' } : { tone: 'off', text: 'Connecting…' };
  });

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
    getWhatsNew()
      .then((a) => {
        if (a) whatsNew = { ...a, all: false };
      })
      .catch(() => {});
    let stop: (() => void) | undefined;
    let stopBackend: (() => void) | undefined;
    let stopChanges: (() => void) | undefined;
    let stopOpened: (() => void) | undefined;
    let stopRemote: (() => void) | undefined;
    let stopLow: (() => void) | undefined;
    let stopPower: (() => void) | undefined;
    let stopScreen: (() => void) | undefined;
    let stopView: (() => void) | undefined;
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
    // A desktop widget's "Edit…" opens its page.
    void onOpenView((v) => {
      if (isTab(v)) select(v);
    }).then((fn) => {
      if (disposed) fn();
      else stopView = fn;
    });
    getBackendStatus()
      .then((s) => (backend = s))
      .catch(() => {});
    getRemoteBrief()
      .then((b) => (remote = b))
      .catch(() => {});
    getLowBatteries()
      .then((v) => (lowBatteries = v))
      .catch(() => {});
    void onLowBattery((v) => (lowBatteries = v)).then((fn) => {
      if (disposed) fn();
      else stopLow = fn;
    });
    getPowerState()
      .then((p) => (pendingPower = p.pending))
      .catch(() => {});
    void onPowerPending((p) => (pendingPower = p.pending)).then((fn) => {
      if (disposed) fn();
      else stopPower = fn;
    });
    getScreenStatus()
      .then((s) => ((screenShare = s.share), (screenMic = s.mic)))
      .catch(() => {});
    void onScreenShare((s) => ((screenShare = s.share), (screenMic = s.mic))).then((fn) => {
      if (disposed) fn();
      else stopScreen = fn;
    });
    void onRemote((b) => (remote = b)).then((fn) => {
      if (disposed) fn();
      else stopRemote = fn;
    });
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
      pageHidden = document.hidden;
      animator.setHidden(document.hidden);
      setUiVisible(!document.hidden);
    };
    document.addEventListener('visibilitychange', onVisibility);
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        openPalette();
      }
    };
    window.addEventListener('keydown', onKey);
    const mq = window.matchMedia('(min-width: 1500px)');
    wide = mq.matches;
    const onWide = (e: MediaQueryListEvent) => (wide = e.matches);
    mq.addEventListener('change', onWide);
    return () => {
      window.removeEventListener('keydown', onKey);
      mq.removeEventListener('change', onWide);
      disposed = true;
      stop?.();
      stopBackend?.();
      stopChanges?.();
      stopOpened?.();
      stopRemote?.();
      stopLow?.();
      stopPower?.();
      stopScreen?.();
      stopView?.();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });
</script>

<div class="shell">
  <Sidebar {tab} onselect={select} {dots} status={source} />

  <div class="main">
    <TopBar
      {tab}
      {remote}
      {fans}
      gpu={gpuTune}
      update={update.status?.available && !update.status.installing ? update.status.available.version : null}
      onselect={select}
      onremote={openRemote}
      onbios={fansToBios}
      onstock={gpuToStock}
      onsearch={openPalette}
    />

    <main>
      {#if pendingPower}
        <div class="power-banner" role="alert">
          <span>
            <b>{POWER_WORD[pendingPower.action] ?? pendingPower.action}</b> at {new Date(pendingPower.at * 1000).toLocaleTimeString([], {
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
            })}, asked from {pendingPower.by}.{powerError ? ` ${powerError}` : ''}
          </span>
          <button type="button" onclick={() => void cancelPower()}>Cancel</button>
        </div>
      {/if}
      {#if screenShare}
        <div class="power-banner screen-banner" role="status">
          <span>
            <b>Your screen is shared</b> with {screenShare.phone}{screenShare.slow ? ' (slow view)' : ''}: it sees this screen and can use the
            mouse and keyboard.{screenMic ? ' Its microphone is on: this PC hears the phone.' : ''}
          </span>
          <button type="button" onclick={() => void stopScreen()}>Stop sharing</button>
        </div>
      {/if}
      {#if visited.cooling}
        <div id="panel-cooling" role="tabpanel" aria-labelledby="tab-cooling" hidden={tab !== 'cooling'}>
          <CoolingPanel {snap} {backend} active={tab === 'cooling'} />
        </div>
      {/if}
      <div id="panel-dashboard" role="tabpanel" aria-labelledby="tab-dashboard" hidden={tab !== 'dashboard'}>
        <div class="dash" class:with-rail={railOn}>
          <div class="dash-main">
            <ServiceBanner status={backend} />
            {#if !snap}
              <p class="waiting" role="status">
                {backend && !backend.connected ? 'Connecting to the Vyro Sensor Service…' : 'Waiting for the first reading…'}
              </p>
            {/if}
            <PawnIoBanner status={pawnioShown} />
            {#if attention.length}
              <button type="button" class="health-strip {overall}" onclick={() => select('health')}>
                <span class="alert-dot {overall}" aria-hidden="true"></span>
                <span class="msg">{attention[0]}{attention.length > 1 ? ` (+${attention.length - 1} more)` : ''}</span>
                <span class="go">Health →</span>
              </button>
            {/if}
            {#if low}
              <button type="button" class="health-strip {low.tone}" onclick={() => select('devices')}>
                <span class="alert-dot {low.tone}" aria-hidden="true"></span>
                <span class="msg"><b>Low battery:</b> {low.text}</span>
                <span class="go">Devices →</span>
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
              onrail={wide && !railWanted ? () => setRail(true) : undefined}
            />
          </div>
          {#if railOn && snap}
            <LiveRail {snap} {attention} onselect={select} onfold={() => setRail(false)} />
          {/if}
        </div>
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

      {#if visited.gpu}
        <div id="panel-gpu" role="tabpanel" aria-labelledby="tab-gpu" hidden={tab !== 'gpu'}>
          <!-- Loaded on first visit, as Benchmark. -->
          {#await import('./components/GpuTunePanel.svelte') then { default: GpuTunePanel }}
            <GpuTunePanel {snap} {backend} active={tab === 'gpu'} />
          {/await}
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
      {#if visited.mypc}
        <div id="panel-mypc" role="tabpanel" aria-labelledby="tab-mypc" hidden={tab !== 'mypc'}>
          <!-- Loaded on first visit; its 3-D view (Three.js) exists only while the tab shows. -->
          {#await import('./components/MyPcPanel.svelte') then { default: MyPcPanel }}
            <MyPcPanel {snap} active={tab === 'mypc' && !pageHidden} onselect={select} />
          {/await}
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
      {#if visited.widgets}
        <div id="panel-widgets" role="tabpanel" aria-labelledby="tab-widgets" hidden={tab !== 'widgets'}>
          <!-- Loaded on first visit. -->
          {#await import('./components/WidgetsPanel.svelte') then { default: WidgetsPanel }}
            <WidgetsPanel {snap} />
          {/await}
        </div>
      {/if}
      {#if visited.settings}
        <div id="panel-settings" role="tabpanel" aria-labelledby="tab-settings" hidden={tab !== 'settings'}>
          <SettingsPanel {snap} {backend} active={tab === 'settings'} onwhatsnew={openWhatsNew} />
        </div>
      {/if}
    </main>

    <AppStatus app={snap?.app ?? null} {uiFps} source={inTauri ? 'engine' : 'browser sim'} />
  </div>

  <div class="toasts" role="status" aria-live="polite">
    {#each toasts as t (t.key)}
      <button type="button" class="toast" onclick={() => select('health')}>
        <b>{tempText(t.title)}</b>
        <span>{tempText(t.message)}</span>
      </button>
    {/each}
  </div>
</div>

{#if paletteLoaded}
  {#await import('./components/shell/CommandPalette.svelte') then { default: CommandPalette }}
    <CommandPalette
      bind:open={palette}
      {snap}
      fansActive={fans?.active ?? false}
      onselect={select}
      onsection={openSection}
      onhistory={(id) => openHistory(id)}
      onbios={fansToBios}
      onwhatsnew={openWhatsNew}
    />
  {/await}
{/if}

{#if whatsNew}
  {#await import('./components/WhatsNew.svelte') then { default: WhatsNew }}
    <WhatsNew current={whatsNew.current} from={whatsNew.from} all={whatsNew.all} onclose={closeWhatsNew} />
  {/await}
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    min-height: 100vh;
  }
  .main {
    --gutter: clamp(0.75rem, 1.8vw, 2rem);
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 0 var(--gutter) 0.8rem;
  }
  main {
    flex: 1;
  }
  /* A view comes in with a short rise (nothing with the profile's animation off). */
  [role='tabpanel']:not([hidden]) {
    animation: view-in var(--dur-3) var(--ease-out);
  }
  @keyframes view-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .dash.with-rail {
    display: grid;
    grid-template-columns: minmax(0, 1fr) clamp(17rem, 18vw, 21rem);
    align-items: start;
    gap: 1.25rem;
  }
  .dash-main {
    min-width: 0;
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
    padding: 0.7rem 0.9rem;
    border: 1px solid color-mix(in srgb, var(--hot) 50%, var(--line));
    border-radius: 12px;
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
  .toast {
    animation: toast-in var(--dur-3) var(--ease-out);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(24px);
    }
  }
  .alert-dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--warn);
  }
  .alert-dot.fail {
    background: var(--hot);
  }
  .power-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin: 0 0 0.9rem;
    padding: 0.6rem 0.9rem;
    border: 1px solid color-mix(in srgb, var(--hot) 55%, var(--line));
    border-radius: 12px;
    background: color-mix(in srgb, var(--hot) 10%, var(--panel));
    color: var(--text);
    font-size: 0.9rem;
  }
  .screen-banner {
    border-color: color-mix(in srgb, var(--warn) 55%, var(--line));
    background: color-mix(in srgb, var(--warn) 10%, var(--panel));
  }
  .health-strip {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    margin: 0 0 0.9rem;
    padding: 0.6rem 0.9rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 12px;
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
    transition: border-color var(--dur-2);
  }
  .health-strip:hover {
    border-color: var(--warn);
  }
  .health-strip.fail {
    border-color: color-mix(in srgb, var(--hot) 55%, var(--line));
    background: color-mix(in srgb, var(--hot) 10%, var(--panel));
  }
  .health-strip .alert-dot {
    flex: none;
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
  .waiting {
    margin: 0 0 0.9rem;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
