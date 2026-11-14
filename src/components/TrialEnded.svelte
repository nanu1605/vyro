<script lang="ts">
  // Said once, when the Pro trial has ended (2.5): what stays, what Pro keeps, and the ways to it.
  import { onMount } from 'svelte';
  import { buyPro, editionSeen } from '../ipc';
  import { edition } from '../edition.svelte';

  interface Props {
    /** Opens Settings → Clokrix Pro, where a key goes in. */
    onkey: () => void;
  }
  let { onkey }: Props = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  onMount(() => dialog?.showModal());

  async function done(then?: () => void | Promise<void>): Promise<void> {
    try {
      edition.e = await editionSeen();
    } catch {
      // Shown again at the next start.
    }
    await then?.();
  }
</script>

<dialog bind:this={dialog} class="ended" aria-labelledby="ended-title" oncancel={() => void done()}>
  <h2 id="ended-title">Your Clokrix Pro trial has ended</h2>
  <p>
    Clokrix Free keeps the Dashboard. Pro keeps everything else: Health and alerts, fan control and GPU tuning, History and
    the Benchmark, every part of this PC, widgets, the overlay and the phone app.
  </p>
  <p>
    Fan control and GPU tuning are off (fans on the BIOS, the GPU at stock), with your settings kept for when Pro is on
    again. History kept recording.
  </p>
  <p class="price"><b>{edition.e?.price ?? '$100'}</b> once, for life, on this PC.</p>
  <div class="actions">
    <button type="button" class="primary" disabled={!edition.e?.canBuy} onclick={() => void done(async () => void (edition.e = await buyPro()))}>
      Buy Pro
    </button>
    <button type="button" onclick={() => void done(onkey)}>Enter a key</button>
    <button type="button" onclick={() => void done()}>Not now</button>
  </div>
</dialog>

<style>
  .ended {
    width: min(32rem, calc(100vw - 2rem));
    padding: 1.3rem 1.4rem 1.1rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
    box-shadow: var(--shadow-lift);
  }
  .ended::backdrop {
    background: rgb(0 0 0 / 55%);
  }
  h2 {
    margin: 0 0 0.7rem;
    font-size: 1.15rem;
  }
  p {
    margin: 0 0 0.6rem;
    font-size: 0.86rem;
    line-height: 1.45;
  }
  .price b {
    font-size: 1.1rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.4rem;
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
</style>
