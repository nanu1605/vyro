<script lang="ts">
  import { getPerformanceScore } from '../ipc';
  import { BANDS, INPUTS, coverageText, fmtPart, trendPath, trendText } from '../score';
  import { prefs } from '../prefs.svelte';
  import type { ScoreReport } from '../types';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  // Worked out from the past week; once a minute is plenty while Insights shows.
  let report = $state.raw<ScoreReport | null>(null);
  let error = $state<string | null>(null);
  $effect(() => {
    if (!active) return;
    const load = () =>
      getPerformanceScore()
        .then((r) => {
          report = r;
          error = null;
        })
        .catch((e: unknown) => (error = String(e)));
    void load();
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  });

  const s = $derived(report?.latest ?? null);
  const band = $derived(s?.band ? BANDS[s.band] : null);
  const unit = $derived(prefs.temperatureUnit);
  const W = 240;
  const H = 48;
  const path = $derived(report ? trendPath(report.history, W, H) : '');
  const trend = $derived(report && s ? trendText(report.history, s.ts) : null);
  const round = (v: number) => Math.round(v);
</script>

<section class="score tone-{band?.tone ?? 'unknown'}" aria-labelledby="score-title">
  <div class="head">
    <div class="number">
      <h2 id="score-title">Performance Score</h2>
      {#if error}
        <p class="fine" role="alert">Score unavailable: {error}</p>
      {:else if !s}
        <p class="fine" role="status">Working out the score…</p>
      {:else if s.score != null}
        <p class="value"><b>{s.score}</b><span class="of">/100</span><span class="band">{band?.label}</span></p>
      {:else}
        <p class="value none">Not enough data yet</p>
      {/if}
      {#if s}<p class="fine">{coverageText(s)} · formula {s.formula}</p>{/if}
    </div>
    {#if path}
      <figure class="trend">
        <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none" role="img" aria-label="Daily score over the past year{trend ? `: ${trend.toLowerCase()}` : ''}">
          <line x1="0" x2={W} y1={H * 0.25} y2={H * 0.25} class="grid" />
          <line x1="0" x2={W} y1={H * 0.5} y2={H * 0.5} class="grid" />
          <path d={path} />
        </svg>
        <figcaption>{trend ?? 'Daily score, past year'}</figcaption>
      </figure>
    {/if}
  </div>
  <p class="fine about">
    How well this PC runs against its own limits and its own history over the past week, not how fast its hardware is.
  </p>
  {#if s}
    <details open>
      <summary>What it is made of</summary>
      <ul class="inputs">
        {#each s.inputs as i (i.id)}
          <li class:missing={i.score == null}>
            <div class="name">
              <b>{INPUTS[i.id].label}</b>
              <small>{INPUTS[i.id].hint}</small>
            </div>
            <div class="bar" aria-hidden="true"><span style="width: {i.score ?? 0}%"></span></div>
            <span class="sub">{#if i.score != null}{round(i.score)}<span class="sr-only"> out of 100</span>{:else}—<span class="sr-only"> no data</span>{/if}</span>
            <span class="weight">weight {i.weight}</span>
            <span class="basis {i.basis}"><span class="mark" aria-hidden="true">{i.basis === 'measured' ? '●' : '◌'}</span>{i.basis === 'measured' ? 'Measured' : 'Inferred'}</span>
            <p class="parts">
              {#if i.missing}
                {i.missing}
              {:else}
                {#each i.parts as p, j (j)}<span>{p.label}: <b>{fmtPart(p, unit)}</b> → {round(p.score)}</span>{/each}
              {/if}
            </p>
          </li>
        {/each}
      </ul>
      <p class="fine">
        Each input scores 0–100 on fixed breakpoints; the score is their weighted average over the inputs with data (none
        below 60% of the weight). Stored once a day for a year; a new formula version keeps older scores as they were.
      </p>
    </details>
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
  .score {
    margin-bottom: 1.2rem;
    padding: 0.85rem 1rem;
    border: 1px solid var(--line);
    border-left: 3px solid var(--tone);
    border-radius: 10px;
    background: var(--panel);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 0.6rem 1.5rem;
  }
  h2 {
    margin: 0;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .value {
    display: flex;
    align-items: baseline;
    gap: 0.35rem;
    margin: 0.2rem 0 0;
  }
  .value b {
    font-family: var(--font-num);
    font-size: 2.4rem;
    line-height: 1;
  }
  .value .of {
    color: var(--muted);
    font-size: 0.9rem;
  }
  .value .band {
    margin-left: 0.5rem;
    padding: 0.05rem 0.5rem;
    border: 1px solid var(--tone);
    border-radius: 999px;
    font-size: 0.8rem;
    font-weight: 600;
  }
  .value.none {
    font-size: 1.05rem;
    font-weight: 600;
  }
  .fine {
    margin: 0.3rem 0 0;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .about {
    margin-top: 0.6rem;
  }
  .trend {
    flex: 0 1 15rem;
    min-width: 0;
    margin: 0;
  }
  .trend svg {
    display: block;
    width: 100%;
    height: 3rem;
    overflow: visible;
  }
  .trend path {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
  }
  .trend .grid {
    stroke: var(--line);
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
  figcaption {
    margin-top: 0.2rem;
    color: var(--muted);
    font-size: 0.74rem;
    text-align: right;
  }
  details {
    margin-top: 0.6rem;
  }
  summary {
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .inputs {
    display: grid;
    gap: 0.1rem;
    margin: 0.5rem 0 0;
    padding: 0;
    list-style: none;
  }
  .inputs li {
    display: grid;
    grid-template-columns: minmax(9rem, 1.3fr) minmax(min(100%, 4rem), 1fr) 2.2rem 4.4rem 5.8rem;
    align-items: center;
    gap: 0.15rem 0.7rem;
    padding: 0.4rem 0;
    border-top: 1px solid var(--line);
    font-size: 0.82rem;
  }
  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name small {
    color: var(--muted);
    font-size: 0.72rem;
  }
  .bar {
    height: 0.45rem;
    border-radius: 999px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--tone);
  }
  .sub {
    font-family: var(--font-num);
    font-weight: 600;
    text-align: right;
  }
  .weight {
    color: var(--muted);
    font-size: 0.74rem;
  }
  .basis {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    justify-self: start;
    padding: 0.02rem 0.45rem;
    border-radius: 999px;
    color: var(--text);
    font-size: 0.7rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .basis.measured {
    border: 1px solid var(--muted);
    background: color-mix(in srgb, var(--text) 14%, transparent);
  }
  .basis.inferred {
    border: 1px dashed var(--muted);
    font-style: italic;
  }
  .mark {
    font-style: normal;
    font-size: 0.65rem;
  }
  .parts {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 1.1rem;
    margin: 0;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .parts b {
    color: var(--text);
    font-family: var(--font-num);
    font-weight: 500;
  }
  li.missing .name b {
    color: var(--muted);
  }
  @media (max-width: 560px) {
    .inputs li {
      grid-template-columns: 1fr auto auto auto;
    }
    .name {
      grid-column: 1 / -1;
    }
  }
</style>
