<script lang="ts">
  // The PC's devices (1.5; in More from 1.7): what is connected, and batteries, low ones first. A
  // phone allowed to control the PC can connect and disconnect its paired Bluetooth devices.
  import { actions, batteryView, connectionText, grouped, KIND_LABEL, lowNotice, stateText } from '../../devices';
  import type { DeviceInfo, Devices, LinkState } from '../../types';
  import { errText, every, type ScreenProps } from './ask';

  let { ask, can, pc }: ScreenProps = $props();

  let data = $state.raw<Devices | null>(null);
  let error = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let result = $state<Record<string, string>>({});

  function load(): Promise<unknown> {
    return ask<Devices>('devices')
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
      void load();
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
  <!-- One answer failed: the list it last gave stays under the error. -->
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if !data}
    {#if !error}<div class="skeleton" role="status" aria-label="Reading…"><span></span><span></span><span></span></div>{/if}
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
            {#if b.text}
              <span class="battery {b.tone}">
                {#if b.pct != null}<span class="cell" aria-hidden="true"><i style:width="{b.pct}%"></i></span>{/if}
                {b.text}{b.charging ? ' ⚡' : ''}
              </span>
            {/if}
            {#if can('device.connect')}
              <div class="acts">
                {#each actions(d) as label (label)}
                  <button
                    type="button"
                    class="ghost"
                    disabled={busy != null}
                    onclick={() => void act(d, label === 'Connect')}
                    aria-label="{label} {d.name}">{label}</button
                  >
                {/each}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/each}
  {/if}
</section>

<style>
  h2 {
    margin: 1rem 0 0.25rem;
  }
  .rows li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
  }
  .rows li.off .what b {
    color: var(--muted);
  }
  .what {
    display: grid;
    flex: 1 1 12rem;
    gap: 0.1rem;
    min-width: 0;
  }
  .battery {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 0.4rem;
    font-variant-numeric: tabular-nums;
  }
  .cell {
    display: block;
    width: 1.6rem;
    height: 0.75rem;
    padding: 1px;
    border: 1.5px solid currentColor;
    border-radius: 3px;
  }
  .cell i {
    display: block;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
  }
  .battery.warn {
    color: var(--warn);
  }
  .battery.hot {
    color: var(--hot);
  }
  .acts {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .acts button {
    padding: 0.35rem 0.8rem;
    font-size: 0.85rem;
  }
</style>
