<script lang="ts">
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import CurveEditor from './CurveEditor.svelte';
  import { defaultCurve, PRESETS, type Preset } from '../curve';
  import { fmt } from '../format';
  import { getFanStatus, getSensorTree, restoreFans, setFanConfig } from '../ipc';
  import { fmtTemp } from '../prefs.svelte';
  import type { BackendStatus, FanConfig, FanStatus, HeaderMode, HeaderStatus, SensorSnapshot } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
    /** Tells the header when Vyro drives fans (for the "fan control active" chip). */
    onstatus?: (s: FanStatus) => void;
  }
  let { snap, backend, active, onstatus }: Props = $props();

  let status = $state<FanStatus | null>(null);
  /** The configuration being edited; `null` follows the engine's. */
  let draft = $state<FanConfig | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let sources = $state<Array<{ id: string; name: string }>>([]);
  let confirm = $state<HTMLDialogElement | null>(null);

  const config = $derived(draft ?? status?.config ?? null);
  const dirty = $derived(draft != null);
  const writable = $derived(status?.capability.state === 'writable');

  function refresh(): void {
    getFanStatus(snap)
      .then((s) => {
        status = s;
        onstatus?.(s);
      })
      .catch((e: unknown) => (error = String(e)));
  }
  // Every tick while shown.
  const ts = $derived(snap?.ts ?? 0);
  $effect(() => {
    if (!active) return;
    void ts;
    untrack(refresh);
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

  function modeOf(c: FanConfig, index: number): HeaderMode {
    return c.headers.find((h) => h.index === index)?.mode ?? { mode: 'bios' };
  }

  function setMode(index: number, mode: HeaderMode): void {
    edit((c) => {
      const h = c.headers.find((x) => x.index === index);
      if (h) h.mode = mode;
      else c.headers.push({ index, mode });
    });
  }

  function pick(h: HeaderStatus, kind: HeaderMode['mode']): void {
    const cur = config ? modeOf(config, h.index) : null;
    if (cur?.mode === kind) return;
    if (kind === 'bios') setMode(h.index, { mode: 'bios' });
    else if (kind === 'fixed') setMode(h.index, { mode: 'fixed', pct: Math.max(h.minPct, Math.round(h.dutyPct ?? 50)) });
    else setMode(h.index, { mode: 'curve', curve: defaultCurve() });
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
    (status?.headers ?? []).filter((x) => x.index !== h.index && x.controllable && x.connected);
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

    {#if status.overrideReason}
      <p class="alert" role="alert">{status.overrideReason}</p>
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

    <div class="grid">
      {#each status.headers as h (h.index)}
        {@const mode = modeOf(config, h.index)}
        <Card
          title={h.name}
          subtitle={h.connected ? `${fmt(h.rpm)} RPM · ${fmt(h.dutyPct)} %` : 'No fan detected'}
          wide={mode.mode === 'curve'}
        >
          <p class="state">
            {#if h.software}
              <span class="tag on">Vyro</span> target {fmt(h.targetPct)} %
              {#if h.sourceC != null}at {t(h.sourceC)}{/if}
            {:else}
              <span class="tag">BIOS</span> following the BIOS curve
            {/if}
          </p>
          {#if !h.controllable}
            <p class="note">
              This header drives a pump. It stays on the BIOS curve; pump control comes in a later update.
            </p>
          {:else}
            {#if !h.connected}
              <p class="note">Nothing seems to be plugged in (0 RPM). You can still set it.</p>
            {/if}
            <div class="modes" role="radiogroup" aria-label="{h.name} mode">
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
                  min={h.minPct}
                  max="100"
                  step="1"
                  value={mode.pct}
                  oninput={(e) => setMode(h.index, { mode: 'fixed', pct: Number(e.currentTarget.value) })}
                />
                <small>Minimum {h.minPct} %</small>
              </label>
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
                      {#each others(h) as o (o.index)}<option value={o.index}>{o.name}</option>{/each}
                    </select>
                  </label>
                {/if}
              </div>
              <CurveEditor
                label="{h.name} curve"
                points={curve.points}
                minPct={h.minPct}
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
          {/if}
        </Card>
      {/each}

      <Card title="Safety" subtitle="Full speed above these temperatures">
        <div class="smooth">
          <label>
            CPU
            <input
              type="number"
              min="40"
              max="105"
              step="1"
              value={config.emergencyCpuC}
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
              value={config.emergencyGpuC}
              onchange={(e) => edit((c) => (c.emergencyGpuC = Number(e.currentTarget.value)))}
            /> °C
          </label>
        </div>
        <p class="note">
          Every fan Vyro drives goes to 100 % when either limit is reached, or when a curve’s temperature has no reading.
          Vyro never sets a fan below 20 % and hands every fan back to the BIOS when it stops, crashes or loses its
          sensors. Closing this window does not stop fan control; the Vyro service keeps running it.
        </p>
      </Card>
    </div>

    <dialog bind:this={confirm} aria-labelledby="fan-confirm-title">
      <h2 id="fan-confirm-title">Turn on fan control?</h2>
      <p>Vyro will set the speed of the fans you choose instead of the BIOS.</p>
      <ul>
        <li>Fans never go below 20 %; the pump stays on the BIOS curve.</li>
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
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr));
    gap: 0.75rem;
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
