<script lang="ts">
  // Settings → Clokrix Pro (2.5): what this PC is (Pro, the trial and its days, or Free), buying,
  // a key entered by hand, moving Pro here from another PC, and freeing this one.
  import Card from './Card.svelte';
  import { buyPro, enterKey, releasePro } from '../ipc';
  import { edition, trialDays } from '../edition.svelte';
  import { fmtDate } from '../format';

  const e = $derived(edition.e);
  const days = $derived(trialDays(e));
  const subtitle = $derived(
    !e ? null : e.tier === 'pro' ? 'Pro' : e.tier === 'trial' ? `Trial: ${days} ${days === 1 ? 'day' : 'days'} left` : 'Free',
  );

  let key = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** The key is Pro on another PC: offer to move it here. */
  let elsewhere = $state(false);
  let freeing = $state(false);

  async function run(f: () => Promise<void>): Promise<void> {
    busy = true;
    try {
      await f();
      error = null;
    } catch (x) {
      error = String(x);
    }
    busy = false;
  }
  const buy = () => run(async () => void (edition.e = await buyPro()));
  async function turnOn(moveHere: boolean): Promise<void> {
    await run(async () => {
      edition.e = await enterKey(key, moveHere);
      key = '';
      elsewhere = false;
    });
    if (error && /another PC/.test(error) && !moveHere) elsewhere = true;
  }
  const free = () =>
    run(async () => {
      edition.e = await releasePro();
      freeing = false;
    });
</script>

<Card title="Clokrix Pro" id="pro" {subtitle}>
  {#if !e}
    <p class="note">Finding out…</p>
  {:else if e.tier === 'pro' && !e.key}
    <p>Every part of Clokrix is open in this build.</p>
  {:else}
    {#if e.tier === 'pro'}
      <p>Pro is on for this PC, for life. Key: <code>{e.key}</code></p>
    {:else if e.tier === 'trial'}
      <p>
        Every part of Clokrix is open until {e.trialEnds ? fmtDate(e.trialEnds) : 'the trial ends'}. After that Clokrix Free
        keeps the Dashboard; Pro keeps everything.
      </p>
    {:else}
      <p>Clokrix Free: the Dashboard and these settings. Pro opens every view, the phone app, widgets and the overlay.</p>
    {/if}
    {#if e.note}<p class="note" role="status">{e.note}</p>{/if}

    {#if e.tier !== 'pro'}
      <p class="price"><b>{e.price}</b> once, for life, on one PC; it can move to another.</p>
      {#if e.buying}
        <p class="note" role="status">Waiting for your payment in the browser. Pro turns on by itself once it is paid.</p>
      {/if}
      <div class="actions">
        <button type="button" class="primary" disabled={busy || !e.canBuy || e.buying} onclick={() => void buy()}>Buy Pro</button>
      </div>
      <form
        class="key"
        onsubmit={(ev) => {
          ev.preventDefault();
          void turnOn(false);
        }}
      >
        <label for="pro-key">Already have a key?</label>
        <div class="row">
          <input id="pro-key" bind:value={key} placeholder="CLKX-XXXXX-XXXXX-XXXXX-XXXXX" autocomplete="off" spellcheck="false" />
          <button type="submit" disabled={busy || key.trim().length < 20}>Turn Pro on</button>
        </div>
      </form>
      {#if elsewhere}
        <p class="note">
          Move Pro here if the other PC is gone or Windows was reinstalled. It can move once every 30 days, and the other PC
          turns Free.
        </p>
        <div class="actions">
          <button type="button" disabled={busy} onclick={() => void turnOn(true)}>Move Pro here</button>
        </div>
      {/if}
    {:else if freeing}
      <p>This PC turns Free, and the key can then turn Pro on on another PC.</p>
      <div class="actions">
        <button type="button" disabled={busy} onclick={() => void free()}>Free this PC</button>
        <button type="button" disabled={busy} onclick={() => (freeing = false)}>Cancel</button>
      </div>
    {:else}
      <p class="note">Moving to a new PC? Free this one first, then enter the key there.</p>
      <div class="actions">
        <button type="button" disabled={busy} onclick={() => (freeing = true)}>Free this PC…</button>
      </div>
    {/if}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}
</Card>

<style>
  p {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
  }
  .note {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .price b {
    font-size: 1.05rem;
  }
  code {
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.6rem;
  }
  button {
    padding: 0.35rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
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
  .key {
    display: grid;
    gap: 0.3rem;
    margin-bottom: 0.6rem;
    font-size: 0.82rem;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: 0.82rem ui-monospace, 'Cascadia Mono', Consolas, monospace;
  }
  .err {
    color: var(--hot);
    font-size: 0.82rem;
  }
</style>
