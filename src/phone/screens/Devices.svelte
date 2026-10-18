<script lang="ts">
  // The PC's devices (1.5): what is connected, and batteries, low ones first. A phone allowed to
  // control the PC can connect and disconnect its paired Bluetooth devices.
  import { actions, batteryView, connectionText, grouped, KIND_LABEL, lowNotice, stateText } from '../../devices';
  import type { DeviceInfo, Devices, LinkState } from '../../types';
  import { errText, every, type ScreenProps } from './ask';

  let { ask, can, pc }: ScreenProps = $props();

  let data = $state.raw<Devices | null>(null);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let result = $state<Record<string, string>>({});

  function load(): void {
    ask<Devices>('devices')
      .then((v) => {
        data = v;
        error = null;
      })
      .catch((e: unknown) => (error = errText(e)));
  }
  $effect(() => every(30_000, load));

  const groups = $derived(data ? grouped(data.devices) : []);
  const low = $derived(data ? lowNotice(data.devices) : null);

  async function act(d: DeviceInfo, connect: boolean): Promise<void> {
    if (d.control == null) return;
    busy = d.id;
    result = { ...result, [d.id]: connect ? 'Connecting…' : 'Disconnecting…' };
    try {
      const seen = await ask<LinkState>('device.connect', { id: d.id, control: d.control, connect });
      const want = connect ? 'connected' : 'notConnected';
      result = {
        ...result,
        [d.id]: seen === want ? (connect ? 'Connected' : 'Disconnected') : connect ? "Didn't connect" : "Didn't disconnect",
      };
      load();
    } catch (e) {
      result = { ...result, [d.id]: errText(e) };
    } finally {
      busy = null;
    }
  }
</script>

<section class="panel" aria-labelledby="dev-title">
  <h1 id="dev-title">Devices on {pc}</h1>
  {#if low}
    <p class="note {low.tone === 'fail' ? 'hot' : 'warn'}" role="status"><b>Low battery:</b> {low.text}</p>
  {/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if !data}
    <p class="muted" role="status">Reading…</p>
  {:else}
    {#each groups as g (g.id)}
      <h2 id="dev-{g.id}">{g.title}</h2>
      <ul class="rows" aria-labelledby="dev-{g.id}">
        {#each g.devices as d (d.id)}
          {@const b = batteryView(d)}
          <li class:off={d.state === 'notConnected'}>
            <div class="what">
              <b>{d.name}</b>
              <span class="muted small">
                {[KIND_LABEL[d.kind], connectionText(d, data.devices)].filter(Boolean).join(' · ')} · {stateText(d)}
              </span>
              {#if result[d.id]}<span class="small" role="status">{result[d.id]}</span>{/if}
            </div>
            <span class="battery {b.tone}">{b.text}{b.charging ? ' ⚡' : ''}</span>
            {#if can('device.connect')}
              {#each actions(d) as label (label)}
                <button
                  type="button"
                  class="ghost"
                  disabled={busy != null}
                  onclick={() => void act(d, label === 'Connect')}
                  aria-label="{label} {d.name}">{label}</button
                >
              {/each}
            {/if}
          </li>
        {/each}
      </ul>
    {/each}
  {/if}
</section>

<style>
  h2 {
    margin: 0.9rem 0 0.35rem;
  }
  .rows {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .rows li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--line);
  }
  .rows li.off .what b {
    color: var(--muted);
  }
  .what {
    display: grid;
    flex: 1;
    min-width: 10rem;
  }
  .battery {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .battery.warn {
    color: var(--warn);
  }
  .battery.hot {
    color: var(--hot);
  }
  .rows button {
    padding: 0.35rem 0.75rem;
    font-size: 0.85rem;
  }
  .note.hot {
    padding: 0.5rem 0.7rem;
    border-left: 3px solid var(--hot);
    background: color-mix(in srgb, var(--hot) 12%, transparent);
  }
</style>
