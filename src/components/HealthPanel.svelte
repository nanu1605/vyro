<script lang="ts">
  import AlertsPanel from './AlertsPanel.svelte';
  import { getHealthEvents } from '../ipc';
  import { fmtTemp, tempText } from '../prefs.svelte';
  import type { HealthEvent, SensorSnapshot, Status } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    active: boolean;
    /** Opens the Insights tab. */
    oninsights?: () => void;
  }
  let { snap, active, oninsights }: Props = $props();

  const worth = $derived(snap?.insights?.attention.length ?? 0);
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

  <p class="insights-line">
    <span class="dot tone-{worth ? 'warn' : 'ok'}" aria-hidden="true"></span>
    <span>
      {worth
        ? `Insights: ${worth} ${worth === 1 ? 'finding' : 'findings'} worth a look.`
        : 'Insights: nothing unusual in how this PC behaves compared with its own history.'}
    </span>
    {#if oninsights}<button type="button" class="link" onclick={oninsights}>Open Insights</button>{/if}
  </p>

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
  .insights-line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin: 1.1rem 0 0;
    padding: 0.55rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    font-size: 0.82rem;
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
