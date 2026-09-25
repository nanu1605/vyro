<script lang="ts">
  import { NA } from '../format';

  interface Props {
    rows: Array<[string, string | number | null | undefined]>;
  }
  let { rows }: Props = $props();
</script>

<dl>
  {#each rows as [label, value], i (i)}
    <dt>{label}</dt>
    <dd class:na={value == null || value === ''}>{value == null || value === '' ? NA : value}</dd>
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
</style>
