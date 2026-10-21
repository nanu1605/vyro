<script lang="ts">
  // Where toasts show (1.7): above the tabs, newest last. A tap dismisses one.
  import { dismiss, toasts } from './toast.svelte';

  interface Props {
    /** The tabs show: sit above them. */
    raised: boolean;
  }
  let { raised }: Props = $props();
</script>

<div class="toasts" class:raised role="status" aria-live="polite">
  {#each toasts.list as t (t.id)}
    <button type="button" class="toast {t.tone}" onclick={() => dismiss(t.id)}>
      {t.text}
    </button>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 0;
    bottom: calc(1rem + env(safe-area-inset-bottom));
    left: 0;
    z-index: 40;
    display: grid;
    gap: 0.45rem;
    max-width: 40rem;
    margin: 0 auto;
    padding: 0 1rem;
    pointer-events: none;
  }
  .toasts.raised {
    bottom: calc(var(--nav-h) + env(safe-area-inset-bottom) + 0.6rem);
  }
  .toast {
    min-width: 0;
    padding: 0.7rem 0.9rem;
    border: 1px solid color-mix(in srgb, var(--ok) 45%, var(--line));
    border-radius: 14px;
    background: var(--panel);
    color: var(--text);
    font-weight: 600;
    text-align: left;
    overflow-wrap: anywhere;
    box-shadow: 0 12px 28px -12px var(--shadow-color);
    pointer-events: auto;
    animation: pop var(--dur-2) var(--ease-out);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
  }
  .toast.warn {
    border-color: color-mix(in srgb, var(--warn) 55%, var(--line));
  }
  .toast.hot {
    border-color: color-mix(in srgb, var(--hot) 60%, var(--line));
  }
</style>
