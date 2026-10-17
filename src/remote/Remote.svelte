<script lang="ts">
  // The remote companion page (P26), served by the Vyro app to a paired phone or computer on the
  // same network. Read-only: it shows the PC's readings and can only sign itself out.
  import { onMount } from 'svelte';
  import { fmt, fmtDate, fmtDateTime } from '../format';
  import { applyLook, type AccentChoice, type ThemeId } from '../themes';
  import type { RemoteInfo, RemoteView } from '../types';
  import { getInfo, getView, NotPaired, pair, signOut, stream } from './api';
  import { canonCode, codeFromHash, deviceName, driveRows, fanRows, notices, pageTitle, showCode, STALE_SECS, tempRows, tiles, type Level, type Row } from './companion';

  type Stage = 'loading' | 'pair' | 'live' | 'down';
  let stage = $state<Stage>('loading');
  let info = $state<RemoteInfo | null>(null);
  let view = $state.raw<RemoteView | null>(null);
  let lastAt = $state(0);
  let now = $state(Date.now());
  let signedOut = $state(false);
  let code = $state('');
  let name = $state(deviceName(navigator.userAgent));
  let busy = $state(false);
  let error = $state<string | null>(null);

  let close: (() => void) | null = null;
  let retry: ReturnType<typeof setTimeout> | null = null;

  const unit = $derived(info?.unit ?? 'c');
  const stale = $derived(stage === 'live' && (lastAt === 0 || now - lastAt > STALE_SECS * 1000));
  const notes = $derived(view ? notices(view) : []);
  const cards = $derived(view ? tiles(view, unit) : []);
  const lists = $derived(
    view
      ? [
          { id: 'temps', title: 'Temperatures', rows: tempRows(view, unit) },
          { id: 'fans', title: 'Fans', rows: fanRows(view) },
          { id: 'drives', title: 'Drives', rows: driveRows(view, unit) },
        ].filter((l) => l.rows.length)
      : [],
  );
  const WORD: Record<Exclude<Level, null>, string> = { ok: 'OK', warn: 'Warning', hot: 'Problem' };

  $effect(() => {
    document.title = stage === 'live' ? pageTitle(info?.pc ?? '', view) : 'Vyro';
  });

  function onView(v: RemoteView): void {
    view = v;
    lastAt = Date.now();
  }

  function later(f: () => void, ms: number): void {
    if (retry) clearTimeout(retry);
    retry = setTimeout(f, ms);
  }

  function connect(): void {
    close?.();
    close = null;
    if (document.hidden || stage !== 'live') return;
    close = stream(onView, () => {
      close = null;
      later(() => void check(), 3000);
    });
  }

  /** After the stream broke: signed out, or only away for a moment? */
  async function check(): Promise<void> {
    try {
      await getInfo();
      connect();
    } catch (e) {
      if (e instanceof NotPaired) {
        stage = 'pair';
        signedOut = true;
        view = null;
      } else {
        stage = 'down';
        later(() => void start(), 5000);
      }
    }
  }

  async function start(): Promise<void> {
    try {
      info = await getInfo();
      applyLook({ theme: info.theme as ThemeId, accent: info.accent as AccentChoice, openOn: 'dashboard' });
      stage = 'live';
      void getView()
        .then(onView)
        .catch(() => {});
      connect();
    } catch (e) {
      if (e instanceof NotPaired) {
        stage = 'pair';
      } else {
        stage = 'down';
        later(() => void start(), 5000);
      }
    }
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    const c = canonCode(code);
    if (!c) {
      error = 'Enter the 8-character code shown in Vyro.';
      return;
    }
    busy = true;
    error = null;
    try {
      await pair(c, name.trim());
      signedOut = false;
      code = '';
      await start();
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
    busy = false;
  }

  async function leave(): Promise<void> {
    close?.();
    close = null;
    await signOut().catch(() => {});
    view = null;
    info = null;
    stage = 'pair';
  }

  /** The QR code's address carries the code: take it and drop it from the address bar. */
  function takeCode(): void {
    const fromLink = codeFromHash(location.hash);
    if (fromLink) {
      code = showCode(fromLink);
      error = null;
    }
    if (location.hash) history.replaceState(null, '', location.pathname);
  }

  onMount(() => {
    takeCode();
    void start();
    // A pairing link opened while the page already shows.
    window.addEventListener('hashchange', takeCode);
    const tick = setInterval(() => (now = Date.now()), 1000);
    const onVisibility = () => {
      if (document.hidden) {
        close?.();
        close = null;
      } else if (stage === 'live') {
        void getView()
          .then(onView)
          .catch(() => {});
        connect();
      }
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      clearInterval(tick);
      if (retry) clearTimeout(retry);
      close?.();
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('hashchange', takeCode);
    };
  });

  const ago = (ts: number) => {
    const s = Math.max(0, Math.round(now / 1000 - ts));
    return s < 60 ? `${s} s ago` : s < 3600 ? `${Math.floor(s / 60)} min ago` : fmtDateTime(ts);
  };
  const rowLevel = (r: Row) => (r.level === 'warn' || r.level === 'hot' ? r.level : null);
</script>

<div class="page">
  <header>
    <div class="top">
      <p class="brand"><span class="logo">VYRO</span>{#if info?.pc}<span class="pc">{info.pc}</span>{/if}</p>
      {#if stage === 'live'}
        <p class="live" class:wait={stale} role="status">{stale ? 'Waiting for readings…' : 'Live'}</p>
      {/if}
    </div>
    <p class="lan">Local network · not encrypted · read-only</p>
  </header>

  <main>

    {#if stage === 'loading'}
      <p class="muted" role="status">Connecting to Vyro…</p>
    {:else if stage === 'down'}
      <section class="panel" aria-labelledby="down-title">
        <h1 id="down-title">Can’t reach Vyro</h1>
        <p>
          Vyro has to be running on the PC with Settings → Remote turned on, and this device on the same network. Trying
          again every few seconds…
        </p>
      </section>
    {:else if stage === 'pair'}
      <section class="panel pair" aria-labelledby="pair-title">
        <h1 id="pair-title">Pair with Vyro</h1>
        {#if signedOut}
          <p class="note warn" role="status">This device was signed out. Pair it again to keep watching.</p>
        {/if}
        <p>
          On the PC, open Vyro → Settings → Remote → <b>Pair a device</b>, then scan the QR code with this device or type
          the code shown there.
        </p>
        <form onsubmit={submit}>
          <label>
            <span>Code</span>
            <input
              value={code}
              oninput={(e) => (code = showCode(e.currentTarget.value))}
              autocomplete="one-time-code"
              autocapitalize="characters"
              spellcheck="false"
              maxlength="9"
              placeholder="XXXX-XXXX"
              required
            />
          </label>
          <label>
            <span>This device’s name</span>
            <input bind:value={name} maxlength="40" autocomplete="off" />
          </label>
          {#if error}<p class="error" role="alert">{error}</p>{/if}
          <button type="submit" disabled={busy}>{busy ? 'Pairing…' : 'Pair'}</button>
        </form>
        <p class="muted small">
          A paired device can read the PC’s sensors until it is signed out or its pairing ends. Nothing on this page can
          change the PC.
        </p>
      </section>
    {:else if view}
      <h1 class="sr-only">Readings from {info?.pc || 'this PC'}</h1>
      {#if notes.length}
        <section class="notices" aria-label="Alerts and health">
          <ul>
            {#each notes as n, i (i)}
              <li class={n.level}><span class="sr-only">{n.level === 'hot' ? 'Problem:' : 'Warning:'}</span>{n.text}</li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if view.game}
        <section class="panel game" aria-labelledby="game-title">
          <h2 id="game-title">{view.game.app}</h2>
          <p>
            <span class="num">{fmt(view.game.fps)}</span> FPS
            <span class="muted">· 1% low {fmt(view.game.low1Fps)} · {fmt(view.game.frameMs, 1)} ms</span>
          </p>
        </section>
      {/if}

      <section class="tiles" aria-label="Readings">
        {#each cards as t (t.id)}
          <article class="panel tile {t.level ?? ''}" aria-labelledby="t-{t.id}">
            <h2 id="t-{t.id}">
              {t.title}
              {#if t.level === 'warn' || t.level === 'hot'}<span class="tag {t.level}">{WORD[t.level]}</span>{/if}
            </h2>
            {#if t.sub}<p class="sub">{t.sub}</p>{/if}
            <p class="big"><span class="num">{t.value}</span> <span class="unit">{t.unit}</span></p>
            {#if t.bar != null}
              <div class="bar" aria-hidden="true"><i style:width="{Math.max(0, Math.min(100, t.bar))}%"></i></div>
            {/if}
            <dl>
              {#each t.lines as [k, v] (k)}
                <div><dt>{k}</dt><dd>{v}</dd></div>
              {/each}
            </dl>
          </article>
        {/each}
      </section>

      {#each lists as l (l.id)}
        <section class="panel list" aria-labelledby="l-{l.id}">
          <h2 id="l-{l.id}">{l.title}</h2>
          <ul>
            {#each l.rows as r, i (i)}
              <li>
                <span class="name">{r.name}</span>
                <span class="value {rowLevel(r) ?? ''}">
                  {r.value}
                  {#if rowLevel(r)}<span class="tag {rowLevel(r)}">{WORD[rowLevel(r)!]}</span>{/if}
                </span>
                {#if r.note}<span class="note-text">{r.note}</span>{/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}

      {#if view.recent.length}
        <section class="panel list" aria-labelledby="l-recent">
          <h2 id="l-recent">Recent alerts</h2>
          <ul>
            {#each view.recent as a, i (i)}
              <li class="recent">
                <span class="name">{a.title}</span>
                <span class="note-text">{ago(a.ts)}</span>
                {#if a.message}<span class="msg">{a.message}</span>{/if}
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      <footer>
        {#if info}
          <p>{info.device} · paired until {fmtDate(info.expires)}</p>
        {/if}
        <button type="button" class="ghost" onclick={() => void leave()}>Sign out of this PC</button>
        {#if info}<p class="muted small">Vyro {info.version}</p>{/if}
      </footer>
    {:else}
      <p class="muted" role="status">Waiting for the first reading…</p>
    {/if}
  </main>
</div>

<style>
  .page {
    max-width: 60rem;
    margin: 0 auto;
    padding: max(1rem, env(safe-area-inset-top)) 1rem max(1.5rem, env(safe-area-inset-bottom));
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    min-width: 0;
    margin: 0;
  }
  .logo {
    font-weight: 800;
    letter-spacing: 0.12em;
    color: var(--accent);
  }
  .pc {
    overflow: hidden;
    color: var(--text);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .live {
    flex: none;
    margin: 0;
    padding: 0.15rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--ok) 50%, var(--line));
    border-radius: 999px;
    color: var(--text);
    font-size: 0.78rem;
  }
  .live::before {
    content: '';
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.4rem;
    border-radius: 50%;
    background: var(--ok);
  }
  .live.wait {
    border-color: color-mix(in srgb, var(--warn) 50%, var(--line));
  }
  .live.wait::before {
    background: var(--warn);
  }
  .lan {
    margin: 0.35rem 0 1rem;
    color: var(--muted);
    font-size: 0.78rem;
  }
  h1 {
    margin: 0 0 0.6rem;
    font-size: 1.25rem;
  }
  h2 {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  p {
    line-height: 1.45;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.8rem;
  }
  .panel {
    margin-bottom: 0.75rem;
  }
  .pair form {
    display: grid;
    gap: 0.8rem;
    margin: 1rem 0;
  }
  .pair label {
    display: grid;
    gap: 0.3rem;
    font-size: 0.85rem;
  }
  input {
    width: 100%;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 1rem;
  }
  .pair label:first-child input {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 1.3rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  button {
    padding: 0.65rem 1rem;
    border: 0;
    border-radius: 8px;
    background: var(--accent);
    color: var(--bg);
    font: inherit;
    font-weight: 700;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button.ghost {
    border: 1px solid var(--line);
    background: transparent;
    color: var(--text);
    font-weight: 500;
  }
  .error {
    margin: 0;
    color: var(--hot);
  }
  .note.warn {
    padding: 0.5rem 0.7rem;
    border-left: 3px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 12%, transparent);
  }
  .notices ul {
    display: grid;
    gap: 0.4rem;
    margin: 0 0 0.75rem;
    padding: 0;
    list-style: none;
  }
  .notices li {
    padding: 0.55rem 0.75rem;
    border-left: 3px solid var(--warn);
    border-radius: 6px;
    background: color-mix(in srgb, var(--warn) 14%, var(--panel));
  }
  .notices li.hot {
    border-left-color: var(--hot);
    background: color-mix(in srgb, var(--hot) 16%, var(--panel));
  }
  .game p {
    margin: 0.3rem 0 0;
  }
  .game .num {
    font-size: 1.6rem;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }
  .tile {
    margin: 0;
    min-width: 0;
  }
  .tile.warn {
    border-color: color-mix(in srgb, var(--warn) 60%, var(--line));
  }
  .tile.hot {
    border-color: color-mix(in srgb, var(--hot) 70%, var(--line));
  }
  .sub {
    overflow: hidden;
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .big {
    margin: 0.35rem 0 0.25rem;
  }
  .num {
    font-family: var(--font-num);
    font-size: 2rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .bar {
    height: 4px;
    margin-bottom: 0.5rem;
    border-radius: 2px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  dl {
    display: grid;
    gap: 0.2rem;
    margin: 0;
    font-size: 0.82rem;
  }
  dl div {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .tag {
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
    color: var(--bg);
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: none;
  }
  .tag.warn {
    background: var(--warn);
  }
  .tag.hot {
    background: var(--hot);
  }
  .list ul {
    margin: 0.5rem 0 0;
    padding: 0;
    list-style: none;
  }
  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.25rem 0.75rem;
    padding: 0.45rem 0;
    border-top: 1px solid var(--line);
    font-size: 0.9rem;
  }
  .list li:first-child {
    border-top: 0;
  }
  .name {
    flex: 1 1 10rem;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .value {
    font-variant-numeric: tabular-nums;
  }
  .note-text {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .recent .msg {
    flex-basis: 100%;
    color: var(--muted);
    font-size: 0.82rem;
  }
  footer {
    display: grid;
    justify-items: start;
    gap: 0.5rem;
    margin-top: 1.25rem;
    color: var(--muted);
    font-size: 0.85rem;
  }
  footer p {
    margin: 0;
  }
</style>
