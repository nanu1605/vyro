<script lang="ts">
  import { onMount } from 'svelte';
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import LoadState from './LoadState.svelte';
  import Pill from './Pill.svelte';
  import { fmt, fmtDate, fmtDateTime, fmtDuration, NA } from '../format';
  import { getBackendStatus, getSystemInfo } from '../ipc';
  import type { BackendStatus, PowerEvent, StartupSource, SystemInfo } from '../types';
  import { scrollable } from '../scrollable';

  let sys = $state.raw<SystemInfo | null>(null);
  let backend = $state.raw<BackendStatus | null>(null);
  /** Admin-only data (BitLocker) comes from the sensor service. */
  const service = $derived(backend ? backend.mode === 'service' && backend.connected : null);
  let error = $state<string | null>(null);
  let now = $state(Math.floor(Date.now() / 1000));

  function load(): void {
    error = null;
    getSystemInfo()
      .then((v) => (sys = v))
      .catch((e: unknown) => (error = String(e)));
  }

  onMount(() => {
    load();
    getBackendStatus()
      .then((b) => (backend = b))
      .catch(() => {});
    // Uptime ticks locally; no need to ask the engine every second.
    const id = setInterval(() => (now = Math.floor(Date.now() / 1000)), 1000);
    return () => clearInterval(id);
  });

  const uptime = $derived(sys?.bootTime != null ? now - sys.bootTime : null);
  const sec = $derived(sys?.security ?? null);

  const SOURCE: Record<StartupSource, string> = {
    registryMachine: 'Registry (all users)',
    registryUser: 'Registry (you)',
    folderMachine: 'Startup folder (all users)',
    folderUser: 'Startup folder (you)',
    scheduledTask: 'Scheduled task',
  };

  const EVENT: Record<PowerEvent['kind'], string> = {
    boot: 'Boot',
    shutdown: 'Shutdown',
    unexpectedShutdown: 'Unexpected shutdown',
    sleep: 'Sleep',
    wake: 'Wake',
  };
</script>

{#if error || !sys}
  <LoadState what="system information" {error} loading={!sys} retry={load} />
{:else}
  <div class="grid">
    <Card title="Operating system" subtitle={sys.os.name}>
      <KV
        rows={[
          ['Version', sys.os.displayVersion],
          ['Build', sys.os.build],
          ['Edition', sys.os.edition],
          ['Architecture', sys.os.architecture],
          ['Installed', fmtDate(sys.os.installDate)],
        ]}
      />
    </Card>

    <Card title="Uptime" subtitle={fmtDuration(uptime)}>
      <KV
        rows={[
          ['Last boot', fmtDateTime(sys.bootTime)],
          ['Processes', fmt(sys.processes.processes)],
          ['Threads', fmt(sys.processes.threads)],
          ['Handles', fmt(sys.processes.handles)],
        ]}
      />
    </Card>

    <Card title="Security">
      <dl class="flags">
        <dt>Firmware</dt>
        <dd><Pill value={sec?.uefi} on="UEFI" off="Legacy BIOS" warnOff /></dd>
        <dt>Secure Boot</dt>
        <dd><Pill value={sec?.secureBoot} warnOff /></dd>
        <dt>TPM</dt>
        <dd><Pill value={sec?.tpmPresent} on={sec?.tpmVersion ? `TPM ${sec.tpmVersion}` : 'Present'} off="Not found" warnOff /></dd>
        <dt>Virtualization</dt>
        <dd><Pill value={sec?.virtualizationEnabled} /></dd>
        <dt>Hypervisor</dt>
        <dd><Pill value={sec?.hypervisorPresent} on="Running" off="None" /></dd>
        <dt>BitLocker ({'C:'})</dt>
        <dd>
          <Pill value={sec?.bitlockerSystemDrive} on="Protected" off="Off" unknown={service === false ? 'Needs the service' : 'Unknown'} />
        </dd>
        {#if backend}
          <dt>Sensor service</dt>
          <dd><Pill value={service} on={backend.serviceVersion ? `Connected (${backend.serviceVersion})` : 'Connected'} off="Not running" warnOff /></dd>
        {/if}
      </dl>
    </Card>

    <Card title="Startup apps" subtitle={sys.startupApps.length ? `${sys.startupApps.filter((a) => a.enabled).length} enabled of ${sys.startupApps.length}` : null} wide>
      {#if sys.startupApps.length === 0}
        <p class="muted">No startup entries found.</p>
      {:else}
        <div class="table-wrap" use:scrollable role="region" aria-label="Startup apps">
          <table>
            <thead>
              <tr><th>Name</th><th>Status</th><th>Source</th><th>Command</th></tr>
            </thead>
            <tbody>
              {#each sys.startupApps as app, i (i)}
                <tr class:disabled={!app.enabled}>
                  <td>{app.name}</td>
                  <td><Pill value={app.enabled} /></td>
                  <td class="muted">{SOURCE[app.source]}</td>
                  <td class="cmd" title={app.command}>{app.command || NA}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <p class="muted note">Startup impact is not shown: Windows does not expose it publicly.</p>
      {/if}
    </Card>

    <Card title="Power history" subtitle={sys.powerEvents.length ? `last ${sys.powerEvents.length} events` : null} wide>
      {#if sys.powerEvents.length === 0}
        <p class="muted">No power events available.</p>
      {:else}
        <ol class="events">
          {#each sys.powerEvents as e, i (i)}
            <li class={e.kind}>
              <time datetime={new Date(e.ts * 1000).toISOString()}>{fmtDateTime(e.ts)}</time>
              <span class="kind">{EVENT[e.kind]}</span>
              {#if e.detail}<span class="muted">{e.detail}</span>{/if}
            </li>
          {/each}
        </ol>
      {/if}
    </Card>
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
    gap: 1rem;
  }
  .flags {
    display: grid;
    grid-template-columns: minmax(7rem, max-content) 1fr;
    align-items: center;
    gap: 0.45rem 1rem;
    margin: 0;
    font-size: 0.85rem;
  }
  .flags dt {
    color: var(--muted);
  }
  .flags dd {
    margin: 0;
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  th {
    color: var(--muted);
    font-weight: 500;
    text-align: left;
  }
  th,
  td {
    padding: 0.4rem 0.75rem 0.4rem 0;
    border-bottom: 1px solid var(--line);
    white-space: nowrap;
  }
  .cmd {
    max-width: 28rem;
    overflow: hidden;
    color: var(--muted);
    font-family: var(--font-num);
    font-size: 0.75rem;
    text-overflow: ellipsis;
  }
  tr.disabled td:first-child {
    color: var(--muted);
  }
  .events {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.82rem;
  }
  .events li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.9rem;
    padding-left: 0.7rem;
    border-left: 3px solid var(--line);
  }
  .events time {
    min-width: 11rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .kind {
    font-weight: 600;
  }
  .events .boot,
  .events .wake {
    border-left-color: var(--ok);
  }
  .events .sleep,
  .events .shutdown {
    border-left-color: var(--accent-cpu);
  }
  .events .unexpectedShutdown {
    border-left-color: var(--hot);
  }
  .muted {
    color: var(--muted);
  }
  .note {
    margin: 0.6rem 0 0;
    font-size: 0.75rem;
  }
</style>
