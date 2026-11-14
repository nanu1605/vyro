<script lang="ts">
  // What a Free PC shows in place of a Pro view or Settings section (2.5): what it is, what Pro
  // costs, and the ways to it. Buy Pro opens the checkout in the browser; once paid, Pro turns on
  // by itself. A key bought before goes in at Settings → Clokrix Pro.
  import Lock from '@lucide/svelte/icons/lock';
  import { buyPro } from '../ipc';
  import { edition } from '../edition.svelte';

  interface Props {
    /** What is locked: a view's name or a section's. */
    title: string;
    /** What it does, in a few words. */
    blurb?: string;
    /** Smaller, inside a Settings section. */
    compact?: boolean;
    /** Opens Settings → Clokrix Pro, where a key goes in. */
    onkey: () => void;
  }
  let { title, blurb, compact = false, onkey }: Props = $props();

  const e = $derived(edition.e);
  let error = $state<string | null>(null);
  async function buy(): Promise<void> {
    try {
      edition.e = await buyPro();
      error = null;
    } catch (x) {
      error = String(x);
    }
  }
</script>

<div class="pro" class:compact>
  {#if !compact}
    <span class="badge" aria-hidden="true"><Lock size={22} /></span>
    <h2>{title} is part of Clokrix Pro</h2>
    {#if blurb}<p class="blurb">{blurb}.</p>{/if}
    <p>
      Clokrix Free keeps the Dashboard. Pro opens every view: Health and alerts, fan control and GPU tuning, History,
      Insights and the Benchmark, every part of this PC, desktop widgets, the game overlay and the phone app.
    </p>
  {:else}
    <p class="lock"><Lock size={15} aria-hidden="true" /> Part of Clokrix Pro.</p>
  {/if}
  <p class="price"><b>{e?.price ?? '$100'}</b> once, for life, on this PC.</p>
  {#if e?.buying}
    <p class="status" role="status">Waiting for your payment in the browser. Pro turns on by itself once it is paid.</p>
  {/if}
  <div class="actions">
    <button type="button" class="primary" disabled={!e?.canBuy || e?.buying} onclick={() => void buy()}>Buy Pro</button>
    <button type="button" onclick={onkey}>Enter a key</button>
  </div>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</div>

<style>
  .pro {
    display: grid;
    justify-items: center;
    gap: 0.2rem;
    max-width: 34rem;
    margin: 3rem auto;
    padding: 2rem 1.5rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    text-align: center;
  }
  .pro.compact {
    justify-items: start;
    max-width: none;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }
  .badge {
    display: grid;
    width: 3rem;
    height: 3rem;
    margin-bottom: 0.5rem;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
  }
  h2 {
    margin: 0 0 0.3rem;
    font-size: 1.15rem;
  }
  p {
    margin: 0 0 0.5rem;
    font-size: 0.86rem;
    line-height: 1.45;
  }
  .blurb,
  .status {
    color: var(--muted);
  }
  .lock {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-weight: 600;
  }
  .price b {
    font-size: 1.1rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0.5rem;
    margin-top: 0.3rem;
  }
  .compact .actions {
    justify-content: flex-start;
  }
  button {
    padding: 0.45rem 1rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .primary {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
    font-weight: 600;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .err {
    margin-top: 0.5rem;
    color: var(--hot);
  }
</style>
