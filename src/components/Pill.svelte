<script lang="ts">
  interface Props {
    /** true → on, false → off, null → unknown. */
    value: boolean | null | undefined;
    on?: string;
    off?: string;
    unknown?: string;
    /** Colour "off" as a warning (e.g. Secure Boot disabled). */
    warnOff?: boolean;
  }
  let { value, on = 'Enabled', off = 'Disabled', unknown = 'Unknown', warnOff = false }: Props = $props();
  const tone = $derived(value == null ? 'unknown' : value ? 'on' : warnOff ? 'warn' : 'off');
</script>

<span class="pill {tone}">{value == null ? unknown : value ? on : off}</span>

<style>
  .pill {
    display: inline-block;
    padding: 0.05rem 0.55rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .on {
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
    color: var(--ok);
  }
  .warn {
    border-color: color-mix(in srgb, var(--warn) 45%, transparent);
    color: var(--warn);
  }
</style>
