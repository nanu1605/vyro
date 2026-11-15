<script lang="ts">
  // Settings (2.5, the owner, 2026-11-15): a search box and six tiles, each a group of settings
  // with its state in a line. A tile opens its group in a window over the page; search finds any
  // one setting and opens its window there. What each setting does is said in a line, the rest
  // behind "More"; the less used ones are under "Advanced".
  import { untrack, type Component } from 'svelte';
  import Gamepad from '@lucide/svelte/icons/gamepad-2';
  import Palette from '@lucide/svelte/icons/palette';
  import Power from '@lucide/svelte/icons/power';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import SearchIcon from '@lucide/svelte/icons/search';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Card from './Card.svelte';
  import More from './More.svelte';
  import SettingsDialog from './SettingsDialog.svelte';
  import SettingsTile from './SettingsTile.svelte';
  import ProCard from './ProCard.svelte';
  import ProLocked from './ProLocked.svelte';
  import { cards, resetOrder } from '../card-order.svelte';
  import { edition, lockedSection, trialDays } from '../edition.svelte';
  import { closeSettingsSection, openSettingsGroup, openSettingsSection, settingsDialog } from '../settings-dialog.svelte';
  import { followRemote, remoteStatus } from '../remote-status.svelte';
  import { GROUPS, PART_TITLES, group as groupOf, placeOf, proLine, remoteLine, searchSettings, startupLine, updatesLine, whereOf, type GroupId, type Setting } from '../settings-map';
  import { windowLook } from '../window-look.svelte';
  import { getAutostart, getSensorTree, pickProfile, setAutostart, type OverlayCorner, type OverlaySettings } from '../ipc';
  import { activeProfile, game, prefs, profiles, savePrefs } from '../prefs.svelte';
  import OverlayPlate from './OverlayPlate.svelte';
  import { pollLabel } from '../games';
  import { ACCENTS, defaultMetrics, graphShown, METRICS, MODES, overlayData, plateColor, shownMetrics, UPDATE_CHOICES, type OverlayAccent } from '../overlay';
  import { simGame } from '../sim-game';
  import type { GameAutomation, OverlayMetric, Profile } from '../types';
  import { checkNow, install, update } from '../updates.svelte';
  import { fmtDateTime } from '../format';
  import { ACCENTS as ACCENT_LIST, THEMES, accentPair, accentFills, applyLook, theme as themeOf } from '../themes';
  import { isHex } from '../color';
  import ColorWheel from './ColorWheel.svelte';
  import type { BackendStatus, SensorSnapshot, SensorTree, TempUnit } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
    /** Opens What's new (1.5). */
    onwhatsnew: () => void;
  }
  let { snap, backend, active, onwhatsnew }: Props = $props();

  let autostart = $state<boolean | null>(null);
  const look = $derived(themeOf(prefs.theme));
  const glassOn = $derived(prefs.glass !== false && windowLook.transparency);
  /** A custom accent (1.3): the colour in use, or where the wheel starts from. */
  const custom = $derived(isHex(prefs.accent));
  let lastCustom = $state<string | null>(null);
  $effect(() => {
    if (isHex(prefs.accent)) lastCustom = prefs.accent;
  });
  const customColour = $derived(custom ? prefs.accent : (lastCustom ?? accentFills(look, prefs.accent)[0]));
  const previewAccent = (hex: string) => applyLook({ theme: prefs.theme, accent: hex as `#${string}`, openOn: prefs.openOn, glass: glassOn });
  const saveAccent = (hex: string) => void save('Accent', () => savePrefs({ accent: hex as `#${string}` }));
  let tree = $state.raw<SensorTree | null>(null);
  let error = $state<string | null>(null);

  // Remote's card is its own chunk: loaded as Settings shows, so its window has it at once.
  type RemoteCardType = Component<{ active: boolean; at: string | null }>;
  let RemoteCard = $state.raw<RemoteCardType | null>(null);

  $effect(() => {
    if (!active) return;
    untrack(() => {
      void getAutostart()
        .then((x) => (autostart = x))
        .catch(() => (autostart = null));
      void getSensorTree(snap).then((t) => (tree = t));
      if (!RemoteCard) void import('./RemoteCard.svelte').then((m) => (RemoteCard = m.default as unknown as RemoteCardType));
    });
    // The Phone and remote tile's line, and its window's first frame.
    return followRemote();
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
  const setOverlay = (change: Partial<OverlaySettings>) => save('Overlay', () => savePrefs({ overlay: { ...prefs.overlay, ...change } }));
  const SLIDERS = [
    { key: 'scale', label: 'Size', min: 75, max: 200, step: 5, unit: '%', id: 'set-overlay-size' },
    { key: 'opacity', label: 'Background', min: 0, max: 100, step: 5, unit: '%', id: 'set-overlay-background' },
    { key: 'offsetX', label: 'In from the side', min: 0, max: 200, step: 4, unit: ' px', id: 'set-overlay-offset' },
    { key: 'offsetY', label: 'In from the edge', min: 0, max: 200, step: 4, unit: ' px', id: undefined },
  ] as const;
  function toggleMetric(id: OverlayMetric, on: boolean): void {
    const next = on ? [...prefs.overlay.metrics, id] : prefs.overlay.metrics.filter((m) => m !== id);
    if (next.length) void setOverlay({ metrics: next });
  }
  const setGames = (change: Partial<GameAutomation>) => save('Game profiles', () => savePrefs({ games: { ...prefs.games, ...change } }));
  const partTitle = (p: string) => PART_TITLES[p] ?? p;
  const nameOf = (p: Profile | null | undefined) => profiles.list.find((x) => x.id === p)?.name ?? p ?? '';

  const upd = $derived(update.status);
  let busy = $state(false);
  async function run(what: string, f: () => Promise<void>): Promise<void> {
    busy = true;
    await save(what, f);
    busy = false;
  }

  /** The group open in its window: from a tile, search, Ctrl K and links. */
  const open = $derived(settingsDialog.id ? groupOf(settingsDialog.id) : null);
  // Settings no longer shown: its window goes with it (one in the top layer would stay over the
  // next view).
  $effect(() => {
    if (!active) untrack(closeSettingsSection);
  });
  // The overlay's preview, in Gaming and overlay's window only: the game being played, else the
  // simulated one. Its graph draws up to 30 frames a second.
  const preview = $derived.by(() => {
    if (!snap || !active || open?.id !== 'gaming') return null;
    const g = snap.game ?? simGame(snap.ts, snap.gpu[0]?.usage ?? 80, Math.random);
    return overlayData(prefs.overlay, snap, g);
  });

  // --- The tiles. ---------------------------------------------------------------------------------
  const ICONS: Record<GroupId, Component<{ size?: number; 'aria-hidden'?: boolean | 'true' }>> = {
    appearance: Palette,
    gaming: Gamepad,
    remote: Smartphone,
    general: Power,
    updates: RefreshCw,
    pro: Sparkles,
  };
  /** A group locked on Free: every part of it is Pro's. */
  const locked = (id: GroupId) => groupOf(id).parts.every((p) => lockedSection(p));
  const accentName = $derived(
    prefs.accent === 'theme' ? null : isHex(prefs.accent) ? 'custom accent' : `${ACCENT_LIST.find((a) => a.id === prefs.accent)?.name ?? prefs.accent} accent`,
  );
  const lines = $derived<Record<GroupId, string>>({
    appearance: [look.name, accentName, glassOn ? 'glass on' : 'glass off'].filter(Boolean).join(' · '),
    gaming: locked('gaming')
      ? 'Part of Clokrix Pro'
      : `${nameOf(activeProfile())} profile · overlay ${prefs.overlay.enabled ? (MODES.find((m) => m.id === prefs.overlay.mode)?.label ?? 'on') : 'off'}`,
    remote: locked('remote') ? 'Part of Clokrix Pro' : (remoteLine(remoteStatus.s) ?? groupOf('remote').blurb),
    general: startupLine(autostart, prefs.temperatureUnit, prefs.hiddenSensors.length),
    updates: updatesLine(upd) ?? groupOf('updates').blurb,
    pro: proLine(edition.e, trialDays(edition.e)) ?? groupOf('pro').blurb,
  });

  // --- Search. ------------------------------------------------------------------------------------
  let query = $state('');
  const found = $derived(searchSettings(query));
  let sel = $state(0);
  $effect(() => {
    void found;
    sel = 0;
  });
  function openFound(s: Setting, el?: Element | null): void {
    const r = el?.getBoundingClientRect();
    openSettingsSection(s.id, r ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : null);
  }
  function onSearchKey(e: KeyboardEvent): void {
    if (!found.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      sel = (sel + (e.key === 'ArrowDown' ? 1 : found.length - 1)) % found.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const s = found[sel];
      if (s) openFound(s, document.getElementById(`found-${s.id}`));
    }
  }

  const names = $derived(new Map((tree?.groups ?? []).flatMap((g) => g.sensors).map((s) => [s.id, s.name])));
  const UNITS: Array<{ id: TempUnit; label: string }> = [
    { id: 'c', label: 'Celsius (°C)' },
    { id: 'f', label: 'Fahrenheit (°F)' },
  ];
  /** "Put cards back": the other views' cards (Settings' own are tiles now). */
  const rearranged = $derived(Object.keys(cards.views).some((v) => v !== 'settings'));
  const mode = $derived(MODES.find((m) => m.id === prefs.overlay.mode));
</script>

<!-- A setting turned on or off: its switch, its name and a line; "More" beside it when there is
     more to say. -->
{#snippet toggle(id: string, title: string, line: string | null, checked: boolean, onchange: (on: boolean) => void, more: string | undefined = undefined, disabled = false)}
  <div class="row" {id}>
    <label class="check">
      <input
        type="checkbox"
        class="switch"
        role="switch"
        {checked}
        {disabled}
        onchange={(e) => onchange((e.currentTarget as HTMLInputElement).checked)}
      />
      <span>{title}{#if line}<small>{line}</small>{/if}</span>
    </label>
    {#if more}<More about={title}>{more}</More>{/if}
  </div>
{/snippet}

{#snippet part(id: string, level: 2 | 3)}
  {#if id === 'pro'}
    <ProCard />
  {:else if id === 'remote'}
    {#if RemoteCard}
      <RemoteCard {active} at={settingsDialog.at} />
    {:else}
      <Card title="Remote" id="remote" {level}><p class="note">Loading…</p></Card>
    {/if}
  {:else if id === 'appearance'}
    <Card title={partTitle('appearance')} id="appearance" subtitle={look.name} {level}>
      <fieldset id="set-theme">
        <legend>Theme</legend>
        <div class="themes">
          {#each THEMES as t (t.id)}
            {@const [main, second] = accentPair(t, prefs.accent)}
            <label class="check theme">
              <input type="radio" name="theme" checked={prefs.theme === t.id} onchange={() => void save('Theme', () => savePrefs({ theme: t.id }))} />
              <span class="swatch" aria-hidden="true" style:background={t.palette.bg} style:border-color={t.palette.line}>
                <i style:background={t.palette.panelEdge}></i><i style:background={main}></i><i style:background={second}></i>
              </span>
              <span>{t.name}<small>{t.description}</small></span>
            </label>
          {/each}
        </div>
      </fieldset>
      <fieldset>
        <legend>Glass</legend>
        {@render toggle(
          'set-glass',
          'Glass effects',
          windowLook.transparency
            ? 'See-through cards, bars and windows.'
            : 'Off in Windows (Transparency effects): turn it on in Windows Settings → Personalisation → Colours.',
          glassOn,
          (on) => void save('Glass effects', () => savePrefs({ glass: on })),
          windowLook.transparency ? 'On Windows 11 the desktop shows frosted behind Clokrix’s window too. Off is the solid look.' : undefined,
          !windowLook.transparency,
        )}
      </fieldset>
      <fieldset id="set-accent">
        <legend>Accent</legend>
        <div class="accents">
          <label class="check">
            <input type="radio" name="accent" checked={prefs.accent === 'theme'} onchange={() => void save('Accent', () => savePrefs({ accent: 'theme' }))} />
            <span class="dot" aria-hidden="true" style:background={look.accent[0]}></span>
            <span>{look.name}'s own</span>
          </label>
          {#each ACCENT_LIST as a (a.id)}
            <label class="check">
              <input type="radio" name="accent" checked={prefs.accent === a.id} onchange={() => void save('Accent', () => savePrefs({ accent: a.id }))} />
              <span class="dot" aria-hidden="true" style:background={look.light ? a.light[0] : a.dark[0]}></span>
              <span>{a.name}</span>
            </label>
          {/each}
          <label class="check">
            <input type="radio" name="accent" checked={custom} onchange={() => saveAccent(customColour)} />
            <span class="dot custom" aria-hidden="true" style:background={customColour}></span>
            <span>Custom</span>
          </label>
        </div>
        {#if custom}
          <ColorWheel value={prefs.accent} theme={look} onpreview={previewAccent} oncommit={saveAccent} />
        {/if}
      </fieldset>
      <details class="adv">
        <summary>Advanced</summary>
        <fieldset id="set-open-on">
          <legend>Open on</legend>
          <label class="check">
            <input type="radio" name="open-on" checked={prefs.openOn === 'dashboard'} onchange={() => void save('Open on', () => savePrefs({ openOn: 'dashboard' }))} />
            <span>Dashboard<small>When Clokrix starts and when you open it from the tray.</small></span>
          </label>
          <label class="check">
            <input type="radio" name="open-on" checked={prefs.openOn === 'last'} onchange={() => void save('Open on', () => savePrefs({ openOn: 'last' }))} />
            <span>Last tab<small>Where you left off.</small></span>
          </label>
        </fieldset>
        <fieldset id="set-cards">
          <legend>Cards</legend>
          <p class="note">Drag a card by its grip to move it. Each view keeps its own order.</p>
          <div class="actions">
            <button type="button" disabled={!rearranged} onclick={resetOrder}>Put cards back</button>
          </div>
        </fieldset>
      </details>
    </Card>
  {:else if id === 'profile'}
    <Card title={partTitle('profile')} id="profile" subtitle={nameOf(activeProfile()) || null} {level}>
      <fieldset id="set-profile">
        <legend>Poll rate, notifications, animation, dashboard</legend>
        {#each profiles.list as p (p.id)}
          <label class="check">
            <input type="radio" name="profile" checked={activeProfile() === p.id} onchange={() => void save('Profile', () => pickProfile(p.id))} />
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
  {:else if id === 'gaming'}
    <Card title={partTitle('gaming')} id="gaming" {level}>
      {@render toggle(
        'set-frame-rate',
        'Measure the frame rate of fullscreen games',
        'FPS, 1% lows and frame times, with Intel PresentMon.',
        prefs.gameCapture,
        (on) => void save('Gaming', () => savePrefs({ gameCapture: on })),
        'Of the game in the foreground. Nothing is injected into the game.',
      )}
      {@render toggle(
        'set-game-profiles',
        'Switch profiles for games',
        'A game’s profile and fan settings apply while it runs.',
        prefs.games.enabled,
        (on) => void setGames({ enabled: on }),
        'Yours come back when it closes. Set games up in the Gaming tab.',
      )}
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
          {@render toggle(
            'set-game-load',
            'Also count an app in a window that keeps the GPU busy',
            '60% or more of the GPU for 10 s.',
            prefs.games.loadTrigger,
            (on) => void setGames({ loadTrigger: on }),
            'Browsers and video players never count; mark others "Not a game" in the Gaming tab.',
          )}
        </div>
      {/if}
    </Card>
  {:else if id === 'overlay'}
    <Card title={partTitle('overlay')} id="overlay" subtitle={prefs.overlay.enabled ? (mode?.label ?? null) : 'Off'} {level}>
      <div class="overlay-grid">
        <div class="controls">
          {@render toggle(
            'set-overlay',
            'Show the overlay over the game',
            prefs.gameCapture ? 'Borderless and windowed fullscreen games.' : 'Needs the frame rate measured (Games, above).',
            prefs.overlay.enabled,
            (on) => void setOverlay({ enabled: on }),
            'A window in a corner of the game’s screen; it ignores the mouse and never takes focus. Also in the tray menu; each game can have its own in the Gaming tab.',
            !prefs.gameCapture,
          )}
          <div class="field" id="set-overlay-mode">
            <span class="label" id="overlay-mode-label">Mode</span>
            <div class="segmented" role="group" aria-labelledby="overlay-mode-label">
              {#each MODES as m (m.id)}
                <button type="button" aria-pressed={prefs.overlay.mode === m.id} onclick={() => void setOverlay({ mode: m.id, metrics: [] })}>{m.label}</button>
              {/each}
            </div>
            {#if mode}<p class="note">{mode.text}</p>{/if}
          </div>
          <label class="field inline" id="set-overlay-corner">
            <span class="label">Corner</span>
            <select value={prefs.overlay.corner} onchange={(e) => void setOverlay({ corner: (e.currentTarget as HTMLSelectElement).value as OverlayCorner })}>
              {#each CORNERS as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
            </select>
          </label>
          <details class="adv">
            <summary>Advanced</summary>
            <label class="field inline" id="set-overlay-refresh">
              <span class="label">Refresh</span>
              <select value={String(prefs.overlay.updateMs)} onchange={(e) => void setOverlay({ updateMs: Number((e.currentTarget as HTMLSelectElement).value) })}>
                {#each UPDATE_CHOICES as ms (ms)}<option value={String(ms)}>Every {ms / 1000} s</option>{/each}
              </select>
            </label>
            {#each SLIDERS as sl (sl.key)}
              <label class="slider" id={sl.id}>
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
            {@render toggle(
              'set-overlay-graph',
              'Frame-time graph',
              null,
              graphShown(prefs.overlay),
              (on) => void setOverlay({ graph: on }),
              undefined,
              prefs.overlay.mode === 'minimal' || prefs.overlay.mode === 'graph',
            )}
            {@render toggle('set-overlay-compact', 'Compact', null, prefs.overlay.compact, (on) => void setOverlay({ compact: on }))}
            <fieldset class="swatches" id="set-overlay-colour">
              <legend>Colour</legend>
              {#each [['app', plateColor('app', prefs)], ...Object.entries(ACCENTS)] as [id, color] (id)}
                <label class="swatch" class:app={id === 'app'} style:--swatch={color} title={id === 'app' ? 'The app’s accent' : undefined}>
                  <input type="radio" name="overlay-accent" aria-label={id === 'app' ? 'app accent' : id} checked={prefs.overlay.accent === id} onchange={() => void setOverlay({ accent: id as OverlayAccent })} />
                </label>
              {/each}
            </fieldset>
            <details id="set-overlay-readings">
              <summary>Readings: {prefs.overlay.metrics.length ? 'your own' : 'the mode’s'} ({shownMetrics(prefs.overlay.mode, prefs.overlay.metrics).length})</summary>
              <label class="check">
                <input
                  type="checkbox"
                  checked={prefs.overlay.metrics.length > 0}
                  onchange={(e) => void setOverlay({ metrics: (e.currentTarget as HTMLInputElement).checked ? [...defaultMetrics(prefs.overlay.mode)] : [] })}
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
              The overlay refreshes at most as often as Clokrix reads sensors: every {pollLabel(game.state?.pollMs ?? 1000)} right now.
            </p>
          </details>
        </div>
        {#if preview}
          <div class="preview" aria-label="Overlay preview" role="img">
            <div class="corner {preview.corner}"><OverlayPlate data={preview} /></div>
          </div>
        {/if}
      </div>
    </Card>
  {:else if id === 'general'}
    <Card title={partTitle('general')} id="general" {level}>
      {@render toggle(
        'set-tray',
        'Keep running in the tray when the window is closed',
        'Alerts need Clokrix running; quit from the tray icon.',
        prefs.closeToTray,
        (on) => void save('Tray', () => savePrefs({ closeToTray: on })),
      )}
      {@render toggle('set-autostart', 'Start with Windows', 'Opens in the tray when you sign in.', autostart ?? false, (on) => void setStart(on), undefined, autostart == null)}
    </Card>
  {:else if id === 'units'}
    <Card title={partTitle('units')} id="units" {level}>
      <div class="field" id="set-units">
        <span class="label" id="unit-label">Temperature</span>
        <div class="segmented" role="group" aria-labelledby="unit-label">
          {#each UNITS as u (u.id)}
            <button type="button" aria-pressed={prefs.temperatureUnit === u.id} onclick={() => void save('Units', () => savePrefs({ temperatureUnit: u.id }))}>{u.label}</button>
          {/each}
        </div>
        <p class="note">Everywhere: limits, history and notifications.</p>
      </div>
    </Card>
  {:else if id === 'hidden-sensors'}
    <Card title={partTitle('hidden-sensors')} id="hidden-sensors" subtitle={prefs.hiddenSensors.length ? `${prefs.hiddenSensors.length} hidden` : null} {level}>
      <div id="set-hidden">
        {#if prefs.hiddenSensors.length === 0}
          <p class="note">None. Hide a sensor with × in the Sensors view to leave it out of Sensors and History.</p>
        {:else}
          <ul class="hidden">
            {#each prefs.hiddenSensors as id (id)}
              <li>
                <span title={id}>{names.get(id) ?? id}</span>
                <button type="button" onclick={() => void save('Hidden sensors', () => savePrefs({ hiddenSensors: prefs.hiddenSensors.filter((x) => x !== id) }))}>
                  Show
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </Card>
  {:else if id === 'service'}
    <Card title={partTitle('service')} id="service" {level}>
      <div class="row plain" id="set-service">
        {#if !backend}
          <p class="note">Browser preview: no service.</p>
        {:else if backend.mode === 'service'}
          <p>
            {backend.connected ? 'Connected' : 'Reconnecting…'}{backend.serviceVersion ? ` · version ${backend.serviceVersion}` : ''}
            <small class="note block">Reads the sensors and keeps history and alerts, also while this window is closed.</small>
          </p>
          <More about="the sensor service">
            It keeps them in <code>%ProgramData%\Clokrix</code> (log: <code>service.log</code>).
          </More>
        {:else}
          <p>
            Not in use{backend.detail ? `: ${backend.detail}` : ''}
            <small class="note block">Reinstall Clokrix, or start “Clokrix Sensor Service” in Services.</small>
          </p>
          <More about="the sensor service">
            Clokrix reads what it can itself; CPU temperature and power, board sensors, RAM temperatures and drive health need the
            service.
          </More>
        {/if}
      </div>
    </Card>
  {:else if id === 'updates'}
    <Card title={partTitle('updates')} id="updates" subtitle={upd ? `version ${upd.current}` : null} {level}>
      {#if !upd}
        <p class="note">Loading…</p>
      {:else if !upd.configured}
        <div class="row plain">
          <p class="note">This build does not update itself (a test or local build).</p>
          <More about="updates">Release builds look for new versions on the project’s GitHub releases.</More>
        </div>
      {:else}
        {@render toggle(
          'set-check-updates',
          'Check for updates daily',
          'Nothing about this PC is sent.',
          prefs.checkUpdates,
          (on) => void save('Updates', () => savePrefs({ checkUpdates: on })),
          'It fetches the latest release’s version file from GitHub.',
        )}
        <div role="status" class="upd">
          {#if upd.installing}
            <p>Starting the installer… Clokrix closes and opens again when it is done.</p>
          {:else if upd.checking}
            <p>Checking…</p>
          {:else if upd.available}
            <p><b>Clokrix {upd.available.version} is available.</b></p>
            {#if upd.available.notes}<p class="note notes">{upd.available.notes}</p>{/if}
            {#if upd.error}<p class="err">Update failed: {upd.error}</p>{/if}
          {:else if upd.error}
            <p class="err">Check failed: {upd.error}</p>
          {:else if upd.lastCheck}
            <p>Clokrix is up to date <span class="note">(checked {fmtDateTime(upd.lastCheck)})</span></p>
          {/if}
        </div>
        <div class="actions" id="set-update-now">
          {#if upd.available}
            <button type="button" disabled={busy || upd.installing} onclick={() => void run('Install update', install)}>Install and restart</button>
          {/if}
          <button type="button" disabled={busy || upd.checking || upd.installing} onclick={() => void run('Check for updates', checkNow)}>Check now</button>
        </div>
        {#if upd.available}
          <p class="note">Installing asks for administrator approval once and restarts the sensor service.</p>
        {/if}
      {/if}
      {#if upd}
        <div class="actions" id="set-whats-new">
          <button type="button" onclick={onwhatsnew}>What’s new in {upd.current.replace(/^(\d+\.\d+)\.0$/, '$1')}</button>
        </div>
      {/if}
    </Card>
  {/if}
{/snippet}

<div class="settings">
  <div class="find">
    <SearchIcon size={16} aria-hidden="true" />
    <input
      type="search"
      placeholder="Search settings"
      aria-label="Search settings"
      autocomplete="off"
      spellcheck="false"
      role="combobox"
      aria-expanded={!!query.trim()}
      aria-controls="settings-found"
      aria-autocomplete="list"
      aria-activedescendant={query.trim() && found[sel] ? `found-${found[sel]?.id}` : undefined}
      bind:value={query}
      onkeydown={onSearchKey}
    />
  </div>
  {#if query.trim()}
    <ul class="results" id="settings-found" role="listbox" aria-label="Settings found">
      {#each found as s, i (s.id)}
        <!-- Chosen from the field with the arrow keys and Enter; the pointer clicks. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="found-{s.id}"
          role="option"
          tabindex="-1"
          aria-selected={i === sel}
          onclick={(e) => openFound(s, e.currentTarget)}
          onpointermove={() => (sel = i)}
        >
          <span class="label">{s.label}</span>
          <span class="where">{whereOf(s)}{#if locked(placeOf(s.id)!.group)} · Pro{/if}</span>
        </li>
      {/each}
    </ul>
    {#if !found.length}
      <p class="none" role="status">Nothing in Settings matches “{query.trim()}”. Ctrl K also looks through views and sensors.</p>
    {/if}
  {:else}
    <div class="tiles">
      {#each GROUPS as g (g.id)}
        <SettingsTile id={g.id} title={g.title} line={lines[g.id]} icon={ICONS[g.id]} locked={locked(g.id)} onopen={(from) => openSettingsGroup(g.id, from)} />
      {/each}
    </div>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</div>
{#if open}
  {#key settingsDialog.n}
    <SettingsDialog title={open.title} line={lines[open.id]} width={open.width} from={settingsDialog.from} at={settingsDialog.at} onclosed={closeSettingsSection}>
      {#if locked(open.id)}
        <Card title={open.title} level={3}>
          <ProLocked compact title={open.title} onkey={() => openSettingsGroup('pro')} />
        </Card>
      {:else}
        {#each open.parts as p (p)}
          {@render part(p, open.parts.length > 1 ? 3 : 2)}
        {/each}
      {/if}
    </SettingsDialog>
  {/key}
{/if}

<style>
  .settings {
    display: grid;
    gap: 1rem;
    max-width: 72rem;
  }
  .find {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 30rem;
    color: var(--muted);
  }
  .find :global(svg) {
    position: absolute;
    left: 0.85rem;
    pointer-events: none;
  }
  .find input {
    width: 100%;
    padding: 0.6rem 0.9rem 0.6rem 2.4rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel);
    color: var(--text);
    font: inherit;
    font-size: 0.9rem;
  }
  :global(:root[data-glass='on']) .find input {
    border-color: var(--glass-line);
    background: var(--glass-card);
    box-shadow: inset 0 1px 0 var(--glass-rim-low);
  }
  .find input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.9rem;
  }
  @media (max-width: 1100px) {
    .tiles {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
  @media (max-width: 640px) {
    .tiles {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .results {
    display: grid;
    gap: 2px;
    max-width: 40rem;
    margin: 0;
    padding: 0.35rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    list-style: none;
  }
  :global(:root[data-glass='on']) .results {
    border-color: var(--glass-line);
    background: var(--glass-card);
  }
  .results:empty {
    display: none;
  }
  .results li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.2rem 1rem;
    padding: 0.55rem 0.75rem;
    border-radius: 10px;
    cursor: pointer;
  }
  .results li[aria-selected='true'] {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .results .label {
    font-size: 0.9rem;
  }
  .results .where {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .none {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
  }

  /* In a group's window. */
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: 0.5rem;
    align-items: start;
  }
  .row > p {
    margin-bottom: 0.6rem;
  }
  /* No switch before its words: "More" opens under them, not under a switch. */
  .row.plain {
    --more-indent: 0;
  }
  .block {
    display: block;
    margin-top: 0.2rem;
  }
  .field {
    margin: 0.2rem 0 0.8rem;
    font-size: 0.85rem;
  }
  .field .label {
    display: block;
    margin-bottom: 0.4rem;
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .field.inline {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }
  .field.inline .label {
    margin: 0;
    min-width: 4.5rem;
  }
  .field .note {
    margin: 0.4rem 0 0;
  }
  details.adv {
    margin-top: 0.6rem;
    padding-top: 0.6rem;
    border-top: 1px dashed var(--line);
  }
  details.adv > summary {
    margin-bottom: 0.6rem;
    color: var(--muted);
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    cursor: pointer;
  }
  details.adv > summary:hover {
    color: var(--text);
  }
  .overlay-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr);
    gap: 1rem 1.5rem;
    align-items: start;
  }
  /* Its card's width, not the window's: one column under two. */
  @container (max-width: 40rem) {
    .overlay-grid {
      grid-template-columns: minmax(0, 1fr);
    }
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
  /* The app's accent: a dot in a ring, so it reads as "follows the app". */
  .swatch.app input {
    background: radial-gradient(circle, var(--swatch) 0 40%, var(--panel-edge) 44%);
  }
  .swatch input:checked {
    border-color: var(--text);
    box-shadow: 0 0 0 2px var(--bg) inset;
  }
  .swatch input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  #set-overlay-readings {
    margin-bottom: 0.6rem;
    font-size: 0.82rem;
  }
  #set-overlay-readings summary {
    margin-bottom: 0.45rem;
    cursor: pointer;
  }
  .metrics {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 7.5rem), 1fr));
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
    margin: -0.2rem 0 0.4rem 2.6rem;
    font-size: 0.82rem;
  }
  .sub label:not(.check) {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  select {
    padding: 0.25rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }
  fieldset + fieldset {
    margin-top: 0.6rem;
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 11rem), 1fr));
    column-gap: 0.75rem;
  }
  .swatch {
    display: inline-flex;
    flex: none;
    gap: 3px;
    align-items: flex-end;
    width: 2.6rem;
    height: 1.7rem;
    padding: 4px;
    border: 1px solid;
    border-radius: 6px;
  }
  .swatch i {
    flex: 1;
    height: 100%;
    border-radius: 2px;
  }
  .swatch i + i {
    height: 45%;
  }
  .accents {
    display: flex;
    flex-wrap: wrap;
    column-gap: 1rem;
  }
  .accents .check {
    align-items: center;
  }
  .accents .check input {
    margin-top: 0;
  }
  .dot {
    flex: none;
    width: 0.85rem;
    height: 0.85rem;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 25%);
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
    color: var(--hot);
    font-size: 0.82rem;
  }
  code {
    font-size: 0.75rem;
  }
</style>
