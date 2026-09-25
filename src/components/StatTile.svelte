<script lang="ts">
  import { fmt } from '../format';

  interface Props {
    label: string;
    value: number | null | undefined;
    unit: string;
    digits?: number;
    /** Secondary line, e.g. "of 32.0 GB". */
    sub?: string | null;
    /** Thresholds for warn / hot colouring. */
    warn?: number;
    hot?: number;
  }
  let { label, value, unit, digits = 0, sub = null, warn, hot }: Props = $props();

  const level = $derived(
    value == null ? 'na' : hot != null && value >= hot ? 'hot' : warn != null && value >= warn ? 'warn' : 'ok',
  );
</script>

<div class="panel tile {level}">
  <span class="label">{label}</span>
  <span class="value">{fmt(value, digits)}<small>{value == null ? '' : unit}</small></span>
  {#if sub}<span class="sub">{sub}</span>{/if}
</div>

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.75rem 0.9rem;
  }
  .label {
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .value {
    font-family: var(--font-num);
    font-size: 1.45rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    transition: color 0.4s;
  }
  .value small {
    margin-left: 0.2em;
    color: var(--muted);
    font-size: 0.6em;
    font-weight: 500;
  }
  .sub {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .warn .value {
    color: var(--warn);
  }
  .hot .value {
    color: var(--hot);
  }
  .na .value {
    color: var(--muted);
  }
</style>
