<script lang="ts">
  import type { Snippet } from 'svelte';
  import CardGrip from './CardGrip.svelte';

  interface Props {
    title: string;
    subtitle?: string | null;
    wide?: boolean;
    /** To link to the card (the header's Remote chip), and its place in a view's card grid. */
    id?: string;
    /** More classes for the card itself (the display picked in the arrangement). */
    class?: string;
    children: Snippet;
  }
  let { title, subtitle = null, wide = false, id, class: also = '', children }: Props = $props();
</script>

<section class="panel card {also}" class:wide {id}>
  <header>
    <div class="titles">
      <h2>{title}</h2>
      {#if subtitle}<p>{subtitle}</p>{/if}
    </div>
    {#if id}<CardGrip {id} {title} />{/if}
  </header>
  {@render children()}
</section>

<style>
  .card {
    min-width: 0;
  }
  .wide {
    grid-column: 1 / -1;
  }
  /* In a view's card grid (2.0.2): as tall as its room (`--fill`, card-layout.ts), down to the
     next card or the last line, while its cell keeps the card's own height to measure. */
  :global(.card-cell) > .card {
    padding-bottom: calc(1rem + var(--fill, 0px));
    margin-bottom: calc(-1 * var(--fill, 0px));
  }
  header {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h2 {
    margin: 0;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  p {
    margin: 0.3rem 0 0;
    font-size: 1.05rem;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
</style>
