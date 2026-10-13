<script lang="ts">
  import AlertsPanel from './AlertsPanel.svelte';
  import { getHealthEvents, getInsights } from '../ipc';
  import { fmtTemp, tempText } from '../prefs.svelte';
  import type { HealthEvent, Insight, SensorSnapshot, Status } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    active: boolean;
    /** Opens a sensor in History. */
    onhistory?: (id: string) => void;
  }
  let { snap, active, onhistory }: Props = $props();

  // Insights compare the past week with the last hours; once a minute is plenty.
  let insights = $state.raw<Insight[] | null>(null);
  let insightError = $state<string | null>(null);
  $effect(() => {
    if (!active) return;
    const load = () =>
      getInsights()
        .then((i) => {
          insights = i;
          insightError = null;
        })
        .catch((e: unknown) => (insightError = String(e)));
    void load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  });
  const health = $derived(snap?.health ?? null);
  const ts = $derived(snap?.ts ?? 0);

  let events = $state.raw<HealthEvent[]>([]);
  let busy = false;

  $effect(() => {
    if (!active || !ts || busy) return;
    busy = true;
    getHealthEvents()
      .then((e) => (events = e))
      .catch(() => {})
      .finally(() => (busy = false));
  });

  const LABEL: Record<Status, string> = { ok: 'OK', warn: 'Warning', fail: 'Problem', unknown: 'Not monitored' };
  const newest = $derived([...events].reverse());
  const when = (t: number) => {
    const d = new Date(t * 1000);
    const today = new Date().toDateString() === d.toDateString();
    return d.toLocaleString('en-US', {
      weekday: today ? undefined : 'short',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  };
  const summary = $derived.by(() => {
    const subs = health?.subsystems ?? [];
    const bad = subs.filter((s) => s.status === 'warn' || s.status === 'fail');
    if (!subs.length) return 'Waiting for data…';
    if (!bad.length) return 'Everything Vyro monitors is within normal limits.';
    return `${bad.length} ${bad.length === 1 ? 'area needs' : 'areas need'} attention: ${bad.map((s) => s.name).join(', ')}.`;
  });
  /** "85 °C / 92 °C" in the chosen unit. */
  const lim = (warn: number, problem: number): string => `${fmtTemp(warn)} / ${fmtTemp(problem)}`;
</script>

<section class="panel">
  <p class="summary tone-{health?.overall ?? 'unknown'}">
    <span class="dot" aria-hidden="true"></span>{summary}
  </p>

  <div class="grid">
    {#each health?.subsystems ?? [] as s (s.name)}
      <article class="sub tone-{s.status}">
        <header>
          <h2>{s.name}</h2>
          <span class="status">{LABEL[s.status]}</span>
        </header>
        {#if s.messages.length}
          <ul>
            {#each s.messages as m (m)}<li>{tempText(m)}</li>{/each}
          </ul>
        {:else if s.status === 'ok'}
          <p class="fine">Within normal limits.</p>
        {/if}
        {#each s.notes as n (n)}<p class="fine">{n}</p>{/each}
      </article>
    {/each}
  </div>

  <h2 class="events-title">Insights</h2>
  {#if insightError}
    <p class="fine">Insights unavailable: {insightError}</p>
  {:else if insights == null}
    <p class="fine">Looking through the history…</p>
  {:else if insights.length === 0}
    <p class="fine">
      Nothing unusual: temperatures, clocks and fans match how this PC usually behaves. Insights compare the last
      hours with the past week, so they need a few days of history to work well.
    </p>
  {:else}
    <ul class="insights">
      {#each insights as ins, i (i)}
        <li class="tone-{ins.status}">
          <span class="dot" aria-hidden="true"></span>
          <div>
            <span class="sr-only">{LABEL[ins.status]}:</span>
            <b>{tempText(ins.title)}</b>{#if ins.ts != null}<time>{when(ins.ts)}</time>{/if}
            <p>{tempText(ins.detail)}</p>
            {#if onhistory && ins.metrics[0]}
              <button type="button" class="link" onclick={() => onhistory(ins.metrics[0]!)}>Show in History</button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <h2 class="events-title">Recent events</h2>
  {#if newest.length}
    <ol class="events">
      {#each newest as e, i (i)}
        <li class="tone-{e.status}">
          <time>{when(e.ts)}</time>
          <span class="dot" aria-hidden="true"></span>
          <span class="what"><span class="sr-only">{LABEL[e.status]}:</span> <b>{e.subsystem}</b> {tempText(e.message)}</span>
        </li>
      {/each}
    </ol>
  {:else}
    <p class="fine">No thermal limits, throttling, stopped fans or rail problems since Vyro started.</p>
  {/if}
  <p class="note">
    Limits (warning / problem): CPU {lim(90, 99)} (Ryzen 7000/9000 run at {fmtTemp(95)} under full load by design), GPU
    {lim(85, 92)}, hotspot {lim(100, 108)}, NVMe {lim(75, 85)}, VRM {lim(95, 105)}, DIMM {lim(75, 85)}, supply rails ±5 % / ±10 %. A fan that was spinning and reads 0 RPM counts as stopped.
  </p>

  <AlertsPanel {snap} {active} />
</section>

<style>
  .tone-ok {
    --tone: var(--ok);
  }
  .tone-warn {
    --tone: var(--warn);
  }
  .tone-fail {
    --tone: var(--hot);
  }
  .tone-unknown {
    --tone: var(--muted);
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--tone);
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    margin: 0 0 0.9rem;
    font-size: 0.95rem;
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 15rem), 1fr));
    gap: 0.7rem;
  }
  .sub {
    padding: 0.65rem 0.8rem;
    border: 1px solid var(--line);
    border-left: 3px solid var(--tone);
    border-radius: 10px;
    background: var(--panel);
  }
  .sub header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 0.88rem;
  }
  .status {
    color: var(--tone);
    font-size: 0.75rem;
    font-weight: 600;
  }
  ul {
    margin: 0.4rem 0 0;
    padding-left: 1rem;
    font-size: 0.8rem;
  }
  li + li {
    margin-top: 0.2rem;
  }
  .fine {
    margin: 0.35rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .insights {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .insights li {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    margin: 0;
    padding: 0.55rem 0.8rem;
    font-size: 0.82rem;
  }
  .insights li + li {
    border-top: 1px solid var(--line);
  }
  .insights .dot {
    margin-top: 0.35rem;
  }
  .insights time {
    min-width: 0;
    margin-left: 0.6rem;
    font-size: 0.75rem;
  }
  .insights p {
    margin: 0.2rem 0 0.25rem;
    color: var(--muted);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    text-decoration: underline dotted var(--muted);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .events-title {
    margin: 1.3rem 0 0.5rem;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .events {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .events li {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    margin: 0;
    padding: 0.4rem 0.8rem;
    font-size: 0.8rem;
  }
  .events li + li {
    border-top: 1px solid var(--line);
  }
  .events .dot {
    align-self: center;
  }
  time {
    flex: none;
    min-width: 5.5rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .what b {
    margin-right: 0.3rem;
  }
  .note {
    margin: 0.9rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>
