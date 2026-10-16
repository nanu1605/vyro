<script lang="ts">
  import { getInsights } from '../ipc';
  import ScoreCard from './ScoreCard.svelte';
  import {
    BASIS,
    BASIS_HINT,
    CONFIDENCE,
    ROLE,
    VITALS,
    findingTime,
    fmtEvidence,
    fmtVital,
    thinHistory,
    vitalTone,
  } from '../insights';
  import { prefs, tempText } from '../prefs.svelte';
  import type { Basis, Cause, Insights } from '../types';

  interface Props {
    active: boolean;
    /** Findings worth a look that this visit shows for the first time. */
    fresh?: string[];
    /** Opens a sensor in History at a moment. */
    onhistory?: (id: string, at: number | null) => void;
  }
  let { active, fresh = [], onhistory }: Props = $props();

  // Findings compare the past week with the last hours; once a minute is plenty.
  let data = $state.raw<Insights | null>(null);
  let error = $state<string | null>(null);
  $effect(() => {
    if (!active) return;
    const load = () =>
      getInsights()
        .then((i) => {
          data = i;
          error = null;
        })
        .catch((e: unknown) => (error = String(e)));
    void load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  });

  const warn = $derived(data?.findings.filter((f) => f.status === 'warn') ?? []);
  const rest = $derived(data?.findings.filter((f) => f.status !== 'warn') ?? []);
  const unit = $derived(prefs.temperatureUnit);
  const thin = $derived(data ? thinHistory(data.historySecs) : null);

  function when(ts: number): string {
    const d = new Date(ts * 1000);
    const today = new Date().toDateString() === d.toDateString();
    return d.toLocaleString(
      undefined,
      today ? { hour: '2-digit', minute: '2-digit' } : { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' },
    );
  }
</script>

{#snippet badge(b: Basis)}
  <span class="basis {b}" title={BASIS_HINT[b]}><span class="mark" aria-hidden="true">{b === 'measured' ? '●' : '◌'}</span>{BASIS[b]}</span>
{/snippet}

{#snippet cause(c: Cause, nested: boolean)}
  <li class="cause {c.role}">
    <p class="tags">
      {#if nested}<span class="by">because</span>{/if}
      <!-- Related items sit under their own heading, which already says so. -->
      {#if c.role !== 'related'}<span class="role">{ROLE[c.role]}</span>{/if}
      {@render badge(c.basis)}
      <span class="conf">{CONFIDENCE[c.confidence]}</span>
    </p>
    <p class="text">
      {tempText(c.text)}{#if c.ts != null}<time class="at" datetime={new Date(c.ts * 1000).toISOString()}>{when(c.ts)}</time>{/if}
    </p>
    {#if c.because.length}
      <ul class="because">
        {#each c.because as b, i (i)}{@render cause(b, true)}{/each}
      </ul>
    {/if}
  </li>
{/snippet}

<section class="panel">
  <ScoreCard {active} />
  <div class="top">
    <p class="summary tone-{warn.length ? 'warn' : data ? 'ok' : 'unknown'}">
      <span class="dot" aria-hidden="true"></span>
      {#if !data}
        Working out what is unusual…
      {:else if warn.length}
        {warn.length} {warn.length === 1 ? 'finding' : 'findings'} worth a look
      {:else}
        Nothing worth a look
      {/if}
    </p>
    <p class="legend">
      {@render badge('measured')} read from the sensors or the logs · {@render badge('inferred')} worked out from
      measurements
    </p>
  </div>

  {#if error}
    <p class="fine" role="alert">Insights unavailable: {error}</p>
  {:else if !data}
    <p class="fine" role="status">Looking through the history…</p>
  {:else}
    <h2 class="section">Vital signs</h2>
    <ul class="vitals">
      {#each data.vitals as v (v.kind)}
        {@const x = fmtVital(v, unit)}
        <li class="vital tone-{vitalTone(v)}">
          <span class="label">{VITALS[v.kind].label}</span>
          <span class="value">{x.value}</span>
          <span class="usual">
            {#if x.usual}usually {x.usual}{:else if v.kind === 'tempForLoad' && v.value != null}against usual{:else}&nbsp;{/if}
          </span>
          <span class="hint">{tempText(v.detail ?? VITALS[v.kind].hint)}</span>
        </li>
      {/each}
    </ul>
    {#if thin}<p class="fine">{thin}</p>{/if}

    <h2 class="section">Findings</h2>
    {#if data.findings.length === 0}
      <p class="fine">
        Nothing unusual: temperatures, fans, clocks and games match how this PC usually behaves. Vyro compares the last
        hours with the past week and checks again every ten minutes.
      </p>
    {:else}
      <ol class="findings">
        {#each [...warn, ...rest] as f (f.key)}
          <li class="finding tone-{f.status}">
            <div class="head">
              <h3>
                <span class="sr-only">{f.status === 'warn' ? 'Worth a look:' : 'Explanation:'}</span>
                {tempText(f.title)}
                {#if fresh.includes(f.key)}<span class="new">New</span>{/if}
              </h3>
              <span class="when">{findingTime(f, when)}</span>
            </div>
            <p class="tags">{@render badge(f.basis)}<span class="conf">{CONFIDENCE[f.confidence]}</span></p>
            <p class="detail">{tempText(f.detail)}</p>
            {#if f.evidence.length}
              <ul class="evidence" aria-label="Evidence">
                {#each f.evidence as e, i (i)}
                  {@const x = fmtEvidence(e, unit)}
                  <li>
                    <span class="what">{e.label}</span>
                    <b>{x.value}</b>
                    {#if x.usual}<span class="was">usually {x.usual}</span>{/if}
                    {#if e.metric && onhistory}
                      <button type="button" class="link" onclick={() => onhistory(e.metric!, e.from)}>
                        Show in History<span class="sr-only">: {e.label}</span>
                      </button>
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
            {#if f.causes.length}
              <div class="why">
                <h4>Why</h4>
                <ul class="chain">
                  {#each f.causes as c, i (i)}{@render cause(c, false)}{/each}
                </ul>
              </div>
            {/if}
            {#if f.related.length}
              <div class="why related">
                <h4>Around the same time <span class="fine">(not shown to be the cause)</span></h4>
                <ul class="chain">
                  {#each f.related as c, i (i)}{@render cause(c, false)}{/each}
                </ul>
              </div>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
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
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.4rem 1rem;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    margin: 0;
    font-size: 0.95rem;
    font-weight: 600;
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--tone);
  }
  .legend {
    margin: 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .section {
    margin: 1.2rem 0 0.5rem;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .fine {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .vitals {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 10rem), 1fr));
    gap: 0.6rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .vital {
    display: grid;
    gap: 0.1rem;
    padding: 0.55rem 0.75rem;
    border: 1px solid var(--line);
    border-left: 3px solid var(--tone);
    border-radius: 10px;
    background: var(--panel);
  }
  .vital .label {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .vital .value {
    font-family: var(--font-num);
    font-size: 1.2rem;
    font-weight: 600;
  }
  .vital .usual,
  .vital .hint {
    color: var(--muted);
    font-size: 0.74rem;
    overflow-wrap: anywhere;
  }
  .findings {
    display: grid;
    gap: 0.7rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .finding {
    padding: 0.7rem 0.9rem;
    border: 1px solid var(--line);
    border-left: 3px solid var(--tone);
    border-radius: 10px;
    background: var(--panel);
    font-size: 0.84rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.2rem 0.8rem;
  }
  h3 {
    margin: 0;
    font-size: 0.92rem;
  }
  h4 {
    margin: 0 0 0.3rem;
    font-size: 0.76rem;
    font-weight: 600;
  }
  .when,
  time {
    color: var(--muted);
    font-size: 0.76rem;
    font-variant-numeric: tabular-nums;
  }
  .new {
    margin-left: 0.4rem;
    padding: 0 0.4rem;
    border-radius: 4px;
    background: var(--accent);
    color: var(--bg);
    font-size: 0.68rem;
    font-weight: 700;
    vertical-align: 0.1rem;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
    margin: 0.3rem 0 0;
  }
  .basis {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.02rem 0.45rem;
    border-radius: 999px;
    font-size: 0.7rem;
    font-weight: 600;
    white-space: nowrap;
  }
  /* Measured: solid. Inferred: dashed outline, italic. Both say which in words. */
  .basis.measured {
    border: 1px solid var(--muted);
    background: color-mix(in srgb, var(--text) 14%, transparent);
    color: var(--text);
  }
  .basis.inferred {
    border: 1px dashed var(--muted);
    background: transparent;
    color: var(--text);
    font-style: italic;
  }
  .mark {
    font-style: normal;
    font-size: 0.65rem;
  }
  .conf {
    color: var(--muted);
    font-size: 0.74rem;
  }
  .detail {
    margin: 0.35rem 0 0;
    color: var(--muted);
  }
  .evidence {
    display: grid;
    gap: 0.2rem;
    margin: 0.5rem 0 0;
    padding: 0.45rem 0.6rem;
    border-radius: 8px;
    background: var(--bg);
    list-style: none;
  }
  .evidence li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.15rem 0.5rem;
  }
  .evidence .what {
    color: var(--muted);
  }
  .evidence b {
    font-family: var(--font-num);
  }
  .was {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 0.76rem;
    text-decoration: underline dotted var(--muted);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .why {
    margin-top: 0.6rem;
  }
  .chain,
  .because {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .because {
    margin: 0.35rem 0 0 0.35rem;
    padding-left: 0.8rem;
    border-left: 2px dotted var(--line);
  }
  .cause .tags {
    margin: 0;
  }
  .cause .text {
    margin: 0.1rem 0 0;
    overflow-wrap: anywhere;
  }
  .at {
    margin-left: 0.4rem;
  }
  .at::before {
    content: '· ';
  }
  .role {
    font-size: 0.76rem;
    font-weight: 600;
  }
  .by {
    color: var(--muted);
    font-size: 0.74rem;
    font-style: italic;
  }
  .related .text,
  .related .role {
    color: var(--muted);
  }
  .related h4 .fine {
    font-weight: 400;
  }
</style>
