<script lang="ts">
  // A sheet that rises from the bottom (1.7): confirmations and choices, within the thumb's reach.
  // A modal dialog underneath, so focus stays in it and Back or Esc closes it; a tap above it
  // closes it too.
  import type { Snippet } from 'svelte';

  interface Props {
    open: boolean;
    title: string;
    onclose: () => void;
    children: Snippet;
  }
  let { open, title, onclose, children }: Props = $props();
  const id = $props.id();

  let dialog = $state<HTMLDialogElement | null>(null);

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  function outside(e: MouseEvent): void {
    if (!dialog || e.target !== dialog) return;
    const r = dialog.getBoundingClientRect();
    if (e.clientY < r.top || e.clientX < r.left || e.clientX > r.right) dialog.close();
  }
</script>

<dialog bind:this={dialog} class="sheet" aria-labelledby="{id}-title" onclose={() => open && onclose()} onclick={outside}>
  <span class="grab" aria-hidden="true"></span>
  <h2 id="{id}-title">{title}</h2>
  {#if open}{@render children()}{/if}
</dialog>

<style>
  .sheet {
    position: fixed;
    inset: auto 0 0;
    width: 100%;
    max-width: 40rem;
    max-height: 88dvh;
    margin: auto auto 0;
    padding: 0.5rem max(1.1rem, env(safe-area-inset-right)) calc(1.1rem + env(safe-area-inset-bottom))
      max(1.1rem, env(safe-area-inset-left));
    overflow-x: hidden;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-bottom: 0;
    border-radius: 22px 22px 0 0;
    background: var(--panel);
    color: var(--text);
    box-shadow: 0 -24px 48px -24px var(--shadow-color);
  }
  .sheet[open] {
    animation: up var(--dur-3) var(--ease-out);
  }
  .sheet::backdrop {
    background: rgb(0 0 0 / 50%);
    animation: fade var(--dur-2) ease-out;
  }
  .grab {
    display: block;
    width: 2.5rem;
    height: 0.3rem;
    margin: 0.15rem auto 0.8rem;
    border-radius: 999px;
    background: var(--line);
  }
  h2 {
    margin: 0 0 0.6rem;
    color: var(--text);
    font-size: 1.1rem;
    letter-spacing: 0;
    text-transform: none;
  }
  @keyframes up {
    from {
      transform: translateY(100%);
    }
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
