<script lang="ts">
  // The PC's Health, Performance Score and alerts (1.5), as the PC's own views show them. A phone
  // allowed to control the PC can turn alert rules on and off.
  import { fmt, fmtDateTime } from '../../format';
  import type { AlertFired, AlertRule, HealthReport, ScoreReport } from '../../types';
  import { errText, every, type ScreenProps } from './ask';

  let { ask, can, pc }: ScreenProps = $props();

  let health = $state.raw<HealthReport | null>(null);
  let score = $state.raw<ScoreReport | null>(null);
  let alerts = $state.raw<{ rules: AlertRule[]; log: AlertFired[] } | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  function load(): Promise<unknown> {
    return Promise.all([
      ask<HealthReport>('health'),
      ask<ScoreReport>('score').catch(() => null),
      ask<{ rules: AlertRule[]; log: AlertFired[] }>('alerts').catch(() => null),
    ])
      .then(([h, s, a]) => {
        health = h;
        // One answer failed (a slow link): what it said last stays.
        score = s ?? score;
        alerts = a ?? alerts;
        error = null;
      })
      .catch((e: unknown) => (error = errText(e)));
  }
  $effect(() => every(15_000, load));

  const WORD: Record<string, string> = { ok: 'OK', warn: 'Warning', fail: 'Problem', unknown: 'Not known' };
  const recent = $derived(alerts ? [...alerts.log].reverse().slice(0, 10) : []);

  async function toggle(r: AlertRule): Promise<void> {
    busy = true;
    try {
      await ask('alerts.enable', { id: r.id, on: !r.enabled });
      void load();
    } catch (e) {
      error = errText(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel rise" aria-labelledby="health-title">
  <h1 id="health-title">Health of {pc}</h1>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if !health}
    <div class="skeleton" role="status" aria-label="Reading…"><span></span><span></span><span></span></div>
  {:else}
    <div class="hero {health.overall}">
      <span class="ring" aria-hidden="true"></span>
      <div>
        <p class="word">{WORD[health.overall] ?? health.overall}</p>
        {#if score?.latest.score != null}<p class="muted small">Performance Score {fmt(score.latest.score)}</p>{/if}
      </div>
    </div>
    <ul class="rows">
      {#each health.subsystems as s (s.name)}
        <li>
          <span class="dot {s.status}" aria-hidden="true"></span>
          <div>
            <b>{s.name}</b> <span class="muted small">{WORD[s.status] ?? s.status}</span>
            {#each s.messages as m, i (i)}<p class="small">{m}</p>{/each}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if alerts}
  <section class="panel rise" style:--i={1} aria-labelledby="alerts-title">
    <h2 id="alerts-title">Alerts</h2>
    {#if recent.length}
      <ul class="rows">
        {#each recent as a, i (i)}
          <li><div><b>{a.title}</b> <span class="muted small">{fmtDateTime(a.ts)}</span><p class="small">{a.message}</p></div></li>
        {/each}
      </ul>
    {:else}
      <p class="muted small">No alert has fired lately.</p>
    {/if}
    {#if can('alerts.enable') && alerts.rules.length}
      <h2 id="rules-title">Rules</h2>
      <ul class="rows" aria-labelledby="rules-title">
        {#each alerts.rules as r (r.id)}
          <li class="rule">
            <span>{r.name}</span>
            <button type="button" class="ghost" disabled={busy} aria-pressed={r.enabled} onclick={() => void toggle(r)}>
              {r.enabled ? 'On' : 'Off'}<span class="sr-only">: {r.name}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  h2 {
    margin: 0.9rem 0 0.25rem;
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 0.9rem;
    margin: 0.25rem 0 0.75rem;
    padding: 0.85rem;
    border-radius: 14px;
    background: color-mix(in srgb, var(--muted) 10%, transparent);
  }
  .hero p {
    margin: 0;
  }
  .word {
    font-size: 1.35rem;
    font-weight: 700;
  }
  .ring {
    flex: none;
    width: 2.6rem;
    height: 2.6rem;
    border: 5px solid var(--muted);
    border-radius: 50%;
  }
  .hero.ok {
    background: color-mix(in srgb, var(--ok) 12%, transparent);
  }
  .hero.ok .ring {
    border-color: var(--ok);
  }
  .hero.warn {
    background: color-mix(in srgb, var(--warn) 12%, transparent);
  }
  .hero.warn .ring {
    border-color: var(--warn);
  }
  .hero.fail {
    background: color-mix(in srgb, var(--hot) 12%, transparent);
  }
  .hero.fail .ring {
    border-color: var(--hot);
  }
  .rows li {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
  }
  .rows p {
    margin: 0.15rem 0 0;
  }
  .rule {
    align-items: center !important;
    justify-content: space-between;
  }
  .rule button {
    flex: none;
    min-width: 4rem;
    padding: 0.3rem 0.8rem;
    font-size: 0.85rem;
  }
  .dot {
    flex: none;
    width: 0.65rem;
    height: 0.65rem;
    margin-top: 0.35rem;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.fail {
    background: var(--hot);
  }
</style>
