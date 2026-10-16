<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import LoadState from './LoadState.svelte';
  import Meter from './Meter.svelte';
  import { fmt, fmtGBTotal, fmtRate, fmtStorage, NA } from '../format';
  import { fmtTemp } from '../prefs.svelte';
  import { getDrives } from '../ipc';
  import type { DriveInfo, Status, StorageSnapshot } from '../types';
  import { scrollable } from '../scrollable';

  interface Props {
    /** Live readings from the `sensors` event, matched to drives by id. */
    live: StorageSnapshot[];
  }
  let { live }: Props = $props();

  let drives = $state.raw<DriveInfo[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let revealed = $state<Record<string, boolean>>({});

  function load(): void {
    loading = true;
    getDrives()
      .then((v) => {
        drives = v;
        error = null;
      })
      .catch((e: unknown) => (error = String(e)))
      .finally(() => (loading = false));
  }

  $effect(() => {
    load();
  });

  // Re-read details when a drive appears or disappears.
  const liveIds = $derived(live.map((s) => s.id).sort().join(','));
  let seenIds = '';
  $effect(() => {
    const ids = liveIds;
    if (seenIds !== '' && ids !== seenIds && !loading) load();
    seenIds = ids;
  });

  const byId = $derived(new Map(live.map((s) => [s.id, s])));
  const needsAdmin = $derived(drives?.some((d) => d.smartError?.startsWith('SMART / health data needs')) ?? false);

  const HEALTH: Record<Status, string> = { ok: 'Good', warn: 'Caution', fail: 'Bad', unknown: 'Unknown' };

  const temp = (c: number | null | undefined): string | null => fmtTemp(c);

  function limits(d: DriveInfo): string | null {
    const parts = [d.tempWarnC != null ? `warn ${fmtTemp(d.tempWarnC)}` : null, d.tempCritC != null ? `critical ${fmtTemp(d.tempCritC)}` : null];
    return parts.filter(Boolean).join(' · ') || null;
  }

  function hours(h: number | null): string | null {
    return h == null ? null : `${fmt(h)} h (${fmt(h / 24)} days)`;
  }

  function minutes(m: number): string {
    return m >= 120 ? `${fmt(m / 60, 1)} h` : `${fmt(m)} min`;
  }

  function kind(d: DriveInfo): string {
    if (d.ssd === true) return d.type === 'NVMe' ? 'NVMe SSD' : 'SSD';
    if (d.ssd === false) return d.rpm != null ? `HDD · ${fmt(d.rpm)} RPM` : 'HDD';
    return d.bus;
  }

  // Some attributes pack extra fields into the upper raw bytes: temperatures keep min / max
  // there, and Seagate stores minutes / seconds above the power-on and head-flying hours.
  function rawValue(d: DriveInfo, id: number, raw: number): string {
    const temp = id === 194 || id === 190 || (id === 231 && d.ssd === false);
    if (temp) return fmtTemp(raw % 256) ?? NA;
    if (id === 9 || id === 240) return `${fmt(raw % 2 ** 32)} h`;
    return fmt(raw);
  }

  function usedPct(used: number | null | undefined, total: number | null | undefined): number | null {
    return used != null && total ? (used / total) * 100 : null;
  }
</script>

{#if error || !drives || drives.length === 0}
  <LoadState what="drives" {error} loading={!drives || loading} empty="No drives found." retry={load} />
{:else}
  <div class="toolbar">
    <button type="button" onclick={load} disabled={loading}>{loading ? 'Reading…' : 'Re-read health'}</button>
    <span class="muted small">Rates update live; health and SMART refresh every few seconds in the background.</span>
  </div>
  {#if needsAdmin}
    <p class="notice" role="status">
      The Vyro sensor service is not running, so SMART health, drive history counters and some temperatures are
      hidden. Reinstall Vyro or start <b>Vyro Sensor Service</b> in Services, then restart Vyro.
    </p>
  {/if}

  <div class="grid">
    {#each drives as d (d.id)}
      {@const s = byId.get(d.id)}
      {@const t = s?.tempC ?? d.tempC}
      <Card title="Disk {d.index}{d.system ? ' · System' : ''}{d.removable ? ' · Removable' : ''}" subtitle={d.model}>
        <p class="meta">{[d.vendor, fmtStorage(d.capacityMB), kind(d), d.interface ?? d.bus].filter((x) => x && x !== NA).join(' · ')}</p>

        <div class="badges">
          <span class="health {s?.health ?? d.health}">{HEALTH[s?.health ?? d.health]}{d.healthPct != null ? ` · ${fmt(d.healthPct)}% life` : ''}</span>
          {#if t != null}
            <span class="temp" class:hot={d.tempWarnC != null && t >= d.tempWarnC}>{fmtTemp(t)}</span>
          {/if}
        </div>
        {#if d.healthNotes.length > 0}
          <ul class="notes">
            {#each d.healthNotes as n, i (i)}<li>{n}</li>{/each}
          </ul>
        {/if}
        {#if d.smartError && !needsAdmin}
          <p class="muted small">{d.smartError}</p>
        {/if}

        <h3>Activity</h3>
        <div class="rates">
          <div><span class="muted">Read</span><b>{fmtRate(s?.readMBs)}</b><small>{s?.readIops != null ? `${fmt(s.readIops)} IOPS` : ''}</small></div>
          <div><span class="muted">Write</span><b>{fmtRate(s?.writeMBs)}</b><small>{s?.writeIops != null ? `${fmt(s.writeIops)} IOPS` : ''}</small></div>
        </div>
        <Meter label="Active time" value={s?.activityPct} />

        <h3>Space</h3>
        {#if d.volumes.length > 0}
          <Meter label="Used" value={usedPct(d.usedMB, (d.usedMB ?? 0) + (d.freeMB ?? 0))} />
          <ul class="volumes">
            {#each d.volumes as v (v.letter)}
              <li>
                <b>{v.letter}</b>
                <span class="muted">{[v.label, v.fileSystem].filter(Boolean).join(' · ')}</span>
                <span class="num">{fmtStorage(v.freeMB)} free of {fmtStorage(v.totalMB)}</span>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="muted small">No mounted volumes.</p>
        {/if}

        <h3>Health</h3>
        <KV
          rows={[
            ['Temperature', temp(t)],
            ['Limits', limits(d)],
            ['Power-on time', hours(d.powerOnHours)],
            ['Power cycles', d.powerCycles != null ? fmt(d.powerCycles) : null],
            ['Total read', d.totalReadGB != null ? fmtGBTotal(d.totalReadGB) : null],
            ['Total written', d.totalWriteGB != null ? fmtGBTotal(d.totalWriteGB) : null],
          ]}
        />

        {#if d.nvme}
          {@const n = d.nvme}
          <h3>NVMe health log</h3>
          <KV
            rows={[
              ['Life used', `${n.percentageUsed}%`],
              ['Available spare', `${n.availableSparePct}% (threshold ${n.spareThresholdPct}%)`],
              ['Critical warnings', n.criticalWarnings.join('; ') || 'None'],
              ['Unsafe shutdowns', fmt(n.unsafeShutdowns)],
              ['Media errors', fmt(n.mediaErrors)],
              ['Error log entries', fmt(n.errorLogEntries)],
              ['Busy time', minutes(n.busyMinutes)],
              ['Above warning temp', minutes(n.warningTempMinutes)],
              ['Above critical temp', minutes(n.criticalTempMinutes)],
              ['Thermal throttling', n.throttleEvents.some((x) => x > 0) ? `${n.throttleEvents.join(' / ')} events, ${n.throttleSecs.join(' / ')} s` : 'None'],
              ['Sensors', n.sensorsC.map((c, i) => (c == null ? null : `#${i + 1} ${fmtTemp(c)}`)).filter(Boolean).join(' · ') || null],
            ]}
          />
        {/if}

        <h3>Device</h3>
        <KV
          rows={[
            ['Firmware', d.firmware],
            ['Bus', d.bus],
            ['TRIM', d.trim == null ? null : d.trim ? 'Supported' : 'Not supported'],
            ['Windows disk', `\\\\.\\PhysicalDrive${d.index}`],
          ]}
        />
        <div class="serial">
          <span>Serial</span>
          {#if d.serial}
            <code>{revealed[d.id] ? d.serial : '•'.repeat(Math.min(d.serial.length, 12))}</code>
            <button type="button" onclick={() => (revealed[d.id] = !revealed[d.id])} aria-pressed={!!revealed[d.id]}>
              {revealed[d.id] ? 'Hide' : 'Show'}
            </button>
          {:else}
            <code class="muted">{NA}</code>
          {/if}
        </div>

        {#if d.smart.length > 0}
          <details>
            <summary>SMART attributes ({d.smart.length})</summary>
            <div class="table-wrap" use:scrollable role="region" aria-label="SMART attributes">
              <table>
                <thead>
                  <tr><th>ID</th><th class="name">Attribute</th><th>Cur</th><th>Worst</th><th>Thr</th><th>Raw</th></tr>
                </thead>
                <tbody>
                  {#each d.smart as a (a.id)}
                    <tr class:failing={a.failing}>
                      <td>{a.id.toString(16).toUpperCase().padStart(2, '0')}</td>
                      <td class="name">{a.name}{a.prefail ? ' *' : ''}</td>
                      <td>{a.current}</td>
                      <td>{a.worst}</td>
                      <td>{a.threshold ?? NA}</td>
                      <td title="0x{a.rawHex}">{rawValue(d, a.id, a.raw)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
            <p class="muted small">* pre-failure attribute. Raw meaning and units are vendor-specific.</p>
          </details>
        {/if}
      </Card>
    {/each}
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
    gap: 1rem;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin-bottom: 0.9rem;
  }
  button {
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--panel-edge);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .notice {
    margin: 0 0 1rem;
    padding: 0.6rem 0.8rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--warn) 8%, transparent);
    font-size: 0.85rem;
  }
  .meta {
    margin: -0.5rem 0 0.6rem;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .health,
  .temp {
    padding: 0.05rem 0.55rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .health.ok {
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
    color: var(--ok);
  }
  .health.warn,
  .temp.hot {
    border-color: color-mix(in srgb, var(--warn) 45%, transparent);
    color: var(--warn);
  }
  .health.fail {
    border-color: color-mix(in srgb, var(--hot) 55%, transparent);
    color: var(--hot);
  }
  .temp {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .notes {
    margin: 0.5rem 0 0;
    padding-left: 1.1rem;
    color: var(--warn);
    font-size: 0.8rem;
  }
  h3 {
    margin: 1rem 0 0.45rem;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .rates {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .rates div {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .rates span {
    font-size: 0.72rem;
  }
  .rates b {
    font-family: var(--font-num);
    font-size: 1.05rem;
    font-variant-numeric: tabular-nums;
  }
  .rates small {
    min-height: 1em;
    color: var(--muted);
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
  }
  .volumes {
    display: grid;
    gap: 0.3rem;
    margin: 0.5rem 0 0;
    padding: 0;
    list-style: none;
    font-size: 0.82rem;
  }
  .volumes li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.6rem;
    align-items: baseline;
  }
  .num {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .serial {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 0.4rem;
    font-size: 0.85rem;
  }
  .serial span {
    min-width: 7rem;
    color: var(--muted);
  }
  code {
    font-family: var(--font-num);
  }
  details {
    margin-top: 1rem;
    font-size: 0.85rem;
  }
  summary {
    color: var(--muted);
    cursor: pointer;
  }
  .table-wrap {
    margin-top: 0.5rem;
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }
  th,
  td {
    padding: 0.25rem 0.4rem;
    border-bottom: 1px solid var(--line);
    text-align: right;
    white-space: nowrap;
  }
  th {
    color: var(--muted);
    font-weight: 600;
  }
  .name {
    text-align: left;
    white-space: normal;
  }
  tr.failing td {
    color: var(--hot);
    font-weight: 600;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.78rem;
  }
  .error {
    color: var(--hot);
  }
</style>
