<script lang="ts">
  // A desktop widget's face (P30): a gauge, a reading, a graph, a stack of readings or a clock,
  // in the app's theme or see-through. Drawn by the widget's own window and by the previews on
  // the Widgets page; the window adds dragging and its menu.
  import { onMount } from 'svelte';
  import Spark from './Spark.svelte';
  import { readingDef, readingView } from '../dashboard';
  import { fmt } from '../format';
  import { prefs } from '../prefs.svelte';
  import { kindOf, type DesktopWidget } from '../widgets';
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
      <text x="100" y="98" class="big">{fmt(first?.value, first?.digits ?? 0)}</text>
      <text x="100" y="134" class="unit">{first?.unit ?? ''}</text>
      <text x="100" y="178" class="label">{first?.label ?? '—'}</text>
    </svg>
  {:else if w.kind === 'reading'}
    <span class="label">{first?.label ?? '—'}</span>
    <span class="value {first ? level(first) : 'na'}"><b>{fmt(first?.value, first?.digits ?? 0)}</b><small>{first?.value == null ? '' : first.unit}</small></span>
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
  {:else}
    <span class="time">{time}</span>
    <span class="date">{date}</span>
    {#if rows.length}
      <span class="line">
        {#each rows as r, i (r.id)}{#if i}<i aria-hidden="true">·</i>{/if}<span class={level(r)}>{r.label} <b>{fmt(r.value, r.digits)}</b> {r.value == null ? '' : r.unit}</span>{/each}
      </span>
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
  .value b {
    font-family: var(--font-num);
    font-size: 2.2rem;
    font-weight: 650;
    line-height: 1.05;
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
  .line {
    margin-top: 0.5rem;
    font-size: 0.78rem;
  }
  .line > span {
    white-space: nowrap;
  }
  .line i {
    margin: 0 0.4rem;
    color: var(--muted);
    font-style: normal;
  }
  .line b {
    font-family: var(--font-num);
  }
</style>
