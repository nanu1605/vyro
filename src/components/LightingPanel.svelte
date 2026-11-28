<script lang="ts">
  // Ambient lighting (2.6; the owner: "Whatever is glowing in the screen ... the RGB of the
  // keyboard or mouse or any other devices ... should change automatically with the colour which
  // is glowing most on the screen"). The colour glowing most on the screen, on the devices picked,
  // through OpenRGB. The colour and OpenRGB's state come live while the view is open.
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import CardGrid, { type CardItem } from './CardGrid.svelte';
  import LoadState from './LoadState.svelte';
  import More from './More.svelte';
  import { getLighting, onLightingStatus, setLighting, setUpOpenRgb, watchLighting, type LightingView } from '../ipc';
  import type { LightingSettings, LightingStatus, LightSpeed, NoColour, Rgb8 } from '../types';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  let lv = $state.raw<LightingView | null>(null);
  let status = $state.raw<LightingStatus | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  // Set up OpenRGB (2.6.1): pressed, and what came of it.
  let settingUp = $state(false);
  let setupNote = $state<string | null>(null);

  const CARDS: CardItem[] = [
    { id: 'light-main', title: 'Ambient lighting', w: 2 },
    { id: 'light-devices', title: 'Devices' },
    { id: 'light-look', title: 'How it follows the screen' },
  ];

  const SPEEDS: Array<{ id: LightSpeed; label: string; text: string }> = [
    { id: 'auto', label: 'Auto', text: 'Fast while a game is in front, smooth otherwise.' },
    { id: 'fast', label: 'Fast', text: 'About a tenth of a second: games.' },
    { id: 'smooth', label: 'Smooth', text: 'About half a second: films and videos.' },
  ];
  const NO_COLOUR: Array<{ id: NoColour; label: string }> = [
    { id: 'white', label: 'White' },
    { id: 'accent', label: 'Clokrix blue' },
    { id: 'off', label: 'Off' },
  ];

  async function load(): Promise<void> {
    try {
      lv = await getLighting();
      status = lv.status;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function change(patch: Partial<LightingSettings>): Promise<void> {
    if (!lv) return;
    busy = true;
    try {
      lv = await setLighting({ ...lv.settings, ...patch });
      status = lv.status;
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function setUp(): Promise<void> {
    settingUp = true;
    setupNote = null;
    try {
      setupNote = await setUpOpenRgb();
      error = null;
    } catch (e) {
      setupNote = String(e);
    } finally {
      settingUp = false;
      status = await watchLighting().catch(() => status);
    }
  }

  function pick(id: string, on: boolean): void {
    if (!lv) return;
    const devices = lv.settings.devices.filter((d) => d !== id);
    if (on) devices.push(id);
    void change({ devices });
  }

  // Read on opening; while open, the colour and OpenRGB's state come every quarter second, asked
  // for again every 2 s.
  $effect(() => {
    if (!active) return;
    untrack(() => void load());
    const ask = () => void watchLighting().then((s) => (status = s)).catch(() => {});
    ask();
    const timer = setInterval(ask, 2000);
    let stop: (() => void) | undefined;
    let disposed = false;
    void onLightingStatus((s) => (status = s)).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      clearInterval(timer);
      stop?.();
    };
  });

  // "A", "A and B", "A, B and C".
  const list = (items: string[]) => (items.length < 2 ? (items[0] ?? '') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`);
  const hex = (c: Rgb8) => '#' + [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, '0')).join('');
  const settings = $derived(lv?.settings);
  const speed = $derived(SPEEDS.find((s) => s.id === settings?.speed));
  const picked = $derived(status?.devices.filter((d) => d.picked && d.direct).length ?? 0);
</script>

{#if !lv || !settings}
  <LoadState what="lighting" {error} loading={!error} retry={() => void load()} />
{:else}
  {@const monitors = lv.monitors}
  <CardGrid view="lighting" items={CARDS} min="20rem">
    {#snippet card(id)}
      {#if id === 'light-main'}
        <Card title="Ambient lighting" id="light-main" subtitle="The colour glowing most on the screen, on your RGB devices">
          <label class="check">
            <input
              type="checkbox"
              class="switch"
              role="switch"
              checked={settings.on}
              disabled={busy}
              onchange={(e) => void change({ on: e.currentTarget.checked })}
            />
            <span>Light my devices with the screen's colour</span>
          </label>
          {#if settings.on && status}
            <div class="now">
              <span class="swatch" style:background={status.colour ? hex(status.colour) : 'transparent'} aria-hidden="true"></span>
              <div>
                {#if status.link === 'missing' && status.offer}
                  <p><b>OpenRGB isn't on this PC.</b> Clokrix drives your lights through it.</p>
                  <p class="note">
                    Set up OpenRGB {status.offer} (free, open source): Clokrix downloads it from OpenRGB's release page, checks
                    it is the release it knows, and installs it after one Windows prompt for an administrator. It then runs in
                    the background, starting with Windows. Remove it any time from Windows' Apps.
                  </p>
                  <button type="button" class="primary" disabled={settingUp} onclick={() => void setUp()}>Set up OpenRGB</button>
                {:else if status.link === 'settingUp'}
                  <p role="status"><b>{status.setup === 'installing' ? 'Installing OpenRGB…' : 'Downloading OpenRGB…'}</b></p>
                  <p class="note">
                    {status.setup === 'installing' ? 'Answer the Windows prompt for an administrator.' : 'About 23 MB.'}
                  </p>
                {:else if status.link === 'starting'}
                  <p role="status"><b>Starting OpenRGB…</b></p>
                  <p class="note">It looks for every device first, which can take a minute.</p>
                {:else if status.link === 'missing' || status.link === 'notRunning'}
                  <p><b>OpenRGB is not answering.</b></p>
                  <p class="note">
                    Install OpenRGB (free, from <code>openrgb.org</code>) and open it. In its <b>SDK Server</b> tab, choose
                    <b>Start Server</b>, and in its settings start it with Windows, minimised. Clokrix looks for it every 5
                    seconds.
                  </p>
                {:else if status.colour}
                  <p><b>{hex(status.colour)}</b> now{picked === 0 ? ', on no device yet: pick them below' : ` on ${picked} device${picked === 1 ? '' : 's'}`}.</p>
                  {#if status.held}<p class="note">Holding its colour: {status.held}.</p>{/if}
                  {#if status.capture}<p class="note">The screen is read through {status.capture}.</p>{/if}
                {:else}
                  <p class="note">Starting…</p>
                {/if}
                {#if setupNote}<p class="note" role="status">{setupNote}</p>{/if}
              </div>
            </div>
            <!-- 2.6.2: another lighting program writes over OpenRGB's colours. Named, never touched. -->
            {#each status.others as o (o.name)}
              <div class="other" role="status">
                <p><b>{o.name} is running</b> and takes your lights back from OpenRGB, so they don't change.</p>
                <p class="note">
                  {#if o.services.length > 0}
                    In Windows' Services (Win+R, then <code>services.msc</code>), set {list(o.services)} to Manual and stop
                    {o.services.length === 1 ? 'it' : 'them'}, or {o.remedy}.
                  {:else}
                    Quit {list(o.processes)}, or {o.remedy}.
                  {/if}
                  Then restart the PC. Clokrix never stops another program itself.
                </p>
              </div>
            {/each}
          {/if}
          <div class="row">
            <p class="note">
              Each picture of the screen is made small on the graphics card and kept only as one colour: nothing of it is
              saved or leaves this PC. Close the lighting in iCUE, Synapse, G HUB, Armoury Crate, MSI Center or SignalRGB, or
              they fight OpenRGB for the devices.
            </p>
            <More about="ambient lighting">
              The colour is the one glowing most: vivid colours count, grey, white and black do not, so a red sign in a dark
              game lights red. It follows the screen's brightness, and never flashes more than 3 times a second whatever the
              screen does. Devices go into OpenRGB's Direct mode while lighting is on, and back to their own mode when it goes
              off, when Clokrix closes, or at its next start after a crash; nothing is saved to a device.
            </More>
          </div>
        </Card>
      {:else if id === 'light-devices'}
        <Card title="Devices" id="light-devices" subtitle={status?.link === 'connected' ? 'From OpenRGB' : null}>
          {#if !settings.on}
            <p class="note">Turn ambient lighting on to see OpenRGB's devices.</p>
          {:else if !status || status.devices.length === 0}
            <p class="note">{status?.link === 'connected' ? 'OpenRGB has no devices.' : status?.link === 'missing' ? 'Once OpenRGB is set up, its devices are listed here.' : 'Waiting for OpenRGB.'}</p>
          {:else}
            <ul class="devices">
              {#each status.devices as d (d.id)}
                <li>
                  <label class="check">
                    <input type="checkbox" checked={d.picked} disabled={busy || !d.direct} onchange={(e) => pick(d.id, e.currentTarget.checked)} />
                    <span>
                      {d.name}
                      <small>
                        {d.kind} · {d.leds} LED{d.leds === 1 ? '' : 's'}{d.slow ? ' · on the memory bus: 5 colours a second at most' : ''}{d.direct
                          ? ''
                          : ' · no Direct mode in OpenRGB, so it cannot follow the screen'}{d.empty.length > 0
                          ? ` · ${list(d.empty)} ${d.empty.length === 1 ? 'has' : 'have'} no LEDs set: in OpenRGB, Resize ${d.empty.length === 1 ? 'it' : 'them'} to your fans' or strip's LED count`
                          : ''}
                      </small>
                    </span>
                  </label>
                </li>
              {/each}
            </ul>
          {/if}
        </Card>
      {:else if id === 'light-look'}
        <Card title="How it follows the screen" id="light-look">
          <div class="field">
            <span class="label" id="light-speed">Speed</span>
            <div class="segmented" role="group" aria-labelledby="light-speed">
              {#each SPEEDS as s (s.id)}
                <button type="button" aria-pressed={settings.speed === s.id} disabled={busy} onclick={() => void change({ speed: s.id })}>{s.label}</button>
              {/each}
            </div>
            {#if speed}<p class="note">{speed.text}</p>{/if}
          </div>
          <label class="slider">
            <span>Brightness</span>
            <input
              type="range"
              min="10"
              max="100"
              step="5"
              value={settings.brightness}
              disabled={busy}
              onchange={(e) => void change({ brightness: Number(e.currentTarget.value) })}
            />
            <output>{settings.brightness}%</output>
          </label>
          <div class="field">
            <span class="label" id="light-none">When the screen has no colour</span>
            <div class="segmented" role="group" aria-labelledby="light-none">
              {#each NO_COLOUR as n (n.id)}
                <button type="button" aria-pressed={settings.noColour === n.id} disabled={busy} onclick={() => void change({ noColour: n.id })}>{n.label}</button>
              {/each}
            </div>
            <p class="note">A document, the desktop, a black screen.</p>
          </div>
          {#if monitors > 1}
            <label class="field inline">
              <span class="label">Monitor</span>
              <select value={String(settings.monitor)} disabled={busy} onchange={(e) => void change({ monitor: Number(e.currentTarget.value) })}>
                {#each Array.from({ length: monitors }, (_, i) => i) as i (i)}
                  <option value={String(i)}>{i === 0 ? 'The main monitor' : `Monitor ${i + 1}`}</option>
                {/each}
              </select>
            </label>
          {/if}
        </Card>
      {/if}
    {/snippet}
  </CardGrid>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
{/if}

<style>
  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin: 0.4rem 0 0.6rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .check input {
    margin-top: 0.15rem;
  }
  .check small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
  }
  .note {
    margin: 0.2rem 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .now {
    display: flex;
    align-items: center;
    gap: 0.9rem;
    margin: 0.6rem 0;
  }
  .now p {
    margin: 0.1rem 0;
  }
  .swatch {
    flex: none;
    width: 3rem;
    height: 3rem;
    border: 1px solid var(--line);
    border-radius: 12px;
    transition: background-color 120ms linear;
  }
  .other {
    margin: 0.6rem 0;
    padding: 0.5rem 0.7rem;
    border-left: 3px solid var(--warn);
    border-radius: 6px;
    background: color-mix(in srgb, var(--warn) 9%, transparent);
  }
  .other p {
    margin: 0.1rem 0;
  }
  .devices {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .field {
    margin: 0.5rem 0 0.8rem;
  }
  .field .label {
    display: block;
    margin-bottom: 0.3rem;
    font-size: 0.85rem;
  }
  .field.inline {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .field.inline .label {
    margin: 0;
  }
  .slider {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 0.6rem;
    margin: 0.4rem 0 0.8rem;
    font-size: 0.85rem;
  }
  select {
    padding: 0.25rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  button.primary {
    margin-top: 0.4rem;
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
  }
  .err {
    color: var(--bad, #d14343);
  }
</style>
