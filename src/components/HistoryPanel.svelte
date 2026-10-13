<script lang="ts">
  import { untrack } from 'svelte';
  import HistoryChart from './HistoryChart.svelte';
  import { getSensorTree, queryHistory } from '../ipc';
  import { chartForSensor, chartsFor, type ChartDef } from '../presets';
  import { displayUnit, prefs, temp } from '../prefs.svelte';
  import type { HistoryRange, HistorySeries, SensorSnapshot, SensorTree } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    active: boolean;
    /** Sensor opened from the explorer; charted on its own at the top. */
    focus: string | null;
    onclearfocus: () => void;
  }
  let { snap, active, focus, onclearfocus }: Props = $props();

  const RANGES: Array<{ id: HistoryRange; label: string; secs: number }> = [
    { id: '5m', label: '5 min', secs: 300 },
    { id: '30m', label: '30 min', secs: 1800 },
    { id: '1h', label: '1 h', secs: 3600 },
    { id: '6h', label: '6 h', secs: 21600 },
    { id: '24h', label: '24 h', secs: 86400 },
    { id: '7d', label: '7 days', secs: 604800 },
  ];
  const COLORS = ['var(--series-1)', 'var(--series-2)', 'var(--series-3)', 'var(--series-4)', 'var(--series-5)', 'var(--series-6)'];

  function savedRange(): HistoryRange {
    try {
      const r = localStorage.getItem('vyro.historyRange');
      return RANGES.some((x) => x.id === r) ? (r as HistoryRange) : '30m';
    } catch {
      return '30m';
    }
  }

  let range = $state<HistoryRange>(savedRange());
  let tree = $state.raw<SensorTree | null>(null);
  let data = $state.raw<Record<string, HistorySeries>>({});
  let end = $state(Math.floor(Date.now() / 1000));
  let error = $state<string | null>(null);
  let loading = $state(false);

  function pick(r: HistoryRange): void {
    range = r;
    try {
      localStorage.setItem('vyro.historyRange', r);
    } catch {
      /* not remembered */
    }
  }

  const charts = $derived.by<ChartDef[]>(() => {
    // Sensors hidden in the explorer stay out of the preset charts (a focused one still shows).
    const hidden = prefs.hiddenSensors;
    const shown = tree && {
      groups: tree.groups.map((g) => ({ ...g, sensors: g.sensors.filter((s) => !hidden.includes(s.id)) })),
    };
    const list = chartsFor(shown);
    const f = focus ? tree?.groups.flatMap((g) => g.sensors).find((s) => s.id === focus) : null;
    return f ? [chartForSensor(f), ...list] : list;
  });
  const ids = $derived([...new Set(charts.flatMap((c) => c.metrics.map((m) => m.id)))]);
  const secs = $derived(RANGES.find((r) => r.id === range)!.secs);

  async function loadTree(): Promise<void> {
    try {
      tree = await getSensorTree(snap);
    } catch (e) {
      error = String(e);
    }
  }

  let seq = 0;
  async function load(r: HistoryRange, metrics: string[]): Promise<void> {
    const mine = ++seq;
    loading = true;
    try {
      const series = await Promise.all(metrics.map((m) => queryHistory(m, r)));
      if (mine !== seq) return;
      data = Object.fromEntries(series.map((s) => [s.metric, s]));
      end = Math.floor(Date.now() / 1000);
      error = null;
    } catch (e) {
      if (mine === seq) error = String(e);
    } finally {
      if (mine === seq) loading = false;
    }
  }

  // Sensor list: on first show, then every 30 s (new drives, adapters, fans appear).
  $effect(() => {
    if (!active) return;
    untrack(() => void loadTree());
    const t = setInterval(loadTree, 30_000);
    return () => clearInterval(t);
  });

  // Series: on range or chart change, then on a cadence that matches the range's resolution.
  $effect(() => {
    if (!active || ids.length === 0) return;
    const r = range;
    const metrics = ids;
    void load(r, metrics);
    const every = r === '5m' ? 2_000 : r === '30m' ? 5_000 : 30_000;
    const t = setInterval(() => void load(r, metrics), every);
    return () => clearInterval(t);
  });

  /** Series in display units (temperatures in °F when chosen; history is stored in °C). */
  const seriesFor = (c: ChartDef) =>
    c.metrics.map((m, i) => {
      const points = data[m.id]?.points ?? [];
      return {
        name: m.name,
        color: COLORS[i % COLORS.length]!,
        points:
          c.unit === '°C' && prefs.temperatureUnit === 'f'
            ? points.map((p) => ({ ...p, avg: temp(p.avg), min: temp(p.min), max: temp(p.max) }))
            : points,
      };
    });
  const step = $derived(Object.values(data)[0]?.stepSecs ?? 1);
</script>

<section class="panel">
  <div class="bar">
    <p class="note">
      Averages per interval with the min–max band. Kept on disk: every second for 30 min, 10 s averages for
      24 h, 1 min averages for 7 days.{loading ? ' Loading…' : ''}
    </p>
    <div class="ranges" role="group" aria-label="Time range">
      {#each RANGES as r (r.id)}
        <button type="button" aria-pressed={range === r.id} onclick={() => pick(r.id)}>{r.label}</button>
      {/each}
    </div>
  </div>

  {#if error}
    <p class="empty" role="alert">History unavailable: {error}. Vyro tries again shortly.</p>
  {:else if tree && charts.length === 0}
    <p class="empty" role="status">
      No history-tracked sensors yet{prefs.hiddenSensors.length ? ' (hidden sensors are left out; see Settings)' : ''}.
    </p>
  {:else if !tree}
    <p class="empty" role="status">Waiting for the first reading…</p>
  {/if}

  <div class="grid">
    {#each charts as c (c.key)}
      <div class:wide={c.key.startsWith('focus:')}>
        <HistoryChart
          title={c.title}
          unit={displayUnit(c.unit)}
          zero={c.zero}
          max={c.max}
          series={seriesFor(c)}
          start={end - secs}
          {end}
          {step}
          onclose={c.key.startsWith('focus:') ? onclearfocus : undefined}
        />
      </div>
    {/each}
  </div>
</section>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    margin-bottom: 0.75rem;
  }
  .note {
    flex: 1 1 22rem;
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .ranges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  .ranges button {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--muted);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .ranges button:hover {
    color: var(--text);
  }
  .ranges button[aria-pressed='true'] {
    border-color: var(--muted);
    background: var(--panel-edge);
    color: var(--text);
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 30rem), 1fr));
    gap: 0.8rem;
  }
  .grid > div {
    display: flex;
    min-width: 0;
  }
  .grid > div > :global(figure) {
    flex: 1;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .empty {
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>
