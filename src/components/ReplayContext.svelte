<script lang="ts">
  // What the PC was doing at one moment of a game session: the marker, the session's own
  // readings then, and a minute either side of the sensor history the engine kept (P15).
  import { untrack } from 'svelte';
  import HistoryChart from './HistoryChart.svelte';
  import { getHistoryWindow } from '../ipc';
  import type { ChartDef } from '../presets';
  import { CONTEXT_SECS, clock, sampleAt, type MarkerInfo } from '../replay';
  import { fmt, NA } from '../format';
  import { displayUnit, fmtTemp, prefs, temp } from '../prefs.svelte';
  import type { GameTimeline, WindowSeries } from '../types';

  interface Props {
    tl: GameTimeline;
    /** The moment, unix seconds. */
    ts: number;
    /** The marker picked, if a marker was. */
    info: MarkerInfo | null;
    charts: ChartDef[];
    onclose: () => void;
  }
  let { tl, ts, info, charts, onclose }: Props = $props();

  const COLORS = ['var(--series-1)', 'var(--series-2)', 'var(--series-3)', 'var(--series-4)'];
  const ids = $derived([...new Set(charts.flatMap((c) => c.metrics.map((m) => m.id)))]);

  let data = $state.raw<Record<string, WindowSeries> | null>(null);
  let error = $state<string | null>(null);
  let seq = 0;
  async function load(at: number, metrics: string[]): Promise<void> {
    const mine = ++seq;
    data = null;
    try {
      const list = metrics.length ? await getHistoryWindow(metrics, at - CONTEXT_SECS, at + CONTEXT_SECS) : [];
      if (mine !== seq) return;
      data = Object.fromEntries(list.map((s) => [s.metric, s]));
      error = null;
    } catch (e) {
      if (mine === seq) error = String(e);
    }
  }
  $effect(() => {
    const at = ts;
    const metrics = ids;
    untrack(() => void load(at, metrics));
  });

  let heading = $state<HTMLElement | null>(null);
  $effect(() => {
    void ts;
    heading?.focus();
  });

  const sample = $derived(sampleAt(tl, ts));
  const step = $derived(data ? Math.max(0, ...Object.values(data).map((s) => s.stepSecs)) : 0);
  const gone = $derived(data != null && ids.length > 0 && step === 0);
  const resolution = $derived(
    step === 1
      ? 'Every second.'
      : step === 10
        ? '10 s averages: second-by-second history is kept for 30 minutes.'
        : step >= 60
          ? '1 min averages: 10 s averages are kept for a day.'
          : `${step} s averages.`,
  );
  const wall = $derived(
    new Date(ts * 1000).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }),
  );
  const pct = (v: number | null | undefined) => (v == null ? NA : `${fmt(v)}%`);
  const watts = (v: number | null | undefined) => (v == null ? NA : `${fmt(v)} W`);

  const seriesFor = (c: ChartDef) =>
    c.metrics.map((m, i) => {
      const points = data?.[m.id]?.points ?? [];
      return {
        name: m.name,
        color: COLORS[i % COLORS.length]!,
        points:
          c.unit === '°C' && prefs.temperatureUnit === 'f'
            ? points.map((p) => ({ ...p, avg: temp(p.avg), min: temp(p.min), max: temp(p.max) }))
            : points,
      };
    });
</script>

<section class="panel context" aria-labelledby="replay-moment">
  <header>
    <div>
      <h3 id="replay-moment" tabindex="-1" bind:this={heading}>{info?.title ?? 'This moment'}</h3>
      <p class="when">{clock((ts - tl.session.started) * 1000)} into the session · {wall}</p>
    </div>
    <button type="button" class="close" aria-label="Close this moment" onclick={onclose}>×</button>
  </header>
  {#if info}<p class="what">{info.text}</p>{/if}

  <h4>In the game</h4>
  {#if sample}
    <dl class="readings">
      <div><dt>FPS</dt><dd>{fmt(sample.fps)}</dd></div>
      <div><dt>Slowest</dt><dd>{sample.fpsMin != null ? `${fmt(sample.fpsMin)} FPS` : NA}</dd></div>
      <div><dt>Worst frame</dt><dd>{sample.frameMaxMs != null ? `${fmt(sample.frameMaxMs, 1)} ms` : NA}</dd></div>
      <div><dt>CPU</dt><dd>{pct(sample.cpuUsage)} · {fmtTemp(sample.cpuTemp) ?? NA} · {watts(sample.cpuPower)}</dd></div>
      <div><dt>GPU</dt><dd>{pct(sample.gpuUsage)} · {fmtTemp(sample.gpuTemp) ?? NA} · {watts(sample.gpuPower)}</dd></div>
    </dl>
  {:else}
    <p class="note">No readings were recorded for this session.</p>
  {/if}

  <h4>The whole PC, a minute either side</h4>
  {#if error}
    <p class="note" role="alert">Sensor history unavailable: {error}</p>
  {:else if !data}
    <p class="note" role="status">Reading the history…</p>
  {:else if ids.length === 0}
    <p class="note">No history-tracked sensors to show.</p>
  {:else if gone}
    <p class="note">Vyro keeps sensor history for 7 days; this moment is older, so only the game's own readings remain.</p>
  {:else}
    <p class="note">{resolution}</p>
    <div class="charts">
      {#each charts as c (c.key)}
        <HistoryChart
          title={c.title}
          unit={displayUnit(c.unit)}
          zero={c.zero}
          max={c.max}
          series={seriesFor(c)}
          start={ts - CONTEXT_SECS}
          end={ts + CONTEXT_SECS}
          {step}
          height={110}
          markers={[{ ts, color: 'var(--accent)' }]}
          emptyText="Not recorded then."
        />
      {/each}
    </div>
  {/if}
</section>

<style>
  .context {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    min-width: 0;
    padding: 0.8rem 0.9rem;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h3 {
    margin: 0;
    font-size: 0.95rem;
  }
  h3:focus {
    outline: none;
  }
  h3:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  h4 {
    margin: 0.6rem 0 0.1rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .when,
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .what {
    margin: 0;
    font-size: 0.85rem;
  }
  .close {
    padding: 0 0.45rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: none;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .readings {
    display: grid;
    gap: 0.2rem;
    margin: 0;
    font-size: 0.82rem;
  }
  .readings div {
    display: flex;
    justify-content: space-between;
    gap: 0.8rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .charts {
    display: grid;
    gap: 0.5rem;
  }
</style>
