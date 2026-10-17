<script lang="ts">
  import { NA } from '../format';

  interface Props {
    rows: Array<[string, string | number | null | undefined]>;
  }
  let { rows }: Props = $props();
</script>

<dl>
  {#each rows as [label, value], i (i)}
    <div class="pair">
      <dt>{label}</dt>
      <dd class:na={value == null || value === ''}>{value == null || value === '' ? NA : value}</dd>
    </div>
  {/each}
</dl>

<style>
  dl {
    display: grid;
    grid-template-columns: minmax(7rem, max-content) 1fr;
    gap: 0.4rem 1rem;
    margin: 0;
    font-size: 0.85rem;
  }
  /* Elsewhere a pair is just its label and value in the grid's two columns. */
  .pair {
    display: contents;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .na {
    color: var(--muted);
  }
  /* On the Dashboard (1.2) cards can be narrow: in a shared column the labels took the room and
     the values broke a letter or two a line. Each pair is its own row there: the value at the
     right where it fits beside its label, else whole on the next line. Wide cards set pairs side
     by side, as many as fit at 13rem or more, so a value stays near its label. */
  :global(.dash-cell) dl {
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 13rem), 1fr));
    column-gap: 1.5rem;
  }
  :global(.dash-cell) .pair {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.1rem 0.75rem;
    min-width: 0;
  }
  :global(.dash-cell) dt,
  :global(.dash-cell) dd {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  :global(.dash-cell) dd {
    margin-left: auto;
    text-align: right;
  }
</style>
