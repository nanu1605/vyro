<script lang="ts">
  import { tick, untrack } from 'svelte';
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import Meter from './Meter.svelte';
  import FrameTimeGraph from './FrameTimeGraph.svelte';
  import LoadState from './LoadState.svelte';
  import GameProfiles from './GameProfiles.svelte';
  import { fmt, fmtDateTime, fmtDuration, NA } from '../format';
  import { newRule, pollLabel, ruleFor, withRule } from '../games';
  import { getGameSessions, keepProfile } from '../ipc';
  import { fmtTemp, game as automation, prefs, profiles, savePrefs } from '../prefs.svelte';
  import type { Profile } from '../types';
  import type { BackendStatus, Bound, GameSession, SensorSnapshot } from '../types';
  import { scrollable } from '../scrollable';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
    onsettings?: () => void;
  }
  let { snap, backend, active, onsettings }: Props = $props();

  const game = $derived(snap?.game ?? null);
  const live = $derived(game?.state === 'capturing');

  let sessions = $state.raw<GameSession[] | null>(null);
  let error = $state<string | null>(null);
  function load(): void {
    getGameSessions()
      .then((s) => {
        sessions = s;
        error = null;
      })
      .catch((e: unknown) => (error = String(e)));
  }
  // On first show, and whenever a capture ends (its session may have been logged).
  const pid = $derived(game?.pid ?? null);
  $effect(() => {
    if (!active) return;
    void pid;
    untrack(load);
  });

  const BOUND: Record<Bound, { title: string; text: string }> = {
    gpu: {
      title: 'Graphics card limited',
      text: 'The GPU is busy for nearly the whole frame. Lower resolution or graphics settings (or enable upscaling) for more FPS.',
    },
    cpu: {
      title: 'Processor limited',
      text: 'The game’s CPU work takes nearly the whole frame and the GPU waits. Settings that load the CPU (view distance, crowds, simulation) matter most.',
    },
    capped: {
      title: 'Capped',
      text: 'Neither side is busy the whole frame: V-Sync, a frame limiter or the display’s refresh rate holds the frame rate.',
    },
  };
  const share = (busy: number | null | undefined, frame: number | null | undefined) =>
    busy != null && frame ? Math.min(100, (busy / frame) * 100) : null;
  /** Exclusive fullscreen: the game owns the display ("Hardware: Legacy Flip" / "…Copy to front
   * buffer"), so desktop windows such as the overlay cannot appear over it. */
  const exclusive = $derived(game?.presentMode?.startsWith('Hardware: Legacy') ?? false);
  const running = $derived(game && snap ? Math.max(0, snap.ts - game.started) : null);
  const boundLabel = (b: Bound | null) => (b ? BOUND[b].title : NA);

  // Automatic game profiles (P13).
  const auto = $derived(automation.state);
  const nameOf = (p: Profile) => profiles.list.find((x) => x.id === p)?.name ?? p;
  const pollOf = (p: Profile) => profiles.list.find((x) => x.id === p)?.pollMs ?? 1000;
  let autoError = $state<string | null>(null);
  async function act(run: () => Promise<void>): Promise<void> {
    try {
      await run();
      autoError = null;
    } catch (e) {
      autoError = String(e);
    }
  }
  // Session replay (P15): opens in place of the tab (its code loads on first use); closing it
  // returns focus to its row.
  let replay = $state<number | null>(null);
  const loadReplay = () => import('./SessionReplay.svelte');
  let list = $state<HTMLElement | null>(null);
  async function closeReplay(): Promise<void> {
    const id = replay;
    replay = null;
    await tick();
    list?.querySelector<HTMLButtonElement>(`[data-replay="${id}"]`)?.focus();
  }

  function notAGame(exe: string): Promise<void> {
    const rule = ruleFor(prefs.games, exe) ?? newRule(exe, game?.app ?? exe);
    return savePrefs({ games: withRule(prefs.games, { ...rule, ignore: true }) });
  }
</script>

{#if replay != null}
  {#await loadReplay()}
    <p class="note" role="status">Opening the replay…</p>
  {:then { default: SessionReplay }}
    <SessionReplay id={replay} {snap} onclose={() => void closeReplay()} />
  {:catch e}
    <LoadState what="the replay" error={String(e)} retry={() => (replay = null)} />
  {/await}
{:else}
<section class="panel-wrap">
  {#if !game}
    <div class="empty panel" role="status">
      <h2>No game running</h2>
      {#if !prefs.gameCapture}
        <p>
          Frame rate measurement is off.
          {#if onsettings}<button type="button" class="link" onclick={onsettings}>Turn it on in Settings</button>{/if}
        </p>
      {:else}
        <p>
          Start a game in fullscreen or borderless mode and Vyro measures it automatically: FPS, 1% and 0.1% lows, frame
          times, what limits the frame rate, and a summary when you stop playing.
        </p>
        {#if backend?.mode === 'local'}
          <p class="note">Measuring needs the Vyro sensor service, which is not running.</p>
        {/if}
      {/if}
    </div>
  {:else if game.state === 'unavailable'}
    <div class="empty panel" role="alert">
      <h2>{game.app}: frame rate not available</h2>
      <p>{game.detail ?? 'Frame capture did not start.'}</p>
      <p class="note">
        Vyro measures frames with Intel PresentMon, run by the Vyro sensor service.
        {backend?.mode === 'local'
          ? 'The service is not running, so capture lacks the rights it needs: reinstall Vyro or start “Vyro Sensor Service” in Services.'
          : 'If this persists, reinstalling Vyro restores PresentMon.'}
      </p>
    </div>
  {:else}
    <div class="head">
      <div>
        <h2>{game.app}</h2>
        <p class="sub">
          {[game.runtime, game.presentMode].filter(Boolean).join(' · ') || 'Waiting for frames…'}
          {#if running != null}· playing for {fmtDuration(running)}{/if}
        </p>
      </div>
      {#if game.state === 'idle'}
        <span class="badge" role="status">Paused: no frames (menu, loading or minimized)</span>
      {:else if game.state === 'starting'}
        <span class="badge" role="status">Starting capture…</span>
      {/if}
    </div>

    {#if auto?.game && (auto.auto || auto.suppressed || auto.alert)}
      <div class="auto" role="status">
        {#if auto.alert}
          <p><b>{nameOf(auto.profile)} profile</b> while the alert “{auto.alert}” holds.</p>
        {:else if auto.suppressed}
          <p>Your <b>{nameOf(auto.base)}</b> profile stays while {game.app} runs.</p>
        {:else}
          <p>
            <b>{nameOf(auto.profile)} profile</b> while {game.app} runs{auto.fans !== auto.profile
              ? `, with ${nameOf(auto.fans)}'s fan settings`
              : ''}{auto.pollMs !== pollOf(auto.profile) ? `, readings every ${pollLabel(auto.pollMs)}` : ''}.
            {nameOf(auto.base)} comes back when it closes.
          </p>
          <div class="auto-actions">
            <button type="button" onclick={() => void act(keepProfile)}>Keep {nameOf(auto.base)}</button>
            <button type="button" onclick={() => void act(() => notAGame(auto.game!))}>Not a game</button>
          </div>
        {/if}
        {#if autoError}<p class="error" role="alert">{autoError}</p>{/if}
      </div>
    {/if}

    {#if exclusive && prefs.overlay.enabled}
      <p class="notice" role="status">
        {game.app} runs in exclusive fullscreen ({game.presentMode}), which no window can draw over, so the overlay
        cannot show. Choose borderless (or windowed fullscreen) in the game’s display settings to see it.
      </p>
    {/if}

    <div class="tiles">
      <div class="panel tile big">
        <span class="label">FPS</span>
        <span class="value">{fmt(game.fps)}</span>
        <span class="hint">last second</span>
      </div>
      <div class="panel tile">
        <span class="label">1% low</span>
        <span class="value">{fmt(game.low1Fps)}</span>
        <span class="hint">last 10 s</span>
      </div>
      <div class="panel tile">
        <span class="label">0.1% low</span>
        <span class="value">{fmt(game.low01Fps)}</span>
        <span class="hint">last 10 s</span>
      </div>
      <div class="panel tile">
        <span class="label">Frame time</span>
        <span class="value">{fmt(game.frameMs, 1)}<small>{game.frameMs != null ? 'ms' : ''}</small></span>
        <span class="hint">average</span>
      </div>
      <div class="panel tile">
        <span class="label">Latency</span>
        <span class="value">{fmt(game.latencyMs, 1)}<small>{game.latencyMs != null ? 'ms' : ''}</small></span>
        <span class="hint">frame start to screen</span>
      </div>
    </div>

    <div class="grid">
      <Card title="Frame times" wide>
        <FrameTimeGraph points={game.timeline} span={100} height={140} />
        <p class="note">
          Last 10 seconds: the line is the average frame time per 0.1 s, the shaded band reaches the slowest frame, so
          stutters show as spikes.
        </p>
      </Card>

      <Card title="What limits the frame rate" subtitle={live ? boundLabel(game.bound) : null}>
        <div class="meters">
          <Meter label="CPU busy" value={share(game.cpuBusyMs, game.frameMs)} />
          <Meter label="GPU busy" value={share(game.gpuBusyMs, game.frameMs)} />
        </div>
        <p class="note">
          {game.bound && live ? BOUND[game.bound].text : 'Shares of each frame the processor and graphics card spend working.'}
        </p>
      </Card>

      <Card title="This session">
        <KV
          rows={[
            ['Average FPS', fmt(game.sessionFps, 1)],
            ['1% low', fmt(game.sessionLow1Fps, 1)],
            ['Frames', fmt(game.frames)],
            ['Displayed', game.displayedFps != null ? `${fmt(game.displayedFps)} FPS` : null],
            ['CPU busy', game.cpuBusyMs != null ? `${fmt(game.cpuBusyMs, 1)} ms` : null],
            ['GPU busy', game.gpuBusyMs != null ? `${fmt(game.gpuBusyMs, 1)} ms` : null],
          ]}
        />
      </Card>
    </div>
  {/if}

  <div class="profiles">
    <GameProfiles {sessions} playing={auto?.game ? (game?.app ?? auto.game) : null} />
  </div>

  <h2 class="section">Sessions</h2>
  {#if error || !sessions}
    <LoadState what="game sessions" {error} loading={!sessions} retry={load} />
  {:else if sessions.length === 0}
    <p class="note">Games you play for at least 30 seconds are listed here with their frame rates and temperatures, and a replay of each.</p>
  {:else}
    <div class="table-wrap panel" bind:this={list} use:scrollable role="region" aria-label="Game sessions">
      <table>
        <thead>
          <tr>
            <th scope="col">Game</th>
            <th scope="col">Played</th>
            <th scope="col" class="num">Avg FPS</th>
            <th scope="col" class="num">1% low</th>
            <th scope="col" class="num">0.1% low</th>
            <th scope="col">Limited by</th>
            <th scope="col" class="num">CPU max</th>
            <th scope="col" class="num">GPU max</th>
            <th scope="col"><span class="sr-only">Replay</span></th>
          </tr>
        </thead>
        <tbody>
          {#each sessions as s (s.id || s.started + s.app)}
            <tr>
              <td><b>{s.app}</b><small>{s.runtime ?? ''}</small></td>
              <td>{fmtDateTime(s.started)}<small>{fmtDuration(s.secs)}</small></td>
              <td class="num">{fmt(s.avgFps)}</td>
              <td class="num">{fmt(s.low1Fps)}</td>
              <td class="num">{fmt(s.low01Fps)}</td>
              <td>{s.bound ? BOUND[s.bound].title : NA}</td>
              <td class="num">{fmtTemp(s.cpuTempMax) ?? NA}</td>
              <td class="num">{fmtTemp(s.gpuTempMax) ?? NA}</td>
              <td>
                {#if s.hasTimeline}
                  <button
                    type="button"
                    class="replay"
                    data-replay={s.id}
                    aria-label="Replay {s.app}, {fmtDateTime(s.started)}"
                    onclick={() => (replay = s.id)}>Replay</button
                  >
                {:else}
                  <small>Summary only</small>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>
{/if}

<style>
  .empty {
    margin-bottom: 1rem;
  }
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
  .badge {
    padding: 0.25rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--warn) 50%, var(--line));
    border-radius: 999px;
    color: var(--warn);
    font-size: 0.78rem;
  }
  .auto {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    margin: 0 0 0.9rem;
    padding: 0.55rem 0.85rem;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--line));
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 7%, var(--panel));
    font-size: 0.85rem;
  }
  .auto p {
    margin: 0;
  }
  .auto-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .auto button {
    padding: 0.3rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .auto .error {
    flex-basis: 100%;
    color: var(--hot);
  }
  .profiles {
    margin-top: 1rem;
  }
  .notice {
    margin: 0 0 0.9rem;
    padding: 0.55rem 0.85rem;
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    border-radius: 10px;
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
    font-size: 0.85rem;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 9rem), 1fr));
    gap: 0.75rem;
  }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    padding: 0.75rem 0.9rem;
  }
  .label {
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .value {
    font-family: var(--font-num);
    font-size: 1.6rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .big .value {
    color: var(--accent-gpu);
    font-size: 2.2rem;
    line-height: 1.1;
  }
  .value small {
    margin-left: 0.2em;
    color: var(--muted);
    font-size: 0.55em;
    font-weight: 500;
  }
  .hint {
    color: var(--muted);
    font-size: 0.72rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr));
    gap: 0.75rem;
    margin-top: 1rem;
  }
  .meters {
    display: grid;
    gap: 0.5rem;
  }
  .note {
    margin: 0.6rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .section {
    margin: 1.4rem 0 0.5rem;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .table-wrap {
    position: relative;
    overflow-x: auto;
    padding: 0.4rem 0.6rem;
  }
  .replay {
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    white-space: nowrap;
    cursor: pointer;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
  }
  th {
    padding: 0.4rem 0.5rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 500;
    text-align: left;
    white-space: nowrap;
  }
  td {
    padding: 0.45rem 0.5rem;
    border-top: 1px solid var(--line);
    vertical-align: top;
  }
  td small {
    display: block;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .num {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-decoration: underline dotted var(--muted);
    text-underline-offset: 3px;
    cursor: pointer;
  }
</style>
