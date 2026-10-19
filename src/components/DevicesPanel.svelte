<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import LoadState from './LoadState.svelte';
  import {
    actions,
    batteryView,
    connectionText,
    grouped,
    KIND_LABEL,
    lowBattery,
    oneAtATime,
    outcome,
    ownerName,
    stateText,
    summary,
    type Pending,
  } from '../devices';
  import { getDevices, onDevicesRead, setDeviceConnected } from '../ipc';
  import type { DeviceInfo, Devices } from '../types';

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

  function show(v: Devices): void {
    data = v;
    error = null;
    readAt = new Date();
  }

  /** Reads (opening, the minute, after an action) one at a time, every one shown. */
  const load = oneAtATime(
    getDevices,
    show,
    (e) => (error = String(e)),
    (b) => (loading = b),
  );

  $effect(() => {
    if (!active) return;
    untrack(load);
    const timer = setInterval(load, REFRESH_MS);
    return () => clearInterval(timer);
  });

  // Hot add/remove: the background reader's list, read after the change.
  $effect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void onDevicesRead(show).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  // Connect and Disconnect (1.3): what was asked of each device, until its outcome has shown a
  // while. The engine waits for Windows to show the change and answers with what it saw
  // (1.4.1); a read a few seconds later catches a device that connects again by itself.
  let pending = $state<Record<string, Pending>>({});
  /** How long an outcome shows, and when the read after it is. */
  const SHOW_MS = 20_000;
  const RECHECK_MS = 5000;
  let clicks = 0;

  async function act(d: DeviceInfo, want: boolean): Promise<void> {
    const control = d.control;
    const was = pending[d.id];
    if (control == null || (was && was.seen == null && was.error == null)) return;
    const id = d.id;
    const token = ++clicks;
    // Only this click's own answers and timers touch its entry.
    const update = (patch: Partial<Pending>) => {
      const cur = pending[id];
      if (cur?.token === token) pending[id] = { ...cur, ...patch };
    };
    pending[id] = { want, control, token, seen: null, error: null };
    try {
      update({ seen: await setDeviceConnected(id, control, want) });
      load();
      setTimeout(load, RECHECK_MS);
    } catch (e) {
      update({ error: String(e) });
    }
    setTimeout(() => {
      if (pending[id]?.token === token) delete pending[id];
    }, SHOW_MS);
  }

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
            {@const labels = actions(d)}
            {@const p = pending[d.id]}
            {@const result = p ? outcome(p, d) : null}
            <li class:off={d.state === 'notConnected'}>
              <div class="what">
                <b>{d.name}</b>
                <span>{[KIND_LABEL[d.kind], connectionText(d, data.devices), d.manufacturer].filter(Boolean).join(' · ')}</span>
                {#if labels.length > 0}
                  <span class="outcome" role="status">{p ? (result ?? (p.want ? 'Connecting…' : 'Disconnecting…')) : ''}</span>
                {/if}
              </div>
              <span
                class="state"
                class:ok={state === 'Connected'}
                class:bad={d.connected && state !== 'Connected'}
                title={d.state === 'unknown' ? "Paired: Windows doesn't say whether it is connected" : undefined}
              >
                {state}
              </span>
              <span
                class="battery {b.tone}"
                title={b.source ? `Battery ${b.text}${b.charging ? ', charging' : ''}, ${b.source}` : 'No battery reading'}
              >
                <span class="sr-only">Battery</span>
                {#if b.pct != null}
                  <span class="bar" aria-hidden="true"><i style:width="{b.pct}%"></i></span>
                {/if}
                <span class="num">{b.text}</span>
                {#if b.charging}<span class="charging">charging</span>{/if}
                {#if b.source}<span class="sr-only">, {b.source}</span>{:else}<span class="sr-only">, not reported</span>{/if}
              </span>
              <span class="act">
                {#each labels as label (label)}
                  <button
                    type="button"
                    onclick={() => void act(d, label === 'Connect')}
                    disabled={p != null && result == null}
                    aria-label="{label} {d.name}"
                  >
                    {label}
                  </button>
                {/each}
              </span>
            </li>
          {/each}
        </ul>
      {/each}
      <p class="note">
        Battery shows where Windows, Bluetooth LE, XInput, a Logitech receiver or a PlayStation controller reports it;
        other makers' 2.4 GHz receivers keep it to their own software. Monitors are on the Displays tab.
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
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr));
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
    grid-template-columns: minmax(0, 1fr) auto minmax(7.5rem, auto) 6.5rem;
    align-items: center;
    gap: 0.4rem 1rem;
    padding: 0.55rem 0;
    border-top: 1px solid var(--line);
  }
  .endpoints li {
    grid-template-columns: minmax(0, 1fr) auto;
  }
  .act {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    justify-content: flex-end;
  }
  .what .outcome {
    color: var(--text);
  }
  .what .outcome:empty {
    display: none;
  }
  /* A narrow window: battery and the button go under the name. */
  @media (max-width: 640px) {
    .devices li {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .devices .battery {
      justify-content: flex-start;
    }
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
  .battery .charging {
    color: var(--muted);
    font-size: 0.72rem;
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
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 16rem), 1fr));
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
