<script lang="ts">
  import Card from './Card.svelte';
  import CardGrid, { type CardItem } from './CardGrid.svelte';
  import KV from './KV.svelte';
  import LoadState from './LoadState.svelte';
  import Meter from './Meter.svelte';
  import Spark from './Spark.svelte';
  import { fmt, NA } from '../format';
  import { getNetwork, setPingHost } from '../ipc';
  import type { NetAdapterInfo, NetworkSnapshot, PingSnapshot } from '../types';

  interface Props {
    ts: number;
    live: NetworkSnapshot[];
    ping: PingSnapshot | null;
  }
  let { ts, live, ping }: Props = $props();

  const HOST_KEY = 'vyro.pingHost';

  let adapters = $state.raw<NetAdapterInfo[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let revealed = $state<Record<string, boolean>>({});
  let hostInput = $state('');
  let hostError = $state<string | null>(null);

  function load(): void {
    loading = true;
    getNetwork()
      .then((v) => {
        adapters = v;
        error = null;
      })
      .catch((e: unknown) => (error = String(e)))
      .finally(() => (loading = false));
  }

  $effect(() => {
    load();
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(HOST_KEY);
    } catch {
      /* storage unavailable */
    }
    if (saved) {
      hostInput = saved;
      void setPingHost(saved).catch(() => {});
    }
  });

  // Re-read configuration when adapters come, go, connect or disconnect, or the primary moves.
  const liveKey = $derived(live.map((n) => `${n.id}:${n.connected}:${n.primary}`).join(','));
  let seenKey = '';
  $effect(() => {
    const k = liveKey;
    if (seenKey !== '' && k !== seenKey && !loading) load();
    seenKey = k;
  });

  const byId = $derived(new Map(live.map((n) => [n.id, n])));

  async function applyHost(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    const host = hostInput.trim() || '1.1.1.1';
    try {
      await setPingHost(host);
      hostError = null;
      try {
        localStorage.setItem(HOST_KEY, host);
      } catch {
        /* not remembered */
      }
    } catch (err: unknown) {
      hostError = err instanceof Error ? err.message : String(err);
    }
  }

  function mbps(v: number | null | undefined): string {
    if (v == null) return NA;
    if (v >= 1000) return `${fmt(v / 1000, 2)} Gbps`;
    if (v >= 1) return `${fmt(v, v >= 100 ? 0 : 1)} Mbps`;
    return `${fmt(v * 1000)} Kbps`;
  }

  function link(v: number | null | undefined): string | null {
    if (v == null) return null;
    return v >= 1000 ? `${fmt(v / 1000, v % 1000 === 0 ? 0 : 1)} Gbps` : `${fmt(v)} Mbps`;
  }

  function volume(mb: number | null | undefined): string | null {
    if (mb == null) return null;
    return mb >= 1000 ? `${fmt(mb / 1000, 1)} GB` : `${fmt(mb)} MB`;
  }

  function secret(key: string, value: string | null | undefined): string | null {
    if (value == null) return null;
    return revealed[key] ? value : '•'.repeat(Math.min(value.length, 12));
  }

  function toggle(key: string): void {
    revealed[key] = !revealed[key];
  }

  const KIND: Record<string, string> = { ethernet: 'Ethernet', wifi: 'Wi-Fi', other: 'Other' };
  // Ping, then one card per adapter, in Vyro's order or the owner's (2.0.2).
  const cards = $derived<CardItem[]>([
    { id: 'net-ping', title: 'Ping' },
    ...(adapters ?? []).map((a) => ({ id: `net:${a.id}`, title: `${KIND[a.type] ?? a.type}: ${a.name}` })),
  ]);
</script>

<CardGrid view="network" items={cards} min="20rem">
  {#snippet card(id)}
    {#if id === 'net-ping'}
      <Card title="Ping" id="net-ping" subtitle={ping ? `${ping.host}` : NA}>
        <form class="host" onsubmit={applyHost}>
          <label for="ping-host" class="muted small">Target</label>
          <input id="ping-host" bind:value={hostInput} placeholder={ping?.host ?? '1.1.1.1'} spellcheck="false" autocomplete="off" />
          <button type="submit">Apply</button>
        </form>
        {#if hostError}<p class="error small" role="alert">{hostError}</p>{/if}
        <div class="big">
          <b>{ping?.rttMs != null ? fmt(ping.rttMs, ping.rttMs < 10 ? 1 : 0) : NA}</b><small>{ping?.rttMs != null ? 'ms' : ''}</small>
          <span class="loss" class:bad={(ping?.lossPct ?? 0) > 0}>{ping?.lossPct != null ? `${fmt(ping.lossPct, 1)}% loss` : ''}</span>
        </div>
        <Spark {ts} series={[{ value: ping?.rttMs, color: 'var(--accent-cpu)' }]} floor={20} label="Ping, last minute" />
        <KV
          rows={[
            ['Average', ping?.avgMs != null ? `${fmt(ping.avgMs, 1)} ms` : null],
            ['Min / max', ping?.minMs != null ? `${fmt(ping.minMs, 1)} / ${fmt(ping.maxMs, 1)} ms` : null],
            ['Jitter', ping?.jitterMs != null ? `${fmt(ping.jitterMs, 1)} ms` : null],
            ['Window', ping ? `${ping.samples} probes (1 per second)` : null],
            ['Last error', ping?.error],
          ]}
        />
      </Card>
    {:else}
      {@const a = (adapters ?? []).find((x) => `net:${x.id}` === id)}
      {#if a}
          {@const n = byId.get(a.id)}
          {@const w = a.wifi}
          <Card title="{KIND[a.type] ?? a.type}{a.primary ? ' · Primary' : ''}{a.virtualAdapter ? ' · Virtual' : ''}" {id} subtitle={a.name}>
            <p class="meta">{a.description}</p>
            <div class="badges">
              <span class="state" class:up={n?.connected ?? a.connected}>{(n?.connected ?? a.connected) ? 'Connected' : 'Disconnected'}</span>
              {#if link(n?.linkMbps ?? a.linkMbps)}<span class="state">{link(n?.linkMbps ?? a.linkMbps)}</span>{/if}
              {#if n?.wifiSignalPct != null}<span class="state">{fmt(n.wifiSignalPct)}% signal</span>{/if}
            </div>

            <h3>Throughput</h3>
            <div class="rates">
              <div><span class="muted">Download</span><b>{mbps(n?.downMbps)}</b></div>
              <div><span class="muted">Upload</span><b>{mbps(n?.upMbps)}</b></div>
            </div>
            <Spark
              {ts}
              series={[
                { value: n?.downMbps, color: 'var(--accent-cpu)' },
                { value: n?.upMbps, color: 'var(--accent-gpu)' },
              ]}
              label="{a.name} download and upload, last minute"
            />
            <div class="meter-row"><Meter label="Utilization" value={n?.utilizationPct} /></div>
            <KV
              rows={[
                ['Received', volume(n?.receivedMB)],
                ['Sent', volume(n?.sentMB)],
              ]}
            />

            {#if w}
              <h3>Wi-Fi</h3>
              <div class="meter-row"><Meter label="Signal" value={n?.wifiSignalPct ?? w.signalPct} /></div>
              <KV
                rows={[
                  ['Network', secret(`${a.id}.ssid`, w.ssid)],
                  ['Signal strength', (n?.wifiRssiDbm ?? w.rssiDbm) != null ? `${fmt(n?.wifiRssiDbm ?? w.rssiDbm)} dBm` : null],
                  ['Band / channel', [n?.wifiBand ?? w.band, (n?.wifiChannel ?? w.channel) != null ? `channel ${n?.wifiChannel ?? w.channel}` : null].filter(Boolean).join(' · ') || null],
                  ['Standard', w.standard],
                  ['Link rate', w.rxMbps != null ? `${fmt(w.rxMbps)} ↓ / ${fmt(w.txMbps)} ↑ Mbps` : null],
                  ['Security', w.security],
                  ['Access point', secret(`${a.id}.bssid`, w.bssid)],
                ]}
              />
              {#if w.ssid || w.bssid}
                <button type="button" class="reveal" onclick={() => { toggle(`${a.id}.ssid`); toggle(`${a.id}.bssid`); }}>
                  {revealed[`${a.id}.ssid`] ? 'Hide network name' : 'Show network name'}
                </button>
              {/if}
              {#if w.note}<p class="muted small">{w.note}.</p>{/if}
            {/if}

            <h3>Addresses</h3>
            <KV
              rows={[
                ['IPv4', a.ipv4.join(', ') || null],
                ['IPv6', a.ipv6.length ? (revealed[`${a.id}.ip6`] ? a.ipv6.join(', ') : `${a.ipv6.length} hidden`) : null],
                ['Gateway', a.gateways.join(', ') || null],
                ['DNS', a.dns.join(', ') || null],
                ['DHCP', a.dhcp == null ? null : a.dhcp ? 'On' : 'Off (static)'],
                ['MTU', a.mtu],
                ['MAC', secret(`${a.id}.mac`, a.mac)],
              ]}
            />
            {#if a.ipv6.length || a.mac}
              <button type="button" class="reveal" onclick={() => { toggle(`${a.id}.ip6`); toggle(`${a.id}.mac`); }}>
                {revealed[`${a.id}.mac`] ? 'Hide MAC / IPv6' : 'Show MAC / IPv6'}
              </button>
            {/if}
          </Card>
      {/if}
    {/if}
  {/snippet}
</CardGrid>
{#if error || !adapters || adapters.length === 0}
  <LoadState
    what="network adapters"
    {error}
    loading={!adapters || loading}
    empty="No network adapters found."
    retry={load}
  />
{/if}

<style>
  .host {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.6rem;
  }
  input {
    flex: 1;
    min-width: 0;
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg, transparent);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
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
  .reveal {
    margin-top: 0.5rem;
  }
  .big {
    display: flex;
    align-items: baseline;
    gap: 0.3rem;
    margin-bottom: 0.3rem;
  }
  .big b {
    font-family: var(--font-num);
    font-size: 2rem;
    font-variant-numeric: tabular-nums;
  }
  .big small {
    color: var(--muted);
  }
  .loss {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .loss.bad {
    color: var(--warn);
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
  .state {
    padding: 0.05rem 0.55rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .state.up {
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
    color: var(--ok);
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
  }
  .rates div {
    display: flex;
    flex-direction: column;
  }
  .rates span {
    font-size: 0.72rem;
  }
  .rates b {
    font-family: var(--font-num);
    font-size: 1.05rem;
    font-variant-numeric: tabular-nums;
  }
  .rates div:first-child b {
    color: var(--accent-cpu);
  }
  .rates div:last-child b {
    color: var(--accent-gpu);
  }
  .meter-row {
    margin-bottom: 0.6rem;
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
