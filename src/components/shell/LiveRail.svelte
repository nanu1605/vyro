<script lang="ts">
  // The Dashboard's live rail (P28): on a wide window, the room beside the widgets shows the
  // vitals with their last minute, what uses the CPU, the network and anything that needs a
  // look. Folds away (remembered).
  import PanelRightClose from '@lucide/svelte/icons/panel-right-close';
  import Spark from '../Spark.svelte';
  import { fmt } from '../../format';
  import { fmtTemp, tempText } from '../../prefs.svelte';
  import type { Tab } from '../../nav';
  import type { SensorSnapshot } from '../../types';

  interface Props {
    snap: SensorSnapshot;
    /** Worst first. */
    attention: string[];
    onselect: (t: Tab) => void;
    onfold: () => void;
  }
  let { snap, attention, onselect, onfold }: Props = $props();

  const gpu = $derived(snap.gpu.find((g) => g.integrated === false) ?? snap.gpu[0] ?? null);
  const mem = $derived(snap.memory.usedMB != null && snap.memory.totalMB ? (snap.memory.usedMB / snap.memory.totalMB) * 100 : null);
  const net = $derived(snap.network.find((n) => n.primary && n.connected) ?? snap.network.find((n) => n.connected) ?? null);
  const top = $derived(
    [...snap.processes]
      .filter((p) => p.cpuPct != null)
      .sort((a, b) => (b.cpuPct ?? 0) - (a.cpuPct ?? 0))
      .slice(0, 5),
  );
  const game = $derived(snap.game && snap.game.state === 'capturing' ? snap.game : null);
</script>

<aside class="rail" aria-label="Live">
  <header>
    <h2><span class="live" aria-hidden="true"></span>Live</h2>
    <button type="button" class="fold" onclick={onfold} aria-label="Hide the live panel" title="Hide">
      <PanelRightClose size={16} aria-hidden="true" />
    </button>
  </header>

  {#if game}
    <button type="button" class="block game" onclick={() => onselect('gaming')}>
      <span class="k">{game.app}</span>
      <span class="v"><b>{fmt(game.fps)}</b> FPS</span>
    </button>
  {/if}

  <section class="block">
    <div class="row">
      <span class="k">CPU</span>
      <span class="v"><b>{fmt(snap.cpu.usageTotal)}</b> %{#if snap.cpu.tempPackage != null}<i>{fmtTemp(snap.cpu.tempPackage)}</i>{/if}</span>
    </div>
    <Spark ts={snap.ts} series={[{ value: snap.cpu.usageTotal, color: 'var(--accent-cpu-fill)' }]} max={100} label="CPU use, last minute" />
    {#if gpu}
      <div class="row">
        <span class="k">GPU</span>
        <span class="v"><b>{fmt(gpu.usage)}</b> %{#if gpu.tempEdge != null}<i>{fmtTemp(gpu.tempEdge)}</i>{/if}</span>
      </div>
      <Spark ts={snap.ts} series={[{ value: gpu.usage, color: 'var(--accent-gpu-fill)' }]} max={100} label="GPU use, last minute" />
    {/if}
    <div class="row">
      <span class="k">Memory</span>
      <span class="v"><b>{fmt((snap.memory.usedMB ?? 0) / 1024, 1)}</b> GB<i>{fmt(mem)} %</i></span>
    </div>
    <Spark ts={snap.ts} series={[{ value: mem, color: 'var(--series-3)' }]} max={100} label="Memory use, last minute" />
    {#if snap.power.estTotalW != null}
      <div class="row">
        <span class="k">Power</span>
        <span class="v"><b>{fmt(snap.power.estTotalW)}</b> W<i>estimated</i></span>
      </div>
      <Spark ts={snap.ts} series={[{ value: snap.power.estTotalW, color: 'var(--series-4)' }]} floor={50} label="System power, last minute" />
    {/if}
  </section>

  {#if net}
    <section class="block">
      <div class="row">
        <span class="k">Network</span>
        <span class="v">↓ <b>{fmt(net.downMbps, 1)}</b> ↑ <b>{fmt(net.upMbps, 1)}</b> Mbps</span>
      </div>
      <Spark
        ts={snap.ts}
        series={[
          { value: net.downMbps, color: 'var(--accent-cpu-fill)' },
          { value: net.upMbps, color: 'var(--accent-gpu-fill)' },
        ]}
        floor={1}
        label="Network traffic, last minute"
      />
      {#if snap.ping.rttMs != null}<div class="row small"><span class="k">Ping {snap.ping.host}</span><span class="v">{fmt(snap.ping.rttMs)} ms</span></div>{/if}
    </section>
  {/if}

  {#if top.length}
    <section class="block">
      <button type="button" class="head" onclick={() => onselect('processes')}>Top processes <span aria-hidden="true">→</span></button>
      <ol class="procs">
        {#each top as p (p.pid)}
          <li>
            <span class="pname" title={p.name}>{p.name}</span>
            <span class="bar" aria-hidden="true"><span style:transform="scaleX({Math.min(100, Math.max(0, p.cpuPct ?? 0)) / 100})"></span></span>
            <span class="pct">{fmt(p.cpuPct, 1)} %</span>
          </li>
        {/each}
      </ol>
    </section>
  {/if}

  <section class="block">
    <button type="button" class="head" onclick={() => onselect('health')}>Health <span aria-hidden="true">→</span></button>
    {#if attention.length}
      <ul class="att">
        {#each attention.slice(0, 4) as a, i (i)}<li>{tempText(a)}</li>{/each}
      </ul>
    {:else}
      <p class="ok">Everything looks fine.</p>
    {/if}
  </section>
</aside>

<style>
  .rail {
    position: sticky;
    top: calc(var(--topbar-h) + 1rem);
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    max-height: calc(100vh - var(--topbar-h) - 2rem);
    overflow-y: auto;
    scrollbar-width: thin;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .live {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .fold {
    display: grid;
    place-items: center;
    width: 1.8rem;
    height: 1.8rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .fold:hover {
    color: var(--text);
  }
  .block {
    padding: 0.7rem 0.8rem 0.3rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: linear-gradient(180deg, var(--card-top), var(--card) 40%);
    box-shadow: var(--shadow-card);
  }
  .game {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    width: 100%;
    padding-bottom: 0.7rem;
    border-color: color-mix(in srgb, var(--accent-gpu) 45%, var(--line));
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .game .v b {
    color: var(--accent-gpu);
    font-size: 1.3rem;
  }
  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    font-size: 0.8rem;
  }
  .row.small {
    padding-bottom: 0.4rem;
    font-size: 0.74rem;
  }
  .k {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .v {
    font-family: var(--font-num);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .v b {
    font-size: 0.95rem;
    font-weight: 650;
  }
  .v i {
    margin-left: 0.5rem;
    color: var(--muted);
    font-style: normal;
  }
  .rail :global(.wrap) {
    margin: 0.15rem 0 0.45rem;
  }
  .rail :global(.spark) {
    height: 1.9rem;
  }
  .head {
    display: flex;
    justify-content: space-between;
    width: 100%;
    margin-bottom: 0.4rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .head:hover {
    color: var(--text);
  }
  .procs,
  .att {
    margin: 0 0 0.5rem;
    padding: 0;
    list-style: none;
  }
  .procs li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 3rem 3.2rem;
    align-items: center;
    gap: 0.5rem;
    padding: 0.18rem 0;
    font-size: 0.78rem;
  }
  .pname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar {
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--gauge-track);
  }
  /* Scaled, not sized: no layout or repaint while it moves. */
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform var(--dur-3) var(--ease-out);
  }
  .pct {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .att li {
    position: relative;
    padding: 0.2rem 0 0.2rem 0.9rem;
    font-size: 0.78rem;
  }
  .att li::before {
    position: absolute;
    top: 0.55rem;
    left: 0;
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 50%;
    background: var(--warn);
    content: '';
  }
  .ok {
    margin: 0 0 0.6rem;
    color: var(--muted);
    font-size: 0.78rem;
  }
</style>
