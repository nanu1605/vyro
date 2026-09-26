<script lang="ts">
  import { onMount } from 'svelte';
  import GaugePanel from './components/GaugePanel.svelte';
  import StatTile from './components/StatTile.svelte';
  import AppStatus from './components/AppStatus.svelte';
  import HardwarePanel from './components/HardwarePanel.svelte';
  import SystemPanel from './components/SystemPanel.svelte';
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
  import GpuCard from './components/GpuCard.svelte';
  import ProcessesPanel from './components/ProcessesPanel.svelte';
  import SensorsPanel from './components/SensorsPanel.svelte';
  import { animator } from './anim';
  import { fmt, fmtGB } from './format';
  import { getPawnIoStatus, inTauri, onSensors, setUiVisible } from './ipc';
  import type { PawnIoStatus, SensorSnapshot } from './types';

  const TABS = [
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'processes', label: 'Processes' },
    { id: 'hardware', label: 'Hardware' },
    { id: 'storage', label: 'Storage' },
    { id: 'network', label: 'Network' },
    { id: 'displays', label: 'Displays' },
    { id: 'system', label: 'System' },
    { id: 'sensors', label: 'Sensors' },
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
    processes: false,
    hardware: false,
    storage: false,
    network: false,
    displays: false,
    system: false,
    sensors: false,
  });

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
  let uiFps = $state(0);

  const cpu = $derived(snap?.cpu ?? null);
  const gpu = $derived(snap?.gpu[0] ?? null);
  const mem = $derived(snap?.memory ?? null);
  // CPU temperature / power and board fans come through PawnIO (Phase 6).
  let pawnio = $state.raw<PawnIoStatus | null>(null);
  const NEEDS_DRIVER = $derived(pawnio?.state === 'ready' ? 'not reported' : 'needs PawnIO');
  const cpuFan = $derived(snap?.fans.find((f) => f.kind === 'fan') ?? null);

  onMount(() => {
    visited[tab] = true;
    getPawnIoStatus()
      .then((s) => (pawnio = s))
      .catch(() => {});
    let stop: (() => void) | undefined;
    let disposed = false;
    void onSensors((s) => {
      snap = s;
      uiFps = animator.fps;
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
        </button>
      {/each}
    </div>
  </header>

  <main>
    <div id="panel-dashboard" role="tabpanel" aria-labelledby="tab-dashboard" hidden={tab !== 'dashboard'}>
      <PawnIoBanner status={pawnio} />
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
        <StatTile label="CPU package" value={cpu?.tempPackage} unit="°C" warn={80} hot={95} sub={cpu?.tempPackage == null ? NEEDS_DRIVER : null} />
        <StatTile label="CPU power" value={cpu?.powerPkgW} unit="W" digits={1} sub={cpu?.powerPkgW == null ? NEEDS_DRIVER : null} />
        <StatTile label="GPU temp" value={gpu?.tempEdge} unit="°C" warn={80} hot={90} />
        <StatTile label="GPU hotspot" value={gpu?.tempHotspot} unit="°C" warn={90} hot={105} />
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
        <SensorsPanel {snap} active={tab === 'sensors'} />
      </div>
    {/if}
  </main>

  <AppStatus app={snap?.app ?? null} {uiFps} source={inTauri ? 'engine' : 'browser sim'} />
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
