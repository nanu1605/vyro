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

  function load(): void {
    Promise.all([
      ask<HealthReport>('health'),
      ask<ScoreReport>('score').catch(() => null),
      ask<{ rules: AlertRule[]; log: AlertFired[] }>('alerts').catch(() => null),
    ])
      .then(([h, s, a]) => {
        health = h;
        score = s;
        alerts = a;
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
      load();
    } catch (e) {
      error = errText(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" aria-labelledby="health-title">
  <h1 id="health-title">Health of {pc}</h1>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if !health}
    <p class="muted" role="status">Reading…</p>
  {:else}
    <p><b>{WORD[health.overall] ?? health.overall}</b>{score?.latest.score != null ? ` · Performance Score ${fmt(score.latest.score)}` : ''}</p>
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
  <section class="panel" aria-labelledby="alerts-title">
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
    margin: 0.75rem 0 0.35rem;
  }
  .rows {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .rows li {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.4rem 0;
    border-bottom: 1px solid var(--line);
  }
  .rows p {
    margin: 0.15rem 0 0;
  }
  .rule {
    align-items: center !important;
    justify-content: space-between;
  }
  .rule button {
    padding: 0.3rem 0.8rem;
    font-size: 0.85rem;
  }
  .dot {
    flex: none;
    width: 0.6rem;
    height: 0.6rem;
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
