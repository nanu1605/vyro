<script lang="ts">
  // Game session replay (P15): the session's readings on one time axis, its markers, and what
  // the whole PC was doing at any moment of it. Loaded when opened, dropped when closed.
  import { onMount } from 'svelte';
  import HistoryChart from './HistoryChart.svelte';
  import LoadState from './LoadState.svelte';
  import ReplayContext from './ReplayContext.svelte';
  import { getGameTimeline, getSensorTree } from '../ipc';
  import { chartsFor } from '../presets';
  import { GROUPS, clock, contextCharts, markerInfo, points, span, tsOf, type MarkerGroup, type MarkerInfo } from '../replay';
  import { fmt, fmtDateTime, fmtDuration, NA } from '../format';
  import { displayUnit, fmtTemp, profiles, temp } from '../prefs.svelte';
  import type { Bound, GameSample, GameTimeline, Profile, SensorSnapshot, SensorTree } from '../types';

  interface Props {
    id: number;
    snap: SensorSnapshot | null;
    onclose: () => void;
  }
  let { id, snap, onclose }: Props = $props();

  let tl = $state.raw<GameTimeline | null>(null);
  let missing = $state(false);
  let error = $state<string | null>(null);
  let tree = $state.raw<SensorTree | null>(null);

  async function load(): Promise<void> {
    try {
      const t = await getGameTimeline(id);
      tl = t;
      missing = t == null;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }
  onMount(() => {
    void load();
    getSensorTree(snap)
      .then((t) => (tree = t))
      .catch(() => {});
  });

  const LIMITED: Record<Bound, string> = { gpu: 'Graphics card', cpu: 'Processor', capped: 'Capped' };
  const nameOf = (p: Profile) => profiles.list.find((x) => x.id === p)?.name ?? p;
  const color = (g: MarkerGroup) => GROUPS.find((x) => x.id === g)!.color;

  let shown = $state<Record<MarkerGroup, boolean>>({ frames: true, limits: true, you: true });
  let hoverTs = $state<number | null>(null);
  let picked = $state<{ ts: number; info: MarkerInfo | null } | null>(null);

  const axis = $derived(tl ? span(tl) : ([0, 1] as [number, number]));
  const step = $derived(tl ? tl.stepMs / 1000 : 1);
  const marks = $derived(
    tl ? tl.events.map((e) => ({ e, ts: tsOf(tl!, e.t), info: markerInfo(e.marker, nameOf) })) : [],
  );
  const visible = $derived(marks.filter((m) => shown[m.info.group]));
  const lines = $derived(visible.map((m) => ({ ts: m.ts, color: color(m.info.group) })));
  const counts = $derived(
    Object.fromEntries(GROUPS.map((g) => [g.id, marks.filter((m) => m.info.group === g.id).length])) as Record<MarkerGroup, number>,
  );
  const context = $derived(contextCharts(chartsFor(tree)));

  type Pick = (s: GameSample) => number | null;
  const t = (f: Pick): Pick => (s) => {
    const v = f(s);
    return v == null ? null : temp(v);
  };
  const tracks = $derived.by(() => {
    if (!tl) return [];
    const x = tl;
    return [
      {
        key: 'fps', title: 'Frame rate', unit: 'FPS', zero: true, band: true,
        series: [{ name: 'FPS (band: slowest stretch)', color: 'var(--accent-gpu)', points: points(x, (s) => s.fps, (s) => s.fpsMin) }],
      },
      {
        key: 'frame', title: 'Frame time', unit: 'ms', zero: true, band: true,
        series: [{ name: 'Frame time (band: worst frame)', color: 'var(--series-3)', points: points(x, (s) => s.frameMs, undefined, (s) => s.frameMaxMs) }],
      },
      {
        key: 'usage', title: 'Usage', unit: '%', zero: true, max: 100, band: false,
        series: [
          { name: 'CPU', color: 'var(--accent-cpu)', points: points(x, (s) => s.cpuUsage) },
          { name: 'GPU', color: 'var(--accent-gpu)', points: points(x, (s) => s.gpuUsage) },
        ],
      },
      {
        key: 'temps', title: 'Temperatures', unit: displayUnit('°C'), zero: false, band: false,
        series: [
          { name: 'CPU', color: 'var(--accent-cpu)', points: points(x, t((s) => s.cpuTemp)) },
          { name: 'GPU', color: 'var(--accent-gpu)', points: points(x, t((s) => s.gpuTemp)) },
        ],
      },
      {
        key: 'power', title: 'Power', unit: 'W', zero: true, band: false,
        series: [
          { name: 'CPU', color: 'var(--accent-cpu)', points: points(x, (s) => s.cpuPower) },
          { name: 'GPU', color: 'var(--accent-gpu)', points: points(x, (s) => s.gpuPower) },
        ],
      },
    ];
  });

  // Any moment by keyboard: the slider moves the crosshair, "Show" opens it.
  let scrub = $state(0);
  const length = $derived((axis[1] - axis[0]) * 1000);

  function pick(ts: number, info: MarkerInfo | null = null): void {
    picked = { ts, info };
    scrub = Math.round((ts - axis[0]) * 1000);
  }
  const pct = (ts: number) => ((ts - axis[0]) / Math.max(1, axis[1] - axis[0])) * 100;
</script>

<section class="replay panel-wrap" aria-labelledby="replay-title">
  <button type="button" class="back" onclick={onclose}>← Sessions</button>

  {#if error || (!tl && !missing)}
    <LoadState what="the session" {error} loading={!error} retry={load} />
  {:else if missing || !tl}
    <p class="note" role="status">This session's replay is no longer kept.</p>
  {:else}
    {@const s = tl.session}
    <header class="head">
      <h2 id="replay-title">{s.app} <span>replay</span></h2>
      <p class="sub">
        {fmtDateTime(s.started)} · {fmtDuration(s.secs)}{s.runtime ? ` · ${s.runtime}` : ''}{s.profile
          ? ` · ${nameOf(s.profile)} profile`
          : ''}
      </p>
    </header>

    <dl class="summary">
      <div><dt>Average</dt><dd>{fmt(s.avgFps)} <small>FPS</small></dd></div>
      <div><dt>1% low</dt><dd>{fmt(s.low1Fps)} <small>FPS</small></dd></div>
      <div><dt>0.1% low</dt><dd>{fmt(s.low01Fps)} <small>FPS</small></dd></div>
      <div><dt>Limited by</dt><dd>{s.bound ? LIMITED[s.bound] : NA}</dd></div>
      <div><dt>CPU max</dt><dd>{fmtTemp(s.cpuTempMax) ?? NA}</dd></div>
      <div><dt>GPU max</dt><dd>{fmtTemp(s.gpuTempMax) ?? NA}</dd></div>
    </dl>

    <div class="filters" role="group" aria-label="Markers shown">
      {#each GROUPS as g (g.id)}
        <label class="chip">
          <input type="checkbox" bind:checked={shown[g.id]} />
          <span class="dot" style:background={g.color}></span>{g.label} ({counts[g.id]})
        </label>
      {/each}
    </div>

    <div class="layout">
      <div class="main">
        <div class="lane" aria-hidden="true">
          {#each visible as m, i (i)}
            <button
              type="button"
              tabindex="-1"
              class="mark"
              style:left="{pct(m.ts)}%"
              style:background={color(m.info.group)}
              title="{clock(m.e.t)} {m.info.title}"
              aria-label="{m.info.title} at {clock(m.e.t)}"
              onclick={() => pick(m.ts, m.info)}
            ></button>
          {/each}
        </div>
        {#each tracks as c (c.key)}
          <HistoryChart
            title={c.title}
            unit={c.unit}
            zero={c.zero}
            max={c.max}
            series={c.series}
            start={axis[0]}
            end={axis[1]}
            {step}
            band={c.band}
            height={130}
            markers={lines}
            bind:hoverTs
            onpick={(ts) => pick(ts)}
            emptyText="Not recorded for this session."
          />
        {/each}
        <div class="scrub">
          <label for="replay-scrub">Moment</label>
          <input
            id="replay-scrub"
            type="range"
            min="0"
            max={Math.round(length)}
            step={tl.stepMs}
            bind:value={scrub}
            aria-valuetext={clock(scrub)}
            oninput={() => (hoverTs = axis[0] + scrub / 1000)}
          />
          <span class="at">{clock(scrub)}</span>
          <button type="button" onclick={() => pick(axis[0] + scrub / 1000)}>Show</button>
        </div>
      </div>

      <aside>
        {#if picked}
          <ReplayContext {tl} ts={picked.ts} info={picked.info} charts={context} onclose={() => (picked = null)} />
        {:else}
          <p class="hint panel">Pick a marker, or click a chart, to see what the PC was doing at that moment.</p>
        {/if}
      </aside>
    </div>

    <h3 class="section">Markers</h3>
    {#if visible.length === 0}
      <p class="note">
        {marks.length ? 'All markers are filtered out.' : 'Nothing stood out: no drops, stutters or limits were marked.'}
      </p>
    {:else}
      <ol class="events">
        {#each visible as m, i (i)}
          <li>
            <button type="button" onclick={() => pick(m.ts, m.info)}>
              <span class="when">{clock(m.e.t)}</span>
              <span class="dot" style:background={color(m.info.group)}></span>
              <b>{m.info.title}</b>
              <span class="text">{m.info.text}</span>
            </button>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  .back {
    margin-bottom: 0.8rem;
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .head h2 {
    margin: 0;
    font-size: 1.05rem;
  }
  .head h2 span {
    color: var(--muted);
    font-weight: 500;
  }
  .sub,
  .note {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .summary {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 8rem), 1fr));
    gap: 0.5rem;
    margin: 0.8rem 0;
  }
  .summary div {
    padding: 0.5rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: var(--panel);
  }
  .summary dt {
    color: var(--muted);
    font-size: 0.7rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .summary dd {
    margin: 0.1rem 0 0;
    font-family: var(--font-num);
    font-size: 1.15rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .summary small {
    color: var(--muted);
    font-size: 0.65em;
    font-weight: 500;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.6rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 0.8rem;
  }
  @media (min-width: 1100px) {
    .layout {
      grid-template-columns: minmax(0, 1fr) 22rem;
    }
    aside {
      position: sticky;
      top: 0.5rem;
      align-self: start;
      max-height: calc(100vh - 1rem);
      overflow-y: auto;
    }
  }
  @media (max-width: 1099px) {
    aside {
      order: -1;
    }
  }
  .main {
    display: grid;
    gap: 0.5rem;
    min-width: 0;
  }
  /* Lines up with the charts' plot area (figure padding + axis margin). */
  .lane {
    position: relative;
    height: 14px;
    margin: 0 calc(0.8rem + 10px) 0 calc(0.8rem + 46px);
    border-bottom: 1px solid var(--line);
  }
  .lane .mark {
    position: absolute;
    top: 2px;
    width: 9px;
    height: 9px;
    margin-left: -4.5px;
    padding: 0;
    border: 1px solid var(--bg);
    border-radius: 50%;
    cursor: pointer;
  }
  .scrub {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.82rem;
  }
  .scrub input {
    flex: 1 1 12rem;
    min-width: 0;
  }
  .scrub .at {
    min-width: 4.5rem;
    font-variant-numeric: tabular-nums;
  }
  .scrub button {
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .hint {
    margin: 0;
    padding: 0.8rem 0.9rem;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .section {
    margin: 1.2rem 0 0.4rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 500;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .events {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .events button {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.25rem 0.5rem;
    width: 100%;
    padding: 0.4rem 0.5rem;
    border: 0;
    border-top: 1px solid var(--line);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    text-align: left;
    cursor: pointer;
  }
  .events button:hover {
    background: var(--panel);
  }
  .events .when {
    min-width: 4rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .events .text {
    flex: 1 1 16rem;
    color: var(--muted);
  }
</style>
