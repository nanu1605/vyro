<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt, fmtGB, NA } from '../format';
  import type { MemorySnapshot } from '../types';

  interface Props {
    mem: MemorySnapshot | null;
  }
  let { mem }: Props = $props();

  const gb = (mb: number | null | undefined) => (mb == null ? null : `${fmtGB(mb)} GB`);
  const of = (a: number | null | undefined, b: number | null | undefined) =>
    a == null ? null : b == null ? gb(a) : `${fmtGB(a)} / ${fmtGB(b)} GB`;

  const dimms = $derived((mem?.dimms ?? []).filter((d) => d.tempC != null));
  const t = $derived(mem?.timings ?? null);

  // Task Manager's composition bar: in use (without modified) | modified | standby | free.
  const parts = $derived.by(() => {
    const m = mem;
    if (m?.totalMB == null || m.standbyMB == null || m.modifiedMB == null || m.freeMB == null) return null;
    const inUse = Math.max(0, m.totalMB - m.modifiedMB - m.standbyMB - m.freeMB);
    return [
      { key: 'in-use', label: 'In use', mb: inUse },
      { key: 'modified', label: 'Modified', mb: m.modifiedMB },
      { key: 'standby', label: 'Standby', mb: m.standbyMB },
      { key: 'free', label: 'Free', mb: m.freeMB },
    ].map((p) => ({ ...p, pct: (p.mb / m.totalMB!) * 100 }));
  });
</script>

<Card title="Memory" subtitle={mem?.usedMB != null ? `${fmtGB(mem.usedMB)} of ${fmtGB(mem.totalMB)} GB in use` : NA}>
  {#if parts}
    <div class="bar" role="img" aria-label={parts.map((p) => `${p.label} ${fmtGB(p.mb)} GB`).join(', ')}>
      {#each parts as p (p.key)}
        <span class="seg {p.key}" style:width="{p.pct}%"></span>
      {/each}
    </div>
    <ul class="legend">
      {#each parts as p (p.key)}
        <li><span class="dot {p.key}"></span>{p.label} <b>{fmtGB(p.mb)} GB</b></li>
      {/each}
    </ul>
  {/if}
  <KV
    rows={[
      ['Available', gb(mem?.availableMB)],
      ['Committed', of(mem?.committedMB, mem?.commitLimitMB)],
      ['Cached', gb(mem?.cachedMB)],
      ['Compressed', mem?.compressedMB != null ? `${fmt(mem.compressedMB)} MB` : null],
      ['Paged pool', mem?.pagedPoolMB != null ? `${fmt(mem.pagedPoolMB)} MB` : null],
      ['Non-paged pool', mem?.nonPagedPoolMB != null ? `${fmt(mem.nonPagedPoolMB)} MB` : null],
      ['Page file', of(mem?.pageFileUsedMB, mem?.pageFileTotalMB)],
      ['Page faults', mem?.pageFaultsPerSec != null ? `${fmt(mem.pageFaultsPerSec)} /s` : null],
      ['Hard faults', mem?.hardFaultsPerSec != null ? `${fmt(mem.hardFaultsPerSec)} /s` : null],
    ]}
  />
  {#if dimms.length}
    <h3>Modules</h3>
    <KV rows={dimms.map((d): [string, string | null] => [d.slot, d.tempC == null ? null : `${fmt(d.tempC, 1)} °C`])} />
  {/if}
  {#if t}
    <h3>Timings</h3>
    <p class="primary" title="CL-tRCD-tRP-tRAS">
      <b>{t.cl}-{t.rcdRd}-{t.rp}-{t.ras}</b> @ {fmt(t.mts)} MT/s · {t.commandRate}T{t.gearDown ? ' · GDM' : ''}
    </p>
    <KV
      rows={[
        ['tRC', `${t.rc}`],
        ['tRCDWR', `${t.rcdWr}`],
        ['tRRD S / L', `${t.rrdS} / ${t.rrdL}`],
        ['tFAW', `${t.faw}`],
        ['tWTR S / L', `${t.wtrS} / ${t.wtrL}`],
        ['tWR / tRTP', `${t.wr} / ${t.rtp}`],
        ['tCWL', `${t.cwl}`],
        ['tRFC / tRFCsb', `${t.rfc} / ${t.rfcSb}`],
        ['tREFI', `${t.refi}`],
      ]}
    />
  {/if}
</Card>

<style>
  h3 {
    margin: 0.9rem 0 0.45rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .primary {
    margin: 0 0 0.4rem;
    color: var(--muted);
    font-size: 0.85rem;
  }
  .primary b {
    color: var(--text);
    font-size: 1rem;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    display: flex;
    height: 0.7rem;
    margin-bottom: 0.5rem;
    overflow: hidden;
    border-radius: 4px;
    background: var(--gauge-track);
  }
  .seg {
    height: 100%;
    transition: width 0.4s ease-out;
  }
  .in-use {
    background: var(--accent-cpu);
  }
  .modified {
    background: var(--warn);
  }
  .standby {
    background: color-mix(in srgb, var(--accent-cpu) 40%, var(--panel));
  }
  .free {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1rem;
    margin: 0 0 0.8rem;
    padding: 0;
    list-style: none;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .legend b {
    color: var(--text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .dot {
    display: inline-block;
    width: 0.6rem;
    height: 0.6rem;
    margin-right: 0.35rem;
    border-radius: 2px;
    vertical-align: -0.05rem;
  }
  @media (prefers-reduced-motion: reduce) {
    .seg {
      transition: none;
    }
  }
</style>
