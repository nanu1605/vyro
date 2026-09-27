<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import { getAutostart, getSensorTree, setAutostart } from '../ipc';
  import { prefs, savePrefs } from '../prefs.svelte';
  import type { BackendStatus, SensorSnapshot, SensorTree, TempUnit } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
  }
  let { snap, backend, active }: Props = $props();

  let autostart = $state<boolean | null>(null);
  let tree = $state<SensorTree | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!active) return;
    untrack(() => {
      void getAutostart()
        .then((x) => (autostart = x))
        .catch(() => (autostart = null));
      void getSensorTree(snap).then((t) => (tree = t));
    });
  });

  async function save(what: string, run: () => Promise<void>): Promise<void> {
    try {
      await run();
      error = null;
    } catch (e) {
      error = `${what}: ${String(e)}`;
    }
  }

  async function setStart(on: boolean): Promise<void> {
    await save('Start with Windows', async () => {
      await setAutostart(on);
      autostart = on;
    });
    if (error) autostart = await getAutostart().catch(() => null);
  }

  const names = $derived(new Map((tree?.groups ?? []).flatMap((g) => g.sensors).map((s) => [s.id, s.name])));
  const UNITS: Array<{ id: TempUnit; label: string }> = [
    { id: 'c', label: 'Celsius (°C)' },
    { id: 'f', label: 'Fahrenheit (°F)' },
  ];
</script>

<section class="grid">
  <Card title="General">
    <label class="check">
      <input
        type="checkbox"
        checked={prefs.closeToTray}
        onchange={(e) => void save('Tray', () => savePrefs({ closeToTray: (e.currentTarget as HTMLInputElement).checked }))}
      />
      <span>Keep running in the tray when the window is closed<small>Alerts need Vyro running; quit from the tray icon.</small></span>
    </label>
    <label class="check">
      <input
        type="checkbox"
        checked={autostart ?? false}
        disabled={autostart == null}
        onchange={(e) => void setStart((e.currentTarget as HTMLInputElement).checked)}
      />
      <span>Start with Windows<small>Opens in the tray when you sign in.</small></span>
    </label>
  </Card>

  <Card title="Units">
    <fieldset>
      <legend>Temperature</legend>
      {#each UNITS as u (u.id)}
        <label class="check">
          <input
            type="radio"
            name="temp-unit"
            checked={prefs.temperatureUnit === u.id}
            onchange={() => void save('Units', () => savePrefs({ temperatureUnit: u.id }))}
          />
          <span>{u.label}</span>
        </label>
      {/each}
    </fieldset>
    <p class="note">Everywhere, including limits, history and notifications. Vyro measures and compares in °C.</p>
  </Card>

  <Card title="Hidden sensors" subtitle={prefs.hiddenSensors.length ? `${prefs.hiddenSensors.length} hidden` : null}>
    {#if prefs.hiddenSensors.length === 0}
      <p class="note">None. Hide a sensor with × in the Sensors view to leave it out of Sensors and History.</p>
    {:else}
      <ul class="hidden">
        {#each prefs.hiddenSensors as id (id)}
          <li>
            <span title={id}>{names.get(id) ?? id}</span>
            <button
              type="button"
              onclick={() => void save('Hidden sensors', () => savePrefs({ hiddenSensors: prefs.hiddenSensors.filter((x) => x !== id) }))}
            >
              Show
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </Card>

  <Card title="Sensor service">
    {#if !backend}
      <p class="note">Browser preview: no service.</p>
    {:else if backend.mode === 'service'}
      <p>
        {backend.connected ? 'Connected' : 'Reconnecting…'}{backend.serviceVersion ? ` · version ${backend.serviceVersion}` : ''}
      </p>
      <p class="note">
        The Vyro Sensor Service reads the sensors and keeps history and alert rules in <code>%ProgramData%\Vyro</code>
        (log: <code>service.log</code>), also while this window is closed.
      </p>
    {:else}
      <p>Not in use{backend.detail ? `: ${backend.detail}` : ''}</p>
      <p class="note">
        Vyro reads what it can itself; CPU temperature and power, board sensors, RAM temperatures and drive health need
        the service. Reinstall Vyro, or start “Vyro Sensor Service” in Services.
      </p>
    {/if}
  </Card>

  {#if error}<p class="err" role="alert">{error}</p>{/if}
</section>

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
    gap: 1rem;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin-bottom: 0.6rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .check input {
    margin-top: 0.2rem;
  }
  .check small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: 0.4rem;
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  p {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
  }
  .note {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .hidden {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .hidden li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.3rem 0;
    border-top: 1px solid var(--line);
    font-size: 0.82rem;
  }
  .hidden span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hidden button {
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .err {
    grid-column: 1 / -1;
    color: var(--hot);
    font-size: 0.82rem;
  }
  code {
    font-size: 0.75rem;
  }
</style>
