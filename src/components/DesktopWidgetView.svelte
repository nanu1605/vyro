<script lang="ts">
  // A desktop widget's face (P30): a gauge, a reading, a graph, a stack of readings or a clock,
  // and since 1.2 the network, the running game's frame rate or the busiest programs, in the
  // app's theme or see-through. Drawn by the widget's own window and by the previews on the
  // Widgets page; the window adds dragging and its menu.
  import { onMount } from 'svelte';
  import Spark from './Spark.svelte';
  import { readingDef, readingView } from '../dashboard';
  import { fmt, fmtMB } from '../format';
  import { prefs } from '../prefs.svelte';
  import { PROC_SORTS, kindOf, sortOf, topProcesses, type DesktopWidget } from '../widgets';
  import type { SensorSnapshot } from '../types';

  interface Props {
    w: DesktopWidget;
    snap: SensorSnapshot | null;
  }
  let { w, snap }: Props = $props();

  const rows = $derived(
    w.sources.flatMap((id) => {
      const r = readingDef(id, snap);
      if (!r) return [];
      const v = readingView(r, snap, 'not reported', prefs.temperatureUnit);
      return [{ id, label: r.label, digits: r.digits, ...v }];
    }),
  );
  const first = $derived(rows[0] ?? null);
  const level = (r: { value: number | null; warn?: number; hot?: number }) =>
    r.value == null ? 'na' : r.hot != null && r.value >= r.hot ? 'hot' : r.warn != null && r.value >= r.warn ? 'warn' : 'ok';

  // Gauge: a 270° dial; percentages and temperatures have fixed tops, the rest grow with use.
  let peak = $state(1);
  $effect(() => {
    if (first?.value != null && first.value > peak) peak = first.value;
  });
  const top = $derived(!first ? 100 : first.unit === '%' ? 100 : first.hot != null ? first.hot * 1.1 : Math.max(1, peak * 1.15));
  const frac = $derived(first?.value != null ? Math.max(0, Math.min(1, first.value / top)) : 0);
  const R = 78;
  const ARC = (270 / 360) * 2 * Math.PI * R;
  // Text that stays inside (1.2): a long number gets smaller, and so does a reading's.
  const firstText = $derived(fmt(first?.value, first?.digits ?? 0));
  const bigPx = $derived(firstText.length <= 5 ? 44 : Math.round((44 * 5) / firstText.length));
  /** The dial's label, cut with "…" to the width under the dial (the full one is its aria-label). */
  const dialLabel = $derived(cut(first?.label ?? '—', 21));
  function cut(s: string, n: number): string {
    return s.length <= n ? s : `${s.slice(0, n - 1).trimEnd()}…`;
  }

  // Network: the adapter with the default route, as the Dashboard's readings use.
  const net = $derived(snap?.network.find((n) => n.primary && n.connected) ?? snap?.network.find((n) => n.connected) ?? null);
  /** A rate in Mbps, or Gbps from 1000 Mbps, so both fit side by side. */
  const rate = (mbps: number | null | undefined) =>
    mbps == null ? { v: fmt(null), u: '' } : mbps >= 1000 ? { v: fmt(mbps / 1000, 2), u: 'Gbps' } : { v: fmt(mbps, 1), u: 'Mbps' };
  const down = $derived(rate(net?.downMbps));
  const up = $derived(rate(net?.upMbps));

  // Game: while Vyro measures one.
  const game = $derived(snap?.game && snap.game.state === 'capturing' ? snap.game : null);

  // Top processes.
  const by = $derived(sortOf(w));
  const procs = $derived(w.kind === 'top' && snap ? topProcesses(snap.processes, by) : []);
  const noGpu = $derived(by === 'proc.gpu' && !!snap && snap.processes.every((p) => p.gpuPct == null));
  /** A bar's length: % of the CPU or GPU, or the share of all memory. */
  const share = (v: number) => Math.max(0, Math.min(1, by === 'proc.ram' ? v / (snap?.memory.totalMB || Math.max(1, ...procs.map((r) => r.value))) : v / 100));

  // Clock.
  let now = $state(new Date());
  onMount(() => {
    if (w.kind !== 'clock') return;
    const t = setInterval(() => (now = new Date()), 1000);
    return () => clearInterval(t);
  });
  const time = $derived(now.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }));
  const date = $derived(now.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' }));
</script>

<div class="desk {w.style} {w.kind}" style:--bgk="{w.opacity}%" style:zoom={w.scale / 100}>
  {#if w.kind === 'gauge'}
    <svg viewBox="0 0 200 200" class="dial" role="img" aria-label="{first?.label ?? 'Reading'}: {fmt(first?.value, first?.digits ?? 0)} {first?.unit ?? ''}">
      <circle cx="100" cy="104" r={R} class="track" stroke-dasharray="{ARC} 999" transform="rotate(135 100 104)" />
      <circle
        cx="100"
        cy="104"
        r={R}
        class="value {first ? level(first) : 'na'}"
        stroke-dasharray="{ARC * frac} 999"
        transform="rotate(135 100 104)"
      />
      <text x="100" y="98" class="big" style:font-size="{bigPx}px">{firstText}</text>
      <text x="100" y="134" class="unit">{first?.unit ?? ''}</text>
      <text x="100" y="178" class="label">{dialLabel}</text>
    </svg>
  {:else if w.kind === 'reading'}
    <span class="label">{first?.label ?? '—'}</span>
    <span class="value {first ? level(first) : 'na'}" class:long={firstText.length > 6}
      ><b>{firstText}</b><small>{first?.value == null ? '' : first.unit}</small></span
    >
    {#if first?.sub}<span class="sub">{first.sub}</span>{/if}
  {:else if w.kind === 'graph'}
    <div class="head">
      <span class="label">{first?.label ?? '—'}</span>
      <span class="value {first ? level(first) : 'na'}"><b>{fmt(first?.value, first?.digits ?? 0)}</b><small>{first?.value == null ? '' : first.unit}</small></span>
    </div>
    <Spark ts={snap?.ts ?? 0} series={[{ value: first?.value, color: 'var(--accent)' }]} max={first?.unit === '%' ? 100 : undefined} label="{first?.label ?? 'Reading'}, last minute" />
  {:else if w.kind === 'stack'}
    <ul>
      {#each rows as r (r.id)}
        <li class={level(r)}>
          <span class="label">{r.label}</span>
          <span class="num"><b>{fmt(r.value, r.digits)}</b> <small>{r.value == null ? '' : r.unit}</small></span>
          {#if r.unit === '%' || r.hot != null}
            <span class="bar" aria-hidden="true"><i style:transform="scaleX({Math.max(0, Math.min(1, (r.value ?? 0) / (r.unit === '%' ? 100 : (r.hot ?? 100))))})"></i></span>
          {/if}
        </li>
      {:else}
        <li class="na"><span class="label">Pick readings on the Widgets page</span></li>
      {/each}
    </ul>
  {:else if w.kind === 'network'}
    <span class="label">{net?.adapter ?? 'Not connected'}</span>
    <div class="pair">
      <span class="down"><i aria-hidden="true">↓</i><span class="sr-only">Download</span><b>{down.v}</b><small>{down.u}</small></span>
      <span class="up"><i aria-hidden="true">↑</i><span class="sr-only">Upload</span><b>{up.v}</b><small>{up.u}</small></span>
    </div>
    <Spark
      ts={snap?.ts ?? 0}
      series={[
        { value: net?.downMbps, color: 'var(--accent)' },
        { value: net?.upMbps, color: 'var(--muted)' },
      ]}
      label="Download and upload, last minute"
    />
  {:else if w.kind === 'game'}
    {#if game}
      <div class="head">
        <span class="label">{game.app}</span>
        <span class="value"><b>{fmt(game.fps)}</b><small>FPS</small></span>
      </div>
      <span class="sub">1% low {fmt(game.low1Fps)} · {fmt(game.frameMs, 1)} ms a frame</span>
      <Spark ts={snap?.ts ?? 0} series={[{ value: game.fps, color: 'var(--accent)' }]} label="Frame rate, last minute" />
    {:else}
      <span class="label">Game FPS</span>
      <span class="value na"><b>—</b></span>
      <span class="sub">No game running. Its frame rate shows here while Vyro measures it.</span>
    {/if}
  {:else if w.kind === 'top'}
    <span class="label">Top processes · {PROC_SORTS.find((s) => s.id === by)?.name}</span>
    <ul>
      {#each procs as r (r.name)}
        <li>
          <span class="pname">{r.name}{#if r.count > 1}<small>×{r.count}</small>{/if}</span>
          <span class="num"><b>{by === 'proc.ram' ? fmtMB(r.value) : `${fmt(r.value, 1)} %`}</b></span>
          <span class="bar" aria-hidden="true"><i style:transform="scaleX({share(r.value)})"></i></span>
        </li>
      {:else}
        <li class="na"><span class="pname">{noGpu ? 'GPU use per program is not reported' : snap ? 'Nothing busy right now' : 'Waiting for readings'}</span></li>
      {/each}
    </ul>
  {:else}
    <span class="time">{time}</span>
    <span class="date">{date}</span>
    {#if rows.length}
      <!-- One reading a row (1.2): a long name is cut short, never past the edge. -->
      <ul class="readings">
        {#each rows as r (r.id)}
          <li class={level(r)}>
            <span class="label">{r.label}</span>
            <span class="num"><b>{fmt(r.value, r.digits)}</b> <small>{r.value == null ? '' : r.unit}</small></span>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
  <span class="sr-only">{kindOf(w.kind).name} widget</span>
</div>

<style>
  .desk {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: center;
    width: 100%;
    height: 100%;
    padding: 0.8rem 1rem;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--line));
    border-radius: 16px;
    color: var(--text);
    font-family: var(--font-ui);
    font-variant-numeric: tabular-nums;
  }
  .app {
    background: linear-gradient(160deg, color-mix(in srgb, var(--card-top) var(--bgk), transparent), color-mix(in srgb, var(--card) var(--bgk), transparent));
    box-shadow: inset 0 1px 0 color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .glass {
    border-color: rgb(255 255 255 / 14%);
    background: linear-gradient(160deg, rgb(255 255 255 / calc(var(--bgk) * 0.12)), rgb(10 14 22 / calc(var(--bgk) * 0.55)));
    color: #f2f6fa;
    text-shadow: 0 1px 2px rgb(0 0 0 / 55%);
  }
  .solid {
    background: var(--panel);
  }
  .label {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .glass .label,
  .glass small,
  .glass .sub,
  .glass .date {
    color: #c5ced8;
  }
  .value {
    white-space: nowrap;
  }
  .value b {
    font-family: var(--font-num);
    font-size: 2.2rem;
    font-weight: 650;
    line-height: 1.05;
  }
  .value.long b {
    font-size: 1.7rem;
  }
  .value small,
  .num small {
    margin-left: 0.2rem;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .warn b,
  .warn .num b {
    color: var(--warn);
  }
  .hot b {
    color: var(--hot);
  }
  .sub {
    color: var(--muted);
    font-size: 0.72rem;
  }
  /* Gauge */
  .gauge {
    padding: 0.4rem;
  }
  .dial {
    width: 100%;
    height: 100%;
  }
  .dial circle {
    fill: none;
    stroke-linecap: round;
    stroke-width: 13;
  }
  .track {
    stroke: var(--gauge-track);
  }
  .dial .value {
    stroke: var(--accent);
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--accent) 55%, transparent));
    transition: stroke-dasharray 0.8s var(--ease-out);
  }
  .dial .value.warn {
    stroke: var(--warn);
  }
  .dial .value.hot {
    stroke: var(--hot);
  }
  .dial .value.na {
    stroke: transparent;
  }
  .dial text {
    fill: currentColor;
    text-anchor: middle;
  }
  .big {
    font-family: var(--font-num);
    font-size: 44px;
    font-weight: 650;
    dominant-baseline: middle;
  }
  .unit {
    fill: var(--muted) !important;
    font-size: 15px;
  }
  .dial .label {
    fill: var(--muted) !important;
    font-size: 13px;
  }
  /* Graph */
  .graph {
    justify-content: space-between;
  }
  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .head .value b {
    font-size: 1.6rem;
  }
  .graph :global(.wrap) {
    margin: 0.2rem 0 0;
  }
  .graph :global(.spark) {
    height: 4.6rem;
  }
  /* Stack */
  ul {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-rows: auto 4px;
    align-items: baseline;
    column-gap: 0.6rem;
    row-gap: 0.2rem;
    height: 1.95rem;
  }
  li .label {
    font-size: 0.68rem;
  }
  .bar {
    grid-column: 1 / -1;
    height: 4px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--gauge-track);
  }
  /* Scaled, not sized: no layout or repaint while it moves. */
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform 0.8s var(--ease-out);
  }
  li.warn .bar i {
    background: var(--warn);
  }
  li.hot .bar i {
    background: var(--hot);
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .num b {
    font-size: 1rem;
  }
  /* Network, game */
  .pair {
    display: flex;
    gap: 1.1rem;
    margin-top: 0.15rem;
  }
  .pair > span {
    min-width: 0;
    white-space: nowrap;
  }
  .pair i {
    margin-right: 0.25rem;
    font-style: normal;
    font-weight: 700;
  }
  .pair .down i {
    color: var(--accent);
  }
  .pair .up i {
    color: var(--muted);
  }
  .pair b {
    font-family: var(--font-num);
    font-size: 1.35rem;
    font-weight: 650;
  }
  .pair small {
    margin-left: 0.2rem;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .network,
  .game {
    justify-content: space-between;
  }
  .network :global(.spark),
  .game :global(.spark) {
    height: 3.3rem;
  }
  .game.desk > .value b {
    font-size: 1.8rem;
  }
  /* Top processes */
  .top ul {
    gap: 0.2rem;
    margin-top: 0.35rem;
  }
  .top li {
    height: 1.55rem;
  }
  .pname {
    overflow: hidden;
    font-size: 0.78rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pname small {
    margin-left: 0.3rem;
    color: var(--muted);
  }
  .glass .pname small {
    color: #c5ced8;
  }
  .top .num b {
    font-size: 0.85rem;
  }
  /* Clock */
  .clock {
    align-items: flex-start;
  }
  .time {
    font-family: var(--font-num);
    white-space: nowrap;
    font-size: 2.6rem;
    font-weight: 650;
    line-height: 1;
  }
  .date {
    margin-top: 0.2rem;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .readings {
    align-self: stretch;
    gap: 0.1rem;
    margin-top: 0.4rem;
  }
  .readings li {
    grid-template-rows: auto;
    height: auto;
  }
  .readings .num b {
    font-family: var(--font-num);
    font-size: 0.9rem;
  }
</style>
