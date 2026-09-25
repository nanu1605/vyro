<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt, fmtKB, fmtMB, hex4, NA } from '../format';
  import { getHardwareId } from '../ipc';
  import type { CacheInfo, HardwareId } from '../types';

  let hw = $state.raw<HardwareId | null>(null);
  let error = $state<string | null>(null);
  let showSerial = $state(false);

  $effect(() => {
    getHardwareId()
      .then((v) => (hw = v))
      .catch((e: unknown) => (error = String(e)));
  });

  const cpu = $derived(hw?.cpu ?? null);
  const mem = $derived(hw?.memory ?? null);
  const board = $derived(hw?.board ?? null);

  function cacheLabel(c: CacheInfo): string {
    const kind = c.kind === 'data' ? 'Data' : c.kind === 'instruction' ? 'Instr.' : c.kind === 'trace' ? 'Trace' : '';
    return `L${c.level} ${kind}`.trim();
  }

  function cacheValue(c: CacheInfo): string {
    const ways = c.ways ? `, ${c.ways}-way` : '';
    return c.count > 1 ? `${c.count} × ${fmtKB(c.sizeKb)}${ways}` : `${fmtKB(c.sizeKb)}${ways}`;
  }

  const firstDimm = $derived(mem?.dimms[0] ?? null);
  const memSummary = $derived(
    firstDimm
      ? [firstDimm.memoryType, firstDimm.configuredSpeedMts ? `${fmt(firstDimm.configuredSpeedMts)} MT/s` : null]
          .filter(Boolean)
          .join(' · ')
      : null,
  );
</script>

{#if error}
  <p class="error" role="alert">Could not read hardware information: {error}</p>
{:else if !hw}
  <p class="loading" aria-live="polite">Reading hardware…</p>
{:else}
  <div class="grid">
    <Card title="Processor" subtitle={cpu?.brand ?? 'Unknown CPU'} wide>
      <div class="split">
        <KV
          rows={[
            ['Vendor', cpu?.vendor],
            ['Socket', cpu?.socket],
            ['Architecture', cpu?.architecture],
            ['Family / model', cpu?.family != null ? `${cpu.family} / ${cpu.model} (stepping ${cpu.stepping})` : null],
            ['Cores / threads', cpu?.cores != null || cpu?.threads != null ? `${cpu?.cores ?? NA} / ${cpu?.threads ?? NA}` : null],
            ['Hybrid', cpu?.hybrid ? `${cpu.performanceCores ?? '?'} P-cores + ${cpu.efficiencyCores ?? '?'} E-cores` : 'No'],
            ['Base clock', cpu?.baseMhz != null ? `${fmt(cpu.baseMhz)} MHz` : null],
            ['Max clock', cpu?.maxMhz != null ? `${fmt(cpu.maxMhz)} MHz` : null],
            ['Packages', cpu?.packages],
          ]}
        />
        {#if cpu && cpu.caches.length > 0}
          <div>
            <h3>Caches</h3>
            <KV rows={cpu.caches.map((c): [string, string] => [cacheLabel(c), cacheValue(c)])} />
          </div>
        {/if}
      </div>

      {#if cpu && cpu.coreMap.length > 0}
        <h3>Core map</h3>
        <ul class="cores" aria-label="Physical cores">
          {#each cpu.coreMap as core (core.index)}
            <li
              class={core.kind}
              title="Core {core.index} · {core.kind} · logical {core.logical.join(', ')}"
            >
              <span>{core.index}</span>
              {#if core.logical.length > 1}<small>×{core.logical.length}</small>{/if}
            </li>
          {/each}
        </ul>
        {#if cpu.hybrid}
          <p class="legend"><i class="performance"></i> Performance <i class="efficiency"></i> Efficiency · ×2 = SMT</p>
        {/if}
      {/if}

      {#if cpu && cpu.isa.length > 0}
        <h3>Instruction sets</h3>
        <ul class="chips">
          {#each cpu.isa as f (f)}<li>{f}</li>{/each}
        </ul>
      {/if}
    </Card>

    <Card title="Memory" subtitle={mem?.totalMb != null ? `${fmtMB(Math.round(mem.totalMb))} ${memSummary ?? ''}` : null}>
      <KV
        rows={[
          ['Channels', mem?.channels != null ? `${mem.channels} (approx.)` : null],
          ['Slots used', mem?.slotsTotal != null ? `${mem.slotsUsed} of ${mem.slotsTotal}` : mem?.dimms.length ? mem.slotsUsed : null],
          ['Max capacity', mem?.maxCapacityMb != null ? fmtMB(mem.maxCapacityMb) : null],
          ['Timings', 'Needs the sensor driver (Phase 6)'],
        ]}
      />
      {#if mem && mem.dimms.length > 0}
        <ul class="dimms">
          {#each mem.dimms as d, i (i)}
            <li>
              <div class="slot">{d.slot}{#if d.bank}<span>{` · ${d.bank}`}</span>{/if}</div>
              <div>
                <b>{fmtMB(d.sizeMB)}</b>
                {[d.manufacturer, d.partNumber].filter(Boolean).join(' ') || ''}
              </div>
              <div class="spec">
                {[
                  d.memoryType,
                  d.configuredSpeedMts ? `${fmt(d.configuredSpeedMts)} MT/s` : null,
                  d.speedMts && d.speedMts !== d.configuredSpeedMts ? `${fmt(d.speedMts)} rated` : null,
                  d.configuredVoltageMv ? `${fmt(d.configuredVoltageMv / 1000, 2)} V` : null,
                  d.rank ? `${d.rank}R` : null,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </Card>

    <Card title="Motherboard" subtitle={[board?.manufacturer, board?.product].filter(Boolean).join(' ') || null}>
      <KV
        rows={[
          ['Revision', board?.version],
          ['Chipset', board?.chipset],
          ['BIOS', [board?.biosVendor, board?.biosVersion].filter(Boolean).join(' ') || null],
          ['BIOS date', board?.biosDate],
          ['System', [board?.systemManufacturer, board?.systemProduct].filter(Boolean).join(' ') || null],
          ['SMBIOS', hw.smbiosVersion],
        ]}
      />
      <div class="serial">
        <span>Serial</span>
        {#if board?.serial}
          <code>{showSerial ? board.serial : '•'.repeat(Math.min(board.serial.length, 12))}</code>
          <button type="button" onclick={() => (showSerial = !showSerial)} aria-pressed={showSerial}>
            {showSerial ? 'Hide' : 'Show'}
          </button>
        {:else}
          <code class="na">{NA}</code>
        {/if}
      </div>
      <p class="note">Shown on this device only; never logged or sent anywhere.</p>
    </Card>

    {#each hw.gpus as gpu, i (i)}
      <Card title="Graphics" subtitle={gpu.name}>
        <KV
          rows={[
            ['Vendor', gpu.vendor],
            ['VRAM', gpu.vramMB != null ? fmtMB(Math.round(gpu.vramMB)) : null],
            ['Memory type', gpu.memoryType],
            ['Driver', [gpu.vendorDriverVersion, gpu.driverVersion && gpu.driverVersion !== gpu.vendorDriverVersion ? `(${gpu.driverVersion})` : null].filter(Boolean).join(' ') || null],
            ['Driver date', gpu.driverDate],
            ['GPU BIOS', gpu.biosVersion],
            ['PCI id', `${hex4(gpu.vendorId)}:${hex4(gpu.deviceId)} rev ${gpu.revision.toString(16).toUpperCase()}`],
          ]}
        />
      </Card>
    {:else}
      <Card title="Graphics"><p class="note">No hardware GPU reported.</p></Card>
    {/each}
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
    gap: 1rem;
  }
  .split {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
    gap: 1.25rem 2rem;
  }
  h3 {
    margin: 1.1rem 0 0.5rem;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .split h3 {
    margin-top: 0;
  }
  .cores {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .cores li {
    display: grid;
    place-items: center;
    position: relative;
    width: 2.1rem;
    height: 2.1rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    font-family: var(--font-num);
    font-size: 0.75rem;
  }
  .cores small {
    position: absolute;
    right: 2px;
    bottom: 0;
    color: var(--muted);
    font-size: 0.55rem;
  }
  .performance {
    background: color-mix(in srgb, var(--accent-cpu) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent-cpu) 55%, transparent) !important;
  }
  .efficiency {
    background: color-mix(in srgb, var(--accent-gpu) 16%, transparent);
    border-color: color-mix(in srgb, var(--accent-gpu) 45%, transparent) !important;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0.5rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .legend i {
    display: inline-block;
    width: 0.7rem;
    height: 0.7rem;
    border: 1px solid;
    border-radius: 3px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chips li {
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .dimms {
    display: grid;
    gap: 0.6rem;
    margin: 0.9rem 0 0;
    padding: 0;
    list-style: none;
    font-size: 0.82rem;
  }
  .dimms li {
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .dimms .slot {
    margin-bottom: 0.2rem;
    color: var(--muted);
    font-size: 0.72rem;
    overflow-wrap: anywhere;
  }
  .dimms .slot span {
    opacity: 0.7;
  }
  .dimms b {
    margin-right: 0.3rem;
  }
  .dimms .spec {
    margin-top: 0.15rem;
    color: var(--muted);
  }
  .serial {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-top: 0.4rem;
    font-size: 0.85rem;
  }
  .serial span {
    min-width: 7rem;
    color: var(--muted);
  }
  code {
    font-family: var(--font-num);
    letter-spacing: 0.05em;
  }
  .na {
    color: var(--muted);
  }
  button {
    padding: 0.15rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }
  button:hover {
    border-color: var(--accent);
  }
  .note,
  .loading {
    margin: 0.6rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .error {
    color: var(--hot);
  }
</style>
