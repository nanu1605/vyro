<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import { getAutostart, getSensorTree, pickProfile, setAutostart, type OverlayCorner, type OverlaySettings } from '../ipc';
  import { activeProfile, game, prefs, profiles, savePrefs } from '../prefs.svelte';
  import OverlayPlate from './OverlayPlate.svelte';
  import { pollLabel } from '../games';
  import { ACCENTS, defaultMetrics, graphShown, METRICS, MODES, overlayData, shownMetrics, UPDATE_CHOICES, type OverlayAccent } from '../overlay';
  import { simGame } from '../sim-game';
  import type { GameAutomation, OverlayMetric, Profile } from '../types';
  import { checkNow, install, update } from '../updates.svelte';
  import { fmtDateTime } from '../format';
  import type { BackendStatus, SensorSnapshot, SensorTree, TempUnit } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
  }
  let { snap, backend, active }: Props = $props();

  let autostart = $state<boolean | null>(null);
  let tree = $state.raw<SensorTree | null>(null);
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

  const CORNERS: Array<{ id: OverlayCorner; label: string }> = [
    { id: 'topLeft', label: 'Top left' },
    { id: 'topRight', label: 'Top right' },
    { id: 'bottomLeft', label: 'Bottom left' },
    { id: 'bottomRight', label: 'Bottom right' },
  ];
  const setOverlay = (change: Partial<OverlaySettings>) =>
    save('Overlay', () => savePrefs({ overlay: { ...prefs.overlay, ...change } }));
  const SLIDERS = [
    { key: 'scale', label: 'Size', min: 75, max: 200, step: 5, unit: '%' },
    { key: 'opacity', label: 'Background', min: 0, max: 100, step: 5, unit: '%' },
    { key: 'offsetX', label: 'In from the side', min: 0, max: 200, step: 4, unit: ' px' },
    { key: 'offsetY', label: 'In from the edge', min: 0, max: 200, step: 4, unit: ' px' },
  ] as const;
  function toggleMetric(id: OverlayMetric, on: boolean): void {
    const next = on ? [...prefs.overlay.metrics, id] : prefs.overlay.metrics.filter((m) => m !== id);
    if (next.length) void setOverlay({ metrics: next });
  }
  // Preview: the game being played, else the simulated one.
  const preview = $derived.by(() => {
    if (!snap) return null;
    const g = snap.game ?? simGame(snap.ts, snap.gpu[0]?.usage ?? 80, Math.random);
    return overlayData(prefs.overlay, snap, g);
  });
  const setGames = (change: Partial<GameAutomation>) =>
    save('Game profiles', () => savePrefs({ games: { ...prefs.games, ...change } }));
  const nameOf = (p: Profile | null | undefined) => profiles.list.find((x) => x.id === p)?.name ?? p ?? '';

  const upd = $derived(update.status);
  let busy = $state(false);
  async function run(what: string, f: () => Promise<void>): Promise<void> {
    busy = true;
    await save(what, f);
    busy = false;
  }

  const names = $derived(new Map((tree?.groups ?? []).flatMap((g) => g.sensors).map((s) => [s.id, s.name])));
  const UNITS: Array<{ id: TempUnit; label: string }> = [
    { id: 'c', label: 'Celsius (°C)' },
    { id: 'f', label: 'Fahrenheit (°F)' },
  ];
</script>

<section class="grid">
  <Card title="Profile" subtitle={nameOf(activeProfile()) || null}>
    <fieldset>
      <legend>Poll rate, notifications, animation, dashboard</legend>
      {#each profiles.list as p (p.id)}
        <label class="check">
          <input
            type="radio"
            name="profile"
            checked={activeProfile() === p.id}
            onchange={() => void save('Profile', () => pickProfile(p.id))}
          />
          <span>{p.name}<small>{p.description}</small></span>
        </label>
      {/each}
    </fieldset>
    {#if game.state?.alert}
      <p class="note" role="status">
        {nameOf(game.state.profile)} is in use while the alert “{game.state.alert}” holds; it ends 30 s after the alert
        clears. Picking a profile here keeps it instead.
      </p>
    {:else if game.state?.game && game.state.auto}
      <p class="note" role="status">
        {nameOf(game.state.profile)} is in use while {snap?.game?.app ?? game.state.game} runs; {nameOf(game.state.base)} comes
        back when it closes. Picking a profile here keeps it for the rest of the game.
      </p>
    {:else}
      <p class="note">Also in the tray icon's menu.</p>
    {/if}
  </Card>

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

  <Card title="Gaming">
    <label class="check">
      <input
        type="checkbox"
        checked={prefs.gameCapture}
        onchange={(e) => void save('Gaming', () => savePrefs({ gameCapture: (e.currentTarget as HTMLInputElement).checked }))}
      />
      <span
        >Measure the frame rate of fullscreen games<small
          >FPS, 1% lows and frame times of the game in the foreground, with Intel PresentMon. Nothing is injected into
          the game.</small
        ></span
      >
    </label>
    <label class="check">
      <input
        type="checkbox"
        checked={prefs.games.enabled}
        onchange={(e) => void setGames({ enabled: (e.currentTarget as HTMLInputElement).checked })}
      />
      <span
        >Switch profiles for games<small
          >While a game runs, its profile, fan settings and update rate apply; yours come back when it closes. Set
          games up in the Gaming tab.</small
        ></span
      >
    </label>
    {#if prefs.games.enabled}
      <div class="sub">
        <label>
          Games without their own settings
          <select
            value={prefs.games.defaultProfile ?? 'none'}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              void setGames({ defaultProfile: v === 'none' ? null : (v as Profile) });
            }}
          >
            {#each profiles.list as p (p.id)}<option value={p.id}>{p.name} profile</option>{/each}
            <option value="none">Leave the profile as it is</option>
          </select>
        </label>
        <label class="check">
          <input
            type="checkbox"
            checked={prefs.games.loadTrigger}
            onchange={(e) => void setGames({ loadTrigger: (e.currentTarget as HTMLInputElement).checked })}
          />
          <span
            >Also treat an app in a window as a game when it keeps the GPU busy<small
              >60% or more of the GPU for 10 s. Browsers and video players never count; mark others "Not a game" in the
              Gaming tab.</small
            ></span
          >
        </label>
      </div>
    {/if}
  </Card>

  <Card title="Overlay" subtitle={prefs.overlay.enabled ? (MODES.find((m) => m.id === prefs.overlay.mode)?.label ?? null) : 'Off'} wide>
    <div class="overlay-grid">
      <div class="controls">
        <label class="check">
          <input
            type="checkbox"
            checked={prefs.overlay.enabled}
            disabled={!prefs.gameCapture}
            onchange={(e) => void setOverlay({ enabled: (e.currentTarget as HTMLInputElement).checked })}
          />
          <span
            >Show the overlay over the game<small
              >A window in a corner of the game’s screen. It ignores the mouse and never takes focus; borderless and
              windowed fullscreen games only. Also in the tray menu; each game can have its own in the Gaming
              tab.</small
            ></span
          >
        </label>
        <fieldset>
          <legend>Mode</legend>
          {#each MODES as m (m.id)}
            <label class="check">
              <input type="radio" name="overlay-mode" checked={prefs.overlay.mode === m.id} onchange={() => void setOverlay({ mode: m.id, metrics: [] })} />
              <span>{m.label}<small>{m.text}</small></span>
            </label>
          {/each}
        </fieldset>
        <div class="pair">
          <label>
            Corner
            <select
              value={prefs.overlay.corner}
              onchange={(e) => void setOverlay({ corner: (e.currentTarget as HTMLSelectElement).value as OverlayCorner })}
            >
              {#each CORNERS as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
            </select>
          </label>
          <label>
            Refresh
            <select
              value={String(prefs.overlay.updateMs)}
              onchange={(e) => void setOverlay({ updateMs: Number((e.currentTarget as HTMLSelectElement).value) })}
            >
              {#each UPDATE_CHOICES as ms (ms)}<option value={String(ms)}>Every {ms / 1000} s</option>{/each}
            </select>
          </label>
        </div>
        {#each SLIDERS as sl (sl.key)}
          <label class="slider">
            <span>{sl.label}</span>
            <input
              type="range"
              min={sl.min}
              max={sl.max}
              step={sl.step}
              value={prefs.overlay[sl.key]}
              onchange={(e) => void setOverlay({ [sl.key]: Number((e.currentTarget as HTMLInputElement).value) })}
            />
            <output>{prefs.overlay[sl.key]}{sl.unit}</output>
          </label>
        {/each}
        <label class="check">
          <input
            type="checkbox"
            checked={graphShown(prefs.overlay)}
            disabled={prefs.overlay.mode === 'minimal' || prefs.overlay.mode === 'graph'}
            onchange={(e) => void setOverlay({ graph: (e.currentTarget as HTMLInputElement).checked })}
          />
          <span>Frame-time graph</span>
        </label>
        <label class="check">
          <input
            type="checkbox"
            checked={prefs.overlay.compact}
            onchange={(e) => void setOverlay({ compact: (e.currentTarget as HTMLInputElement).checked })}
          />
          <span>Compact</span>
        </label>
        <fieldset class="swatches">
          <legend>Colour</legend>
          {#each Object.entries(ACCENTS) as [id, color] (id)}
            <label class="swatch" style:--swatch={color}>
              <input type="radio" name="overlay-accent" aria-label={id} checked={prefs.overlay.accent === id} onchange={() => void setOverlay({ accent: id as OverlayAccent })} />
            </label>
          {/each}
        </fieldset>
        <details>
          <summary>Readings: {prefs.overlay.metrics.length ? 'your own' : 'the mode’s'} ({shownMetrics(prefs.overlay.mode, prefs.overlay.metrics).length})</summary>
          <label class="check">
            <input
              type="checkbox"
              checked={prefs.overlay.metrics.length > 0}
              onchange={(e) =>
                void setOverlay({ metrics: (e.currentTarget as HTMLInputElement).checked ? [...defaultMetrics(prefs.overlay.mode)] : [] })}
            />
            <span>Choose the readings myself</span>
          </label>
          {#if prefs.overlay.metrics.length}
            <div class="metrics">
              {#each METRICS as m (m.id)}
                <label class="check">
                  <input
                    type="checkbox"
                    checked={prefs.overlay.metrics.includes(m.id)}
                    disabled={prefs.overlay.metrics.length === 1 && prefs.overlay.metrics.includes(m.id)}
                    onchange={(e) => toggleMetric(m.id, (e.currentTarget as HTMLInputElement).checked)}
                  />
                  <span>{m.label}</span>
                </label>
              {/each}
            </div>
          {/if}
        </details>
        <p class="note">
          The overlay refreshes at most as often as Vyro reads sensors: every {pollLabel(game.state?.pollMs ?? 1000)} right
          now.
        </p>
      </div>
      {#if preview}
        <div class="preview" aria-label="Overlay preview" role="img">
          <div class="corner {preview.corner}"><OverlayPlate data={preview} /></div>
        </div>
      {/if}
    </div>
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

  <Card title="Updates" subtitle={upd ? `version ${upd.current}` : null}>
    {#if !upd}
      <p class="note">Loading…</p>
    {:else if !upd.configured}
      <p class="note">
        This build does not update itself (a test or local build). Release builds look for new versions on the project’s
        GitHub releases.
      </p>
    {:else}
      <label class="check">
        <input
          type="checkbox"
          checked={prefs.checkUpdates}
          onchange={(e) => void save('Updates', () => savePrefs({ checkUpdates: (e.currentTarget as HTMLInputElement).checked }))}
        />
        <span>
          Check for updates daily
          <small>Fetches the latest release’s version file from GitHub; nothing about this PC is sent.</small>
        </span>
      </label>
      <div role="status" class="upd">
        {#if upd.installing}
          <p>Starting the installer… Vyro closes and opens again when it is done.</p>
        {:else if upd.checking}
          <p>Checking…</p>
        {:else if upd.available}
          <p><b>Vyro {upd.available.version} is available.</b></p>
          {#if upd.available.notes}<p class="note notes">{upd.available.notes}</p>{/if}
          {#if upd.error}<p class="err">Update failed: {upd.error}</p>{/if}
        {:else if upd.error}
          <p class="err">Check failed: {upd.error}</p>
        {:else if upd.lastCheck}
          <p>Vyro is up to date <span class="note">(checked {fmtDateTime(upd.lastCheck)})</span></p>
        {/if}
      </div>
      <div class="actions">
        {#if upd.available}
          <button type="button" disabled={busy || upd.installing} onclick={() => void run('Install update', install)}>
            Install and restart
          </button>
        {/if}
        <button type="button" disabled={busy || upd.checking || upd.installing} onclick={() => void run('Check for updates', checkNow)}>
          Check now
        </button>
      </div>
      {#if upd.available}
        <p class="note">Installing asks for administrator approval once and restarts the sensor service.</p>
      {/if}
    {/if}
  </Card>

  {#if error}<p class="err" role="alert">{error}</p>{/if}
</section>

<style>
  .overlay-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr);
    gap: 1rem 1.5rem;
  }
  @media (max-width: 760px) {
    .overlay-grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .pair {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1rem;
    margin: 0.2rem 0 0.6rem;
    font-size: 0.82rem;
  }
  .pair label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .slider {
    display: grid;
    grid-template-columns: 8.5rem minmax(0, 1fr) 3.4rem;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.45rem;
    font-size: 0.82rem;
  }
  .slider input {
    width: 100%;
    accent-color: var(--accent);
  }
  .slider output {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .swatches {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.45rem;
    margin-bottom: 0.6rem;
  }
  .swatches legend {
    flex-basis: 100%;
  }
  .swatch input {
    width: 1.5rem;
    height: 1.5rem;
    margin: 0;
    border: 2px solid var(--line);
    border-radius: 50%;
    background: var(--swatch);
    cursor: pointer;
    appearance: none;
  }
  .swatch input:checked {
    border-color: var(--text);
    box-shadow: 0 0 0 2px var(--bg) inset;
  }
  .swatch input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  details {
    margin-bottom: 0.6rem;
    font-size: 0.82rem;
  }
  summary {
    margin-bottom: 0.45rem;
    cursor: pointer;
  }
  .metrics {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(7.5rem, 1fr));
    column-gap: 0.75rem;
  }
  .metrics .check {
    margin-bottom: 0.3rem;
  }
  /* A stand-in game scene behind the plate. */
  .preview {
    display: flex;
    min-height: 15rem;
    padding: 16px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background:
      radial-gradient(circle at 70% 30%, #5b7fa6 0%, transparent 45%),
      radial-gradient(circle at 20% 80%, #8a5a3c 0%, transparent 50%),
      linear-gradient(160deg, #2c3e50, #1b2631 55%, #3e2c23);
    overflow: hidden;
  }
  .preview .corner {
    display: flex;
    flex: 1;
  }
  .corner.topLeft {
    align-items: flex-start;
    justify-content: flex-start;
  }
  .corner.topRight {
    align-items: flex-start;
    justify-content: flex-end;
  }
  .corner.bottomLeft {
    align-items: flex-end;
    justify-content: flex-start;
  }
  .corner.bottomRight {
    align-items: flex-end;
    justify-content: flex-end;
  }
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
  .sub {
    display: grid;
    gap: 0.5rem;
    margin: -0.2rem 0 0.4rem 1.6rem;
    font-size: 0.82rem;
  }
  .sub label:not(.check) {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  select {
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
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
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  .notes {
    white-space: pre-line;
  }
  .hidden button,
  .actions button {
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .actions button {
    padding: 0.35rem 0.8rem;
    font-size: 0.82rem;
  }
  .actions button:disabled {
    cursor: default;
    opacity: 0.55;
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
