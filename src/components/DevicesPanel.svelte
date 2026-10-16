<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import LoadState from './LoadState.svelte';
  import { batteryView, connectionText, grouped, KIND_LABEL, lowBattery, ownerName, stateText, summary } from '../devices';
  import { getDevices, onDevicesChanged } from '../ipc';
  import type { Devices } from '../types';

  interface Props {
    /** The tab shows: read on opening and once a minute (battery) while it does. */
    active: boolean;
  }
  let { active }: Props = $props();

  /** Battery refresh while the tab shows (P22 default). */
  const REFRESH_MS = 60_000;

  let data = $state.raw<Devices | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);
  let readAt = $state<Date | null>(null);

  function load(): void {
    loading = true;
    getDevices()
      .then((v) => {
        data = v;
        error = null;
        readAt = new Date();
      })
      .catch((e: unknown) => (error = String(e)))
      .finally(() => (loading = false));
  }

  $effect(() => {
    if (!active) return;
    untrack(load);
    const timer = setInterval(load, REFRESH_MS);
    return () => clearInterval(timer);
  });

  // Hot add/remove: read again at once while the tab shows; otherwise opening it reads anyway.
  $effect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void onDevicesChanged(() => {
      if (active) load();
    }).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  const groups = $derived(data ? grouped(data.devices) : []);
  const low = $derived(data ? lowBattery(data.devices) : []);
  const outputs = $derived(data?.audio.filter((a) => a.output) ?? []);
  const inputs = $derived(data?.audio.filter((a) => !a.output) ?? []);

  function time(d: Date | null): string {
    return d ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';
  }
</script>

{#if error || !data}
  <LoadState what="devices" {error} loading={!data || loading} retry={load} />
{:else}
  <div class="grid">
    <Card title="Devices" subtitle={summary(data)} wide>
      <div class="toolbar">
        <button type="button" onclick={load} disabled={loading}>{loading ? 'Refreshing…' : 'Refresh'}</button>
        <span class="hint">
          Updates when a device connects or disconnects; battery every minute{readAt ? ` · read ${time(readAt)}` : ''}.
        </span>
      </div>

      {#if low.length > 0}
        <p class="low" role="status">
          <b>Low battery:</b>
          {low.map((d) => `${d.name} ${batteryView(d).text}`).join(', ')}
        </p>
      {/if}

      {#if groups.length === 0}
        <LoadState what="devices" error={null} empty="No peripherals found. Monitors are on the Displays tab." retry={load} />
      {/if}

      {#each groups as g (g.id)}
        <h3 id="devices-{g.id}">{g.title}</h3>
        <ul class="devices" aria-labelledby="devices-{g.id}">
          {#each g.devices as d (d.id)}
            {@const b = batteryView(d)}
            {@const state = stateText(d)}
            <li class:off={!d.connected}>
              <div class="what">
                <b>{d.name}</b>
                <span>{[KIND_LABEL[d.kind], connectionText(d), d.manufacturer].filter(Boolean).join(' · ')}</span>
              </div>
              <span class="state" class:ok={state === 'Connected'} class:bad={d.connected && state !== 'Connected'}>{state}</span>
              <span class="battery {b.tone}" title={b.source ? `Battery ${b.text}, ${b.source}` : 'No battery reading'}>
                <span class="sr-only">Battery</span>
                {#if b.pct != null}
                  <span class="bar" aria-hidden="true"><i style:width="{b.pct}%"></i></span>
                {/if}
                <span class="num">{b.text}</span>
                {#if b.source}<span class="sr-only">, {b.source}</span>{:else}<span class="sr-only">, not reported</span>{/if}
              </span>
            </li>
          {/each}
        </ul>
      {/each}
      <p class="note">
        Battery shows where the device reports it to Windows, over Bluetooth LE or XInput; 2.4 GHz receivers keep it to
        their own software. Monitors are on the Displays tab.
      </p>
    </Card>

    <Card title="Sound" subtitle="Playback and recording" wide>
      <div class="sound">
        {#each [{ id: 'out', title: 'Playback', list: outputs }, { id: 'in', title: 'Recording', list: inputs }] as s (s.id)}
          <div>
            <h3 id="sound-{s.id}">{s.title}</h3>
            {#if s.list.length === 0}
              <p class="none">None</p>
            {:else}
              <ul class="endpoints" aria-labelledby="sound-{s.id}">
                {#each s.list as a (a.id)}
                  {@const owner = ownerName(data, a.device)}
                  <li class:off={!a.plugged}>
                    <div class="what">
                      <b>{a.name}</b>
                      <span>{owner ?? 'This PC'}{a.plugged ? '' : ' · nothing plugged in'}</span>
                    </div>
                    {#if a.default}<span class="state ok">Default</span>{/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/each}
      </div>
    </Card>
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
    margin-bottom: 0.75rem;
  }
  .hint,
  .note,
  .none {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .note {
    margin: 0.9rem 0 0;
  }
  .none {
    margin: 0;
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
  .low {
    margin: 0 0 0.75rem;
    padding: 0.5rem 0.75rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 8px;
    font-size: 0.85rem;
  }
  .low b {
    color: var(--warn);
  }
  h3 {
    margin: 1rem 0 0.4rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .toolbar + h3,
  .low + h3,
  .sound h3 {
    margin-top: 0.25rem;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 7.5rem;
    align-items: center;
    gap: 0.4rem 1rem;
    padding: 0.55rem 0;
    border-top: 1px solid var(--line);
  }
  .endpoints li {
    grid-template-columns: minmax(0, 1fr) auto;
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .what b {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .what span {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .off .what b {
    color: var(--muted);
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
  .state.ok {
    border-color: color-mix(in srgb, var(--ok) 45%, transparent);
    color: var(--ok);
  }
  .state.bad {
    border-color: color-mix(in srgb, var(--hot) 45%, transparent);
    color: var(--hot);
  }
  .battery {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.45rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .battery .num {
    min-width: 3.2em;
    text-align: right;
    font-size: 0.85rem;
    font-weight: 600;
  }
  .bar {
    position: relative;
    width: 2.6rem;
    height: 0.6rem;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 3px;
  }
  .bar i {
    position: absolute;
    inset: 0 auto 0 0;
    background: currentColor;
  }
  .battery.ok {
    color: var(--ok);
  }
  .battery.warn {
    color: var(--warn);
  }
  .battery.hot {
    color: var(--hot);
  }
  .sound {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
    gap: 0.5rem 2rem;
  }
  @media (max-width: 30rem) {
    li {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .battery {
      grid-column: 1 / -1;
      justify-content: flex-start;
    }
    /* No reading: no line of its own on a phone-width window. */
    .battery.none {
      display: none;
    }
  }
</style>
