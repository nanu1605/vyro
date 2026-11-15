<script lang="ts">
  // One of Settings' tiles (2.5): a group's name and its state in a line; it opens the group in
  // its window, growing out of the tile.
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Lock from '@lucide/svelte/icons/lock';
  import type { Component } from 'svelte';

  interface Props {
    id: string;
    title: string;
    line: string;
    icon: Component<{ size?: number; 'aria-hidden'?: boolean | 'true' }>;
    /** Part of Pro, on Free. */
    locked?: boolean;
    onopen: (from: { x: number; y: number }) => void;
  }
  let { id, title, line, icon: Icon, locked = false, onopen }: Props = $props();
</script>

<button
  type="button"
  class="tile panel"
  data-group={id}
  aria-labelledby="tile-{id}-name"
  aria-describedby="tile-{id}-line"
  onclick={(e) => {
    const r = e.currentTarget.getBoundingClientRect();
    onopen({ x: r.left + r.width / 2, y: r.top + r.height / 2 });
  }}
>
  <span class="icon" aria-hidden="true"><Icon size={20} aria-hidden="true" /></span>
  <span class="text">
    <span class="name" id="tile-{id}-name"
      >{title}{#if locked}<Lock size={13} aria-hidden="true" /><span class="sr-only">, part of Pro</span>{/if}</span
    >
    <span class="line" id="tile-{id}-line">{line}</span>
  </span>
  <span class="go" aria-hidden="true"><ChevronRight size={18} aria-hidden="true" /></span>
</button>

<style>
  /* Sized by Settings' page width (cqi): as today in the default window, a little bigger as the
     tiles stretch across a wide one. */
  .tile {
    --pad: clamp(1rem, 0.5rem + 0.9cqi, 1.6rem);
    display: flex;
    align-items: center;
    gap: clamp(0.85rem, 0.5rem + 0.6cqi, 1.2rem);
    width: 100%;
    min-height: clamp(5.25rem, 3rem + 3.5cqi, 7.5rem);
    padding: 1rem var(--pad) 1rem calc(var(--pad) + 0.1rem);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .tile:hover {
    transform: translateY(-2px);
  }
  .tile:active {
    transform: translateY(0) scale(0.99);
  }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: clamp(2.6rem, 1.6rem + 1.2cqi, 3.4rem);
    height: clamp(2.6rem, 1.6rem + 1.2cqi, 3.4rem);
    border-radius: clamp(12px, 0.4rem + 0.3cqi, 15px);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
    color: var(--accent);
  }
  .icon :global(svg) {
    width: clamp(20px, 0.9rem + 0.4cqi, 26px);
    height: clamp(20px, 0.9rem + 0.4cqi, 26px);
  }
  :global(:root[data-glass='on']) .icon {
    box-shadow:
      inset 0 1px 0 var(--glass-rim),
      inset 0 0 0 1px var(--accent-line);
  }
  .text {
    display: grid;
    flex: 1;
    gap: 0.2rem;
    min-width: 0;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: clamp(0.98rem, 0.85rem + 0.25cqi, 1.15rem);
    font-weight: 600;
  }
  .name :global(svg) {
    color: var(--muted);
  }
  .line {
    overflow: hidden;
    color: var(--muted);
    font-size: clamp(0.8rem, 0.72rem + 0.15cqi, 0.9rem);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .go {
    flex: none;
    color: var(--muted);
    transition: transform var(--dur-2) var(--ease-out);
  }
  .tile:hover .go {
    color: var(--text);
    transform: translateX(2px);
  }
</style>
