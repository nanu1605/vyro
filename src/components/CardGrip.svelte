<script lang="ts">
  // A card's grip (2.0.2): drag it to move the card, or focus it and use the arrow keys (Home and
  // End for first and last). Only in a view's card grid.
  import GripVertical from '@lucide/svelte/icons/grip-vertical';
  import { getContext } from 'svelte';
  import { GRID, type GridContext } from './CardGrid.svelte';

  interface Props {
    /** The card's id in its grid. */
    id: string;
    title: string;
  }
  let { id, title }: Props = $props();
  const grid = getContext<GridContext | undefined>(GRID);
  let me = $state<HTMLButtonElement | undefined>();
</script>

{#if grid?.has(id)}
  <button
    bind:this={me}
    type="button"
    class="grip"
    aria-label="Move {grid.title(id) ?? title}"
    title="Drag to move, or use the arrow keys"
    onpointerdown={(e) => me && grid.grab(e, id, me)}
    onkeydown={(e) => me && void grid.key(e, id, me)}
  >
    <GripVertical size={15} aria-hidden="true" />
  </button>
{/if}

<style>
  .grip {
    display: inline-grid;
    flex: none;
    place-items: center;
    width: 1.6rem;
    height: 1.6rem;
    /* In the card's corner, without making its title row any taller. */
    margin: -0.4rem -0.45rem -0.4rem 0;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: none;
    color: var(--muted);
    cursor: grab;
    touch-action: none;
  }
  .grip:hover {
    background: var(--accent-soft);
    color: var(--text);
  }
</style>
