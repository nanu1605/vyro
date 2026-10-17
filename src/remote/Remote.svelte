<script lang="ts">
  // The remote companion page (P26), served by the Vyro app to a paired phone or computer on the
  // same network. Read-only: it shows the PC's readings and can only sign itself out.
  import { onMount } from 'svelte';
  import { fmtDate } from '../format';
  import { applyLook, type AccentChoice, type ThemeId } from '../themes';
  import type { RemoteInfo, RemoteView } from '../types';
  import { getInfo, getView, NotPaired, pair, signOut, stream } from './api';
  import { canonCode, codeFromHash, deviceName, pageTitle, showCode, STALE_SECS } from './companion';
  import LiveView from './LiveView.svelte';

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
</script>

<div class="page">
  <header>
    <div class="top">
      <p class="brand"><span class="logo">VYRO</span>{#if info?.pc}<span class="pc">{info.pc}</span>{/if}</p>
      {#if stage === 'live'}
        <p class="live" class:wait={stale} role="status">{stale ? 'Waiting for readings…' : 'Live'}</p>
      {/if}
    </div>
    <p class="tagline">Local network · not encrypted · read-only</p>
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
      <LiveView {view} {unit} pc={info?.pc ?? ''} {now} />

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
  .pair label:first-child input {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 1.3rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
</style>
