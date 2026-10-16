<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    title: string;
    subtitle?: string | null;
    wide?: boolean;
    /** To link to the card (the header's Remote chip). */
    id?: string;
    children: Snippet;
  }
  let { title, subtitle = null, wide = false, id, children }: Props = $props();
</script>

<section class="panel card" class:wide {id}>
  <header>
    <h2>{title}</h2>
    {#if subtitle}<p>{subtitle}</p>{/if}
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
  header {
    margin-bottom: 0.75rem;
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
