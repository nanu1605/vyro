<script lang="ts">
  // The Dashboard widgets most layouts do not have (P24): Graph, History, Game, Health and
  // Events. Loaded with the first one shown.
  import { untrack } from 'svelte';
  import Spark from './Spark.svelte';
  import FrameTimeGraph from './FrameTimeGraph.svelte';
  import HistoryChart from './HistoryChart.svelte';
  import { gameShown, readingDef, readingView } from '../dashboard';
  import { fmt } from '../format';
  import { getHealthEvents, getSensorTree, queryHistory } from '../ipc';
  import { displayUnit, prefs, temp, tempText } from '../prefs.svelte';
  import type { HealthEvent, HistorySeries, SensorSnapshot, SensorTree, Status, Widget } from '../types';

  interface Props {
    widget: Widget;
    snap: SensorSnapshot | null;
    /** The Dashboard shows: History and Events read only then. */
    active: boolean;
    needs: string;
    editing: boolean;
    onselect?: (tab: 'gaming' | 'health') => void;
  }
  let { widget, snap, active, needs, editing, onselect }: Props = $props();

  /** Approximate row height, for drawings that fill their rows. */
  const ROW_REM = 5;
  const plotRem = $derived(Math.max(2.5, widget.h * ROW_REM + (widget.h - 1) * 0.75 - 3.2));

  const reading = $derived(widget.kind === 'graph' ? readingDef(widget.source ?? '', snap) : null);

  // History: the last hour of a recorded sensor, read when shown and once a minute.
  let tree = $state.raw<SensorTree | null>(null);
  let series = $state.raw<HistorySeries | null>(null);
  let historyError = $state<string | null>(null);
  const sensor = $derived(tree?.groups.flatMap((g) => g.sensors).find((s) => s.id === widget.source) ?? null);
  $effect(() => {
    if (widget.kind !== 'history' || !active || !widget.source) return;
    const id = widget.source;
    const load = () =>
      Promise.all([tree ? Promise.resolve(tree) : getSensorTree(snap), queryHistory(id, '1h')])
        .then(([t, s]) => {
          tree = t;
          series = s;
          historyError = null;
        })
        .catch((e: unknown) => (historyError = String(e)));
    // Untracked: the snapshot changes every tick, the history read only once a minute.
    untrack(load);
    const timer = setInterval(load, 60_000);
    return () => clearInterval(timer);
  });
  const historyPoints = $derived.by(() => {
    const pts = series?.points ?? [];
    return sensor?.unit === '°C' && prefs.temperatureUnit === 'f' ? pts.map((p) => ({ ...p, avg: temp(p.avg), min: temp(p.min), max: temp(p.max) })) : pts;
  });
  const historyEnd = $derived(series?.points.at(-1)?.ts ?? Math.floor(Date.now() / 1000));

  // Events: the latest health changes, once a minute while shown.
  let events = $state.raw<HealthEvent[]>([]);
  $effect(() => {
    if (widget.kind !== 'events' || !active) return;
    const load = () =>
      getHealthEvents()
        .then((e) => (events = [...e].reverse().slice(0, 6)))
        .catch(() => {});
    void load();
    const timer = setInterval(load, 60_000);
    return () => clearInterval(timer);
  });

  const STATUS: Record<Status, string> = { ok: 'All good', warn: 'Warning', fail: 'Problem', unknown: 'Not monitored' };
  const problems = $derived(
    (snap?.health.subsystems ?? [])
      .filter((x) => x.status === 'warn' || x.status === 'fail')
      .sort((a, b) => (a.status === b.status ? 0 : a.status === 'fail' ? -1 : 1)),
  );
  const ago = (ts: number) => {
    const m = Math.max(0, Math.round((Date.now() / 1000 - ts) / 60));
    return m < 1 ? 'now' : m < 60 ? `${m} min ago` : m < 1440 ? `${Math.round(m / 60)} h ago` : `${Math.round(m / 1440)} d ago`;
  };
</script>

{#if widget.kind === 'graph'}
  {#if reading}
    {@const v = readingView(reading, snap, needs, prefs.temperatureUnit)}
    <div class="panel box graph" style:--plot="{plotRem}rem">
      <div class="head">
        <span class="label">{reading.label}</span>
        <span class="num">{v.value == null ? '—' : `${fmt(v.value, reading.digits)} ${v.unit}`}</span>
      </div>
      <Spark ts={snap?.ts ?? 0} series={[{ value: v.value, color: 'var(--accent)' }]} label="{reading.label}, last minute" />
    </div>
  {:else}
    <div class="panel box muted">Unknown reading “{widget.source}”.</div>
  {/if}
{:else if widget.kind === 'history'}
  {#if !widget.source}
    <div class="panel box muted">Choose a sensor in the editor.</div>
  {:else if historyError}
    <div class="panel box muted">History is not available: {historyError}</div>
  {:else if tree && !sensor}
    <div class="panel box muted">This PC has no sensor “{widget.source}”.</div>
  {:else}
    <div class="history">
      <HistoryChart
        title={sensor?.name ?? widget.source}
        unit={displayUnit(sensor?.unit ?? '')}
        zero={sensor?.unit !== '°C'}
        series={[{ name: sensor?.name ?? widget.source, color: 'var(--accent)', points: historyPoints }]}
        start={historyEnd - 3600}
        end={historyEnd}
        step={series?.stepSecs ?? 1}
        height={Math.round(plotRem * 16 - 24)}
        emptyText="Nothing recorded in the last hour yet."
      />
    </div>
  {/if}
{:else if widget.kind === 'game'}
  {@const g = snap?.game}
  <div class="panel box game" style:--plot="{plotRem}rem">
    {#if g && gameShown(snap)}
      <div class="head">
        <span class="label">{g.app}</span>
        <span class="muted small">{[g.runtime, g.presentMode].filter(Boolean).join(' · ')}</span>
      </div>
      <div class="nums">
        <span><b>{fmt(g.fps)}</b> FPS</span>
        <span>1% low <b>{fmt(g.low1Fps)}</b></span>
        <span>frame <b>{fmt(g.frameMs, 1)}</b> ms</span>
      </div>
      {#if widget.h > 1}<FrameTimeGraph points={g.timeline} height={Math.round(plotRem * 16 - 40)} />{/if}
      {#if !editing}<button type="button" class="link" onclick={() => onselect?.('gaming')}>Gaming →</button>{/if}
    {:else}
      <span class="label">Game</span>
      <p class="muted">No game is being measured. FPS and frame times show here while one runs.</p>
    {/if}
  </div>
{:else if widget.kind === 'health'}
  {@const overall = snap?.health.overall ?? 'unknown'}
  <div class="panel box health">
    <div class="head">
      <span class="label">Health</span>
      <span class="grade {overall}"><span class="dot" aria-hidden="true"></span>{STATUS[overall]}</span>
    </div>
    {#if problems.length}
      <ul>
        {#each problems.slice(0, Math.max(1, widget.h * 2 - 1)) as p (p.name)}
          <li><b>{p.name}</b> {p.messages.length ? tempText(p.messages[0]!) : STATUS[p.status]}</li>
        {/each}
      </ul>
    {:else}
      <p class="muted">{snap ? 'Nothing needs attention.' : 'Waiting for the first reading…'}</p>
    {/if}
    {#if !editing}<button type="button" class="link" onclick={() => onselect?.('health')}>Health →</button>{/if}
  </div>
{:else if widget.kind === 'events'}
  <div class="panel box events">
    <span class="label">Health events</span>
    {#if events.length}
      <ul>
        {#each events.slice(0, Math.max(2, widget.h * 2)) as e (e.ts + e.message)}
          <li>
            <span class="dot {e.status}" aria-hidden="true"></span>
            <span class="what"><b>{e.subsystem}</b> {tempText(e.message)}</span>
            <span class="muted small">{ago(e.ts)}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted">No health changes recorded yet.</p>
    {/if}
  </div>
{/if}

<style>
  .box {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    min-width: 0;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.75rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.25rem 0.75rem;
  }
  .label {
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .num {
    font-family: var(--font-num);
    font-size: 1.1rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .graph :global(.spark) {
    height: var(--plot);
  }
  .history {
    display: grid;
  }
  .nums {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1.2rem;
    font-variant-numeric: tabular-nums;
  }
  .nums b {
    font-family: var(--font-num);
    font-size: 1.35rem;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.85rem;
  }
  li {
    padding: 0.2rem 0;
  }
  .events li {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 0.5rem;
    border-top: 1px solid var(--line);
  }
  .what {
    overflow-wrap: anywhere;
  }
  .dot {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok,
  .grade.ok .dot {
    background: var(--ok);
  }
  .dot.warn,
  .grade.warn .dot {
    background: var(--warn);
  }
  .dot.fail,
  .grade.fail .dot {
    background: var(--hot);
  }
  .grade {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 600;
  }
  .grade.warn {
    color: var(--warn);
  }
  .grade.fail {
    color: var(--hot);
  }
  .grade.ok {
    color: var(--ok);
  }
  p {
    margin: 0;
    font-size: 0.85rem;
  }
  .link {
    align-self: flex-start;
    margin-top: auto;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
</style>
