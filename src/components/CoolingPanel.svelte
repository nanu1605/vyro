<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import CardGrid, { type CardItem } from './CardGrid.svelte';
  import CurveEditor from './CurveEditor.svelte';
  import { defaultCurve, PRESETS, PUMP_CURVE, PUMP_SPEEDS, type Preset } from '../curve';
  import { fmt, fmtDateTime } from '../format';
  import { getFanEvents, getFanStatus, getProfiles, getSensorTree, restoreFans, setFanConfig } from '../ipc';
  import { fmtTemp } from '../prefs.svelte';
  import type {
    BackendStatus,
    FanConfig,
    FanEvent,
    FanStatus,
    HeaderConfig,
    HeaderMode,
    HeaderStatus,
    Profile,
    ProfileSpec,
    SensorSnapshot,
  } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
  }
  let { snap, backend, active }: Props = $props();

  let status = $state.raw<FanStatus | null>(null);
  /** The configuration being edited; `null` follows the engine's. */
  let draft = $state<FanConfig | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let sources = $state.raw<Array<{ id: string; name: string }>>([]);
  let confirm = $state<HTMLDialogElement | null>(null);
  let profiles = $state.raw<ProfileSpec[]>([]);
  let events = $state.raw<FanEvent[]>([]);
  /** Which header settings the cards edit: every profile's, or one profile's own. `null`
   * follows the profile in use. */
  let slotPick = $state<Profile | 'all' | null>(null);
  /** Header being renamed, and the name typed so far. */
  let renaming = $state<number | null>(null);
  let nameDraft = $state('');

  const config = $derived(draft ?? status?.config ?? null);
  const inUse = $derived(status?.profile ?? 'desktop');
  const slot = $derived<Profile | 'all'>(slotPick ?? (config?.profiles[inUse] ? inUse : 'all'));
  /** The slot has header settings of its own (every profile's always does). */
  const editable = $derived(slot === 'all' || config?.profiles[slot] != null);
  const profileName = (p: Profile) => profiles.find((x) => x.id === p)?.name ?? p;
  const dirty = $derived(draft != null);
  const writable = $derived(status?.capability.state === 'writable');

  function refresh(): void {
    getFanStatus(snap)
      .then((s) => (status = s))
      .catch((e: unknown) => (error = String(e)));
  }
  // Every tick while shown.
  const ts = $derived(snap?.ts ?? 0);
  $effect(() => {
    if (!active) return;
    void ts;
    untrack(refresh);
  });
  // Profile names, loaded once.
  $effect(() => {
    if (!active || profiles.length) return;
    untrack(() =>
      getProfiles()
        .then((p) => (profiles = p))
        .catch(() => {}),
    );
  });
  // The log: when shown, and whenever fan control changes state.
  const eventKey = $derived(
    status
      ? `${status.config.enabled}|${status.active}|${status.overrideReason}|${status.lastError}|${status.profile}`
      : '',
  );
  $effect(() => {
    if (!active) return;
    void eventKey;
    untrack(() =>
      getFanEvents()
        .then((e) => (events = e.slice(-15).reverse()))
        .catch(() => {}),
    );
  });
  // Temperature sensors a curve can follow, loaded once.
  $effect(() => {
    if (!active || sources.length) return;
    untrack(() =>
      getSensorTree(snap)
        .then((t) => {
          sources = t.groups
            .flatMap((g) => g.sensors)
            .filter((s) => s.unit === '°C' && s.available)
            .map((s) => ({ id: s.id, name: s.name }));
        })
        .catch(() => {}),
    );
  });

  function edit(f: (c: FanConfig) => void): void {
    const base = config;
    if (!base) return;
    const c = JSON.parse(JSON.stringify(base)) as FanConfig;
    f(c);
    draft = c;
  }

  /** The header list the cards show and edit (a profile without its own shows the shared one). */
  function listOf(c: FanConfig): HeaderConfig[] {
    return slot === 'all' ? c.headers : (c.profiles[slot] ?? c.headers);
  }

  function modeOf(c: FanConfig, index: number): HeaderMode {
    return listOf(c).find((h) => h.index === index)?.mode ?? { mode: 'bios' };
  }

  function setMode(index: number, mode: HeaderMode): void {
    if (!editable) return;
    edit((c) => {
      const list = listOf(c);
      const h = list.find((x) => x.index === index);
      if (h) h.mode = mode;
      else list.push({ index, mode });
    });
  }

  /** Gives a profile its own header settings, starting from the shared ones. */
  function ownSettings(p: Profile): void {
    edit((c) => (c.profiles[p] = JSON.parse(JSON.stringify(c.headers)) as HeaderConfig[]));
    slotPick = p;
  }

  /** A profile goes back to the shared header settings. */
  function useShared(p: Profile): void {
    edit((c) => delete c.profiles[p]);
    slotPick = p;
  }

  const labelOf = (c: FanConfig, h: HeaderStatus) => c.labels[h.index] ?? h.name;
  // One card per fan header (two columns wide while it shows a curve), then Safety and the events:
  // in Vyro's order or the owner's (2.0.2).
  const cards = $derived<CardItem[]>([
    ...(status?.headers ?? []).map(
      (h): CardItem => ({
        id: `fan:${h.index}`,
        title: config ? labelOf(config, h) : h.name,
        w: config && modeOf(config, h.index).mode === 'curve' ? 2 : 1,
      }),
    ),
    { id: 'fan-safety', title: 'Safety' },
    { id: 'fan-events', title: 'Recent events' },
  ]);

  function startRename(h: HeaderStatus): void {
    if (!config) return;
    renaming = h.index;
    nameDraft = config.labels[h.index] ?? '';
  }

  /** An empty name goes back to the board's. */
  function saveName(h: HeaderStatus): void {
    const name = nameDraft.trim().slice(0, 32).trim();
    edit((c) => {
      if (name && name !== h.name) c.labels[h.index] = name;
      else delete c.labels[h.index];
    });
    renaming = null;
  }

  /** Treated as a pump: the board's pump header, or one the owner marked. */
  const isPump = (c: FanConfig, h: HeaderStatus) => h.kind === 'pump' || c.pumps.includes(h.index);
  /** The board's pump header is driven only after the owner confirms a pump is on it. */
  const confirmed = (c: FanConfig, h: HeaderStatus) => h.kind !== 'pump' || c.pumps.includes(h.index);
  const floorOf = (c: FanConfig, h: HeaderStatus) =>
    isPump(c, h) ? Math.max(h.minPct, status?.pumpMinPct ?? 60) : h.minPct;

  function pick(h: HeaderStatus, kind: HeaderMode['mode']): void {
    if (!config) return;
    const cur = modeOf(config, h.index);
    if (cur.mode === kind) return;
    const pump = isPump(config, h);
    if (kind === 'bios') setMode(h.index, { mode: 'bios' });
    else if (kind === 'fixed')
      setMode(h.index, { mode: 'fixed', pct: Math.max(floorOf(config, h), Math.round(h.dutyPct ?? 50)) });
    else setMode(h.index, { mode: 'curve', curve: pump ? defaultCurve(undefined, PUMP_CURVE) : defaultCurve() });
  }

  /** Marks a header as driving a pump (or not). Un-marking the board's pump header puts it back
   * on the BIOS, since it may only be driven while confirmed. */
  function setPump(h: HeaderStatus, on: boolean): void {
    edit((c) => {
      c.pumps = on ? [...c.pumps.filter((i) => i !== h.index), h.index] : c.pumps.filter((i) => i !== h.index);
      // Every profile's settings: the pump header may only be driven while confirmed.
      for (const list of [c.headers, ...Object.values(c.profiles)]) {
        const entry = list?.find((x) => x.index === h.index);
        if (!entry) continue;
        if (!on && h.kind === 'pump') entry.mode = { mode: 'bios' };
        else if (on && entry.mode.mode === 'fixed') entry.mode.pct = Math.max(entry.mode.pct, floorOf(c, h));
      }
    });
  }

  async function save(c: FanConfig): Promise<void> {
    busy = true;
    error = null;
    try {
      await setFanConfig(c);
      draft = null;
      refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function turnOn(): void {
    confirm?.close();
    if (config) void save({ ...config, enabled: true });
  }

  function turnOff(): void {
    if (config) void save({ ...config, enabled: false });
  }

  async function backToBios(): Promise<void> {
    busy = true;
    try {
      await restoreFans();
      draft = null;
      refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function copyCurve(from: number, to: number): void {
    if (!config) return;
    const m = modeOf(config, from);
    if (m.mode === 'curve') setMode(to, { mode: 'curve', curve: JSON.parse(JSON.stringify(m.curve)) });
  }

  const others = (h: HeaderStatus) =>
    (status?.headers ?? []).filter(
      (x) => x.index !== h.index && x.controllable && config != null && confirmed(config, x),
    );
  const sourceName = (id: string) => sources.find((s) => s.id === id)?.name ?? id;
  const t = (c: number | null | undefined) => fmtTemp(c) ?? '—';
</script>

<section class="panel-wrap">
  {#if !status}
    <p class="panel empty" role="status">Reading fan control…</p>
  {:else if status.capability.state !== 'writable'}
    <div class="panel empty" role="status">
      <h2>Fan control is not available</h2>
      <p>{status.capability.reason}.</p>
      {#if backend?.mode === 'local'}
        <p class="note">Fan control runs in the Vyro sensor service, which is not running.</p>
      {:else if status.capability.state === 'readonly'}
        <p class="note">
          Vyro drives fans only on motherboards where its fan control has been checked. Your fans keep following the
          BIOS, and their speeds still show on the Dashboard.
        </p>
      {/if}
    </div>
  {:else if config}
    <div class="head">
      <div>
        <h2>Fan control</h2>
        <p class="sub">
          {status.capability.board} ·
          {#if !status.config.enabled}off: every fan follows the BIOS{:else if status.active}Vyro is driving your fans{:else}on, all fans on BIOS{/if}
        </p>
      </div>
      <div class="actions">
        {#if status.config.enabled}
          <button type="button" disabled={busy} onclick={turnOff}>Turn off</button>
          <button type="button" class="primary" disabled={busy} onclick={backToBios}>Return to BIOS</button>
        {:else}
          <button type="button" class="primary" disabled={busy} onclick={() => confirm?.showModal()}>
            Turn on fan control…
          </button>
        {/if}
      </div>
    </div>

    <details class="explain">
      <summary>Why are some fans missing?</summary>
      <p>
        Your motherboard has {status.headers.length} fan headers, and each reports the speed of one fan. Fans on a
        splitter all follow their header’s setting, but only one of them reports a speed. Fans powered by a case or
        cooler hub (usually from a SATA cable) don’t reach the motherboard at all: no program that reads the
        motherboard can see or control them, unless the hub itself is plugged into one of these headers.
      </p>
    </details>

    {#if status.overrideReason}
      <p class="alert" role="alert">{status.overrideReason}</p>
    {/if}
    {#if status.boost}
      <p class="note boost" role="status">
        The alert “{status.boost.rule}” holds: the fans Vyro drives run {status.boost.pct} % faster until it clears.
      </p>
    {/if}
    {#if status.lastError}
      <p class="alert" role="alert">{status.lastError}</p>
    {/if}
    {#if error}<p class="alert" role="alert">{error}</p>{/if}

    {#if dirty}
      <div class="pending" role="status">
        <span>Changes not applied yet.</span>
        <button type="button" class="primary" disabled={busy} onclick={() => draft && save(draft)}>Apply</button>
        <button type="button" disabled={busy} onclick={() => (draft = null)}>Discard</button>
      </div>
    {/if}

    <div class="slot-bar">
      <label>
        Fan settings for
        <select value={slot} onchange={(e) => (slotPick = e.currentTarget.value as Profile | 'all')}>
          <option value="all">Every profile</option>
          {#each profiles as p (p.id)}
            <option value={p.id}
              >{p.name}{p.id === inUse ? ' (in use)' : ''}{config.profiles[p.id] ? ' · own settings' : ''}</option
            >
          {/each}
        </select>
      </label>
      {#if slot !== 'all'}
        {#if config.profiles[slot]}
          <button type="button" onclick={() => useShared(slot as Profile)}>Use the settings for every profile</button>
        {:else}
          <button type="button" onclick={() => ownSettings(slot as Profile)}
            >Give {profileName(slot)} its own fan settings</button
          >
        {/if}
      {/if}
      <p class="note">
        {#if slot === 'all'}
          Every profile uses these, unless it has its own.
          {#if config.profiles[inUse]}The {profileName(inUse)} profile in use has its own.{/if}
        {:else if !config.profiles[slot]}
          {profileName(slot)} uses the settings for every profile.
        {:else if slot !== inUse}
          {profileName(slot)} isn’t in use now: these apply when you switch to it.
        {:else}
          In use now.
        {/if}
        Turning control on or off, the safety limits, pumps and names apply to every profile.
      </p>
    </div>

    {@const now = status}
    {@const cfg = config}
    <CardGrid view="cooling" items={cards} min="18rem">
      {#snippet card(id)}
        {#if id === 'fan-safety'}
          <Card title="Safety" id="fan-safety" subtitle="Full speed above these temperatures">
            <div class="smooth">
              <label>
                CPU
                <input
                  type="number"
                  min="40"
                  max="105"
                  step="1"
                  value={cfg.emergencyCpuC}
                  onchange={(e) => edit((c) => (c.emergencyCpuC = Number(e.currentTarget.value)))}
                /> °C
              </label>
              <label>
                GPU
                <input
                  type="number"
                  min="40"
                  max="105"
                  step="1"
                  value={cfg.emergencyGpuC}
                  onchange={(e) => edit((c) => (c.emergencyGpuC = Number(e.currentTarget.value)))}
                /> °C
              </label>
            </div>
            <p class="note">
              Every fan Vyro drives goes to 100 % when either limit is reached, or when a curve’s temperature has no reading.
              Vyro never sets a fan below 20 % or a pump below {now.pumpMinPct} %, and hands every fan back to the BIOS
              when it stops, crashes, loses its sensors, the pump stops reporting a speed, or the PC goes to sleep. Closing
              this window does not stop fan control; the Vyro service keeps running it.
            </p>
          </Card>
        {:else if id === 'fan-events'}
          <Card title="Recent events" id="fan-events" subtitle={events.length ? null : 'Nothing yet'}>
            {#if events.length}
              <ul class="events">
                {#each events as ev, i (i)}
                  <li class:bad={ev.kind === 'fault' || ev.kind === 'watchdog' || ev.kind === 'fullSpeed'}>
                    <time datetime={new Date(ev.ts * 1000).toISOString()}>{fmtDateTime(ev.ts)}</time>
                    <span>{ev.message}</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="note">Turning control on or off, full-speed moments, faults, sleep and profile changes show here.</p>
            {/if}
          </Card>
        {:else}
          {@const h = now.headers.find((x) => `fan:${x.index}` === id)}
          {#if h}
            {@const mode = modeOf(cfg, h.index)}
            <Card
              title={labelOf(cfg, h)}
              {id}
              subtitle={h.connected ? `${fmt(h.rpm)} RPM · ${fmt(h.dutyPct)} %` : `No speed reading · ${fmt(h.dutyPct)} %`}
            >
              <p class="state">
                {#if h.software}
                  <span class="tag on">Vyro</span> target {fmt(h.targetPct)} %
                  {#if h.sourceC != null}at {t(h.sourceC)}{/if}
                {:else}
                  <span class="tag">BIOS</span> following the BIOS curve
                {/if}
                {#if cfg.labels[h.index]}<span class="board-name">· {h.name}</span>{/if}
              </p>
              {#if !h.controllable}
                <p class="note">Vyro cannot control this header on this motherboard.</p>
              {:else if !confirmed(cfg, h)}
                <p class="note">
                  This header is meant for a liquid cooler’s pump{#if h.connected && h.rpm}; something on it runs at
                    {fmt(h.rpm)} RPM{/if}. Before Vyro controls it, confirm that a pump is plugged in here. Vyro keeps a
                  pump at {now.pumpMinPct} % or more, and hands every fan back to the BIOS if the pump stops reporting a
                  speed.
                </p>
                <button type="button" class="confirm-pump" onclick={() => setPump(h, true)}>A pump is plugged in here</button>
              {:else}
                {#if isPump(cfg, h) && !h.connected}
                  <p class="note warn">
                    Vyro can’t see this pump’s speed, so it can’t tell if the pump stops. Leave it on BIOS unless you’re
                    sure the pump is on this header.
                  </p>
                {:else if !h.connected}
                  <p class="note">
                    Either nothing is plugged in, or a hub or splitter whose speed wire isn’t connected. Fans on it still
                    follow this header’s setting.
                  </p>
                {:else if isPump(cfg, h)}
                  <p class="note">
                    Never below {floorOf(cfg, h)} %. If the pump stops reporting a speed, every fan goes back to the
                    BIOS.
                  </p>
                {/if}
                <fieldset class="plain" disabled={!editable}>
                <div class="modes" role="radiogroup" aria-label="{labelOf(cfg, h)} mode">
                  {#each [['bios', 'BIOS'], ['fixed', 'Fixed'], ['curve', 'Curve']] as const as [kind, text] (kind)}
                    <button
                      type="button"
                      role="radio"
                      aria-checked={mode.mode === kind}
                      class:selected={mode.mode === kind}
                      onclick={() => pick(h, kind)}>{text}</button
                    >
                  {/each}
                </div>
                {#if mode.mode === 'fixed'}
                  <label class="slider">
                    <span>Speed <strong>{mode.pct} %</strong></span>
                    <input
                      type="range"
                      min={floorOf(cfg, h)}
                      max="100"
                      step="1"
                      value={mode.pct}
                      oninput={(e) => setMode(h.index, { mode: 'fixed', pct: Number(e.currentTarget.value) })}
                    />
                    <small>Minimum {floorOf(cfg, h)} %</small>
                  </label>
                  {#if isPump(cfg, h)}
                    <span class="presets">
                      {#each Object.entries(PUMP_SPEEDS) as [name, pct] (name)}
                        <button
                          type="button"
                          class:selected={mode.pct === pct}
                          onclick={() => setMode(h.index, { mode: 'fixed', pct })}
                          >{name[0]?.toUpperCase()}{name.slice(1)} {pct} %</button
                        >
                      {/each}
                    </span>
                  {/if}
                {:else if mode.mode === 'curve'}
                  {@const curve = mode.curve}
                  <div class="curve-bar">
                    <label>
                      Follows
                      <select
                        value={curve.source}
                        onchange={(e) =>
                          setMode(h.index, { mode: 'curve', curve: { ...curve, source: e.currentTarget.value } })}
                      >
                        {#if !sources.some((s) => s.id === curve.source)}
                          <option value={curve.source}>{sourceName(curve.source)}</option>
                        {/if}
                        {#each sources as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
                      </select>
                    </label>
                    <span class="presets">
                      {#each Object.keys(PRESETS) as p (p)}
                        <button
                          type="button"
                          onclick={() =>
                            setMode(h.index, {
                              mode: 'curve',
                              curve: { ...curve, points: PRESETS[p as Preset].map((x) => ({ ...x })) },
                            })}>{p[0]?.toUpperCase()}{p.slice(1)}</button
                        >
                      {/each}
                    </span>
                    {#if others(h).length}
                      <label>
                        Copy to
                        <select
                          value=""
                          onchange={(e) => {
                            const to = Number(e.currentTarget.value);
                            e.currentTarget.value = '';
                            if (Number.isFinite(to)) copyCurve(h.index, to);
                          }}
                        >
                          <option value="" disabled>header…</option>
                          {#each others(h) as o (o.index)}<option value={o.index}>{labelOf(cfg, o)}</option>{/each}
                        </select>
                      </label>
                    {/if}
                  </div>
                  <CurveEditor
                    label="{h.name} curve"
                    points={curve.points}
                    minPct={floorOf(cfg, h)}
                    live={h.software && h.sourceC != null && h.targetPct != null ? { temp: h.sourceC, pct: h.targetPct } : null}
                    onchange={(points) => setMode(h.index, { mode: 'curve', curve: { ...curve, points } })}
                  />
                  <details>
                    <summary>Smoothing</summary>
                    <div class="smooth">
                      <label>
                        Hysteresis
                        <input
                          type="number"
                          min="0"
                          max="15"
                          step="1"
                          value={curve.hysteresisC}
                          onchange={(e) =>
                            setMode(h.index, {
                              mode: 'curve',
                              curve: { ...curve, hysteresisC: Number(e.currentTarget.value) },
                            })}
                        /> °C
                      </label>
                      <label>
                        Speed up
                        <input
                          type="number"
                          min="1"
                          max="100"
                          step="1"
                          value={curve.rampUpPctS}
                          onchange={(e) =>
                            setMode(h.index, {
                              mode: 'curve',
                              curve: { ...curve, rampUpPctS: Number(e.currentTarget.value) },
                            })}
                        /> %/s
                      </label>
                      <label>
                        Slow down
                        <input
                          type="number"
                          min="1"
                          max="100"
                          step="1"
                          value={curve.rampDownPctS}
                          onchange={(e) =>
                            setMode(h.index, {
                              mode: 'curve',
                              curve: { ...curve, rampDownPctS: Number(e.currentTarget.value) },
                            })}
                        /> %/s
                      </label>
                    </div>
                    <p class="note">
                      Hysteresis keeps the fan from hunting: it only slows once the temperature has fallen this far. The
                      speed changes by at most the rates above each second.
                    </p>
                  </details>
                {/if}
                </fieldset>
                {#if h.kind === 'pump'}
                  <button type="button" class="link" onclick={() => setPump(h, false)}>No pump on this header?</button>
                {:else}
                  <label class="pump-mark">
                    <input
                      type="checkbox"
                      checked={cfg.pumps.includes(h.index)}
                      onchange={(e) => setPump(h, e.currentTarget.checked)}
                    />
                    A pump is plugged in here (never below {now.pumpMinPct} %)
                  </label>
                {/if}
              {/if}
              {#if renaming === h.index}
                <form
                  class="rename"
                  onsubmit={(e) => {
                    e.preventDefault();
                    saveName(h);
                  }}
                >
                  <input
                    type="text"
                    maxlength="32"
                    placeholder={h.name}
                    aria-label="Name for {h.name}"
                    bind:value={nameDraft}
                  />
                  <button type="submit">Save</button>
                  <button type="button" onclick={() => (renaming = null)}>Cancel</button>
                </form>
              {:else}
                <button type="button" class="link" onclick={() => startRename(h)}>
                  {cfg.labels[h.index] ? 'Rename' : `Name this header (e.g. “Case fans”)`}
                </button>
              {/if}
            </Card>
          {/if}
        {/if}
      {/snippet}
    </CardGrid>

    <dialog bind:this={confirm} aria-labelledby="fan-confirm-title">
      <h2 id="fan-confirm-title">Turn on fan control?</h2>
      <p>Vyro will set the speed of the fans you choose instead of the BIOS.</p>
      <ul>
        <li>
          Fans never go below 20 %, pumps never below {status.pumpMinPct} %. The pump header is controlled only after you
          confirm a pump is plugged into it.
        </li>
        <li>Every fan Vyro drives runs at 100 % above the safety limits ({t(config.emergencyCpuC)} CPU, {t(config.emergencyGpuC)} GPU).</li>
        <li>If Vyro stops, crashes or loses its sensor readings, the BIOS takes the fans back.</li>
        <li>Closing the window does not stop fan control. Use “Return to BIOS” to hand everything back.</li>
        <li>Close MSI Center, FanControl and other fan tools first: two programs driving the same fans fight.</li>
      </ul>
      <div class="actions">
        <button type="button" onclick={() => confirm?.close()}>Cancel</button>
        <button type="button" class="primary" onclick={turnOn}>Turn on</button>
      </div>
    </dialog>
  {/if}
</section>

<style>
  .empty h2,
  .head h2 {
    margin: 0 0 0.35rem;
    font-size: 1rem;
  }
  .empty p {
    max-width: 44rem;
    margin: 0 0 0.4rem;
    font-size: 0.88rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    margin-bottom: 0.9rem;
  }
  .sub {
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  button {
    padding: 0.35rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .primary {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
  }
  .explain {
    margin: -0.4rem 0 0.9rem;
    font-size: 0.82rem;
  }
  .explain summary {
    color: var(--muted);
    cursor: pointer;
  }
  .explain p {
    max-width: 52rem;
    margin: 0.4rem 0 0;
    color: var(--muted);
  }
  .alert {
    margin: 0 0 0.9rem;
    padding: 0.55rem 0.85rem;
    border: 1px solid color-mix(in srgb, var(--hot) 55%, var(--line));
    border-radius: 10px;
    background: color-mix(in srgb, var(--hot) 10%, var(--panel));
    font-size: 0.85rem;
  }
  .pending {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin: 0 0 0.9rem;
    padding: 0.5rem 0.85rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 10px;
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
    font-size: 0.85rem;
  }
  .state {
    margin: 0 0 0.6rem;
    font-size: 0.82rem;
  }
  .tag {
    margin-right: 0.35rem;
    padding: 0.08rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .tag.on {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    color: var(--accent);
  }
  .note {
    margin: 0.4rem 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .note.boost {
    color: var(--text);
  }
  .modes {
    display: inline-flex;
    margin-bottom: 0.7rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    overflow: hidden;
  }
  .modes button {
    border: 0;
    border-radius: 0;
  }
  .modes button + button {
    border-left: 1px solid var(--line);
  }
  .modes .selected {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg));
    color: var(--text);
  }
  .slider {
    display: grid;
    gap: 0.3rem;
    font-size: 0.82rem;
  }
  .slider small {
    color: var(--muted);
    font-size: 0.74rem;
  }
  input[type='range'] {
    width: 100%;
    accent-color: var(--accent);
  }
  .curve-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    margin-bottom: 0.5rem;
    font-size: 0.82rem;
  }
  .curve-bar label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .presets {
    display: flex;
    gap: 0.3rem;
  }
  .presets button {
    padding: 0.2rem 0.55rem;
    font-size: 0.76rem;
  }
  .presets .selected {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg));
  }
  .slider + .presets {
    margin-top: 0.5rem;
  }
  .note.warn {
    color: var(--warn);
  }
  .slot-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.8rem;
    margin: 0 0 0.75rem;
    font-size: 0.82rem;
  }
  .slot-bar label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .slot-bar .note {
    flex-basis: 100%;
    margin: 0;
  }
  fieldset.plain {
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  fieldset.plain:disabled {
    opacity: 0.6;
  }
  .board-name {
    margin-left: 0.3rem;
    color: var(--muted);
  }
  .rename {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-top: 0.7rem;
  }
  .rename input {
    flex: 1 1 10rem;
    min-width: 0;
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
  }
  .events {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.8rem;
  }
  .events li {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.6rem;
  }
  .events time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .events .bad span {
    color: var(--warn);
  }
  .confirm-pump {
    margin-top: 0.2rem;
  }
  .pump-mark {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-top: 0.7rem;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .pump-mark input {
    accent-color: var(--accent);
  }
  button.link {
    display: block;
    margin-top: 0.7rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    font-size: 0.76rem;
    text-decoration: underline;
    cursor: pointer;
  }
  select,
  input[type='number'] {
    padding: 0.2rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  select {
    max-width: 14rem;
  }
  input[type='number'] {
    width: 4.2rem;
  }
  details {
    margin-top: 0.5rem;
    font-size: 0.82rem;
  }
  summary {
    color: var(--muted);
    cursor: pointer;
  }
  .smooth {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 1.2rem;
    margin-top: 0.5rem;
    font-size: 0.82rem;
  }
  .smooth label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  dialog {
    max-width: 34rem;
    padding: 1.2rem 1.4rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 55%);
  }
  dialog h2 {
    margin: 0 0 0.6rem;
    font-size: 1.05rem;
  }
  dialog p,
  dialog li {
    font-size: 0.86rem;
  }
  dialog ul {
    margin: 0 0 1rem;
    padding-left: 1.2rem;
  }
  dialog li + li {
    margin-top: 0.3rem;
  }
  dialog .actions {
    justify-content: flex-end;
  }
</style>
