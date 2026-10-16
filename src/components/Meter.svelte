<script lang="ts">
  import { fmt, NA } from '../format';

  interface Props {
    label: string;
    /** 0–100; null renders as unavailable. */
    value: number | null | undefined;
    /** Text shown instead of the percentage (e.g. "N/A"). */
    note?: string | null;
  }
  let { label, value, note = null }: Props = $props();
  const pct = $derived(value == null ? 0 : Math.min(100, Math.max(0, value)));
</script>

<div class="meter" class:na={value == null}>
  <span class="label">{label}</span>
  <span class="track" aria-hidden="true"><span class="fill" style:transform="scaleX({pct / 100})"></span></span>
  <span class="value">{value == null ? (note ?? NA) : `${fmt(value)}%`}</span>
</div>

<style>
  .meter {
    display: grid;
    grid-template-columns: 6.5rem 1fr 3rem;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.8rem;
  }
  .label {
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .track {
    height: 0.4rem;
    border-radius: 999px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  /* Scaled rather than sized: a moving width lays out and repaints every frame, a scale runs on
     the compositor. The track's rounded clip keeps the ends round. */
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    transition: transform 0.4s ease-out;
  }
  .value {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .na .value {
    color: var(--muted);
  }
  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
</style>
