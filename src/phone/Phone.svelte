<script lang="ts">
  // The phone app (1.4): a PC's readings from anywhere, through the Vyro relay, sealed end to
  // end. Installable from the browser (no app store). From 1.5 it also shows what runs on the
  // PC, its devices and Health, and, on a phone the owner allowed on the PC, changes it: ends a
  // process, shuts it down, wakes another PC, and the rest Vyro does (screens/).
  import { onMount } from 'svelte';
  import { fmtDate } from '../format';
  import { applyLook, type AccentChoice, type ThemeId } from '../themes';
  import type { RemoteView } from '../types';
  import { deviceName, pageTitle, STALE_SECS } from '../remote/companion';
  import LiveView from '../remote/LiveView.svelte';
  import { addPc, confirmNext, linkFrom, loadPcs, MAX_PCS, parseLink, removePc, savePcs, sleepers, updatePc, type Link, type PairedPc } from './pcs';
  import { PcLink, type LinkState, type PhoneInfo } from './relay';
  import { errText } from './screens/ask';
  import { b64url, randomBytes, SECRET_LEN } from './seal';

  type Screen = 'home' | 'pc' | 'scan' | 'install';
  /** A PC's tabs (1.5); only Live for a PC whose Vyro is older. */
  type Tab = 'live' | 'processes' | 'devices' | 'health' | 'more';
  const TABS: Array<{ id: Tab; label: string }> = [
    { id: 'live', label: 'Live' },
    { id: 'processes', label: 'Processes' },
    { id: 'devices', label: 'Devices' },
    { id: 'health', label: 'Health' },
    { id: 'more', label: 'More' },
  ];

  /** Chrome's install prompt (Android, desktop). */
  interface InstallPrompt extends Event {
    prompt(): Promise<void>;
  }

  const ua = navigator.userAgent;
  const ios = /iPhone|iPad|iPod/.test(ua) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
  const standalone =
    matchMedia('(display-mode: standalone)').matches || (navigator as Navigator & { standalone?: boolean }).standalone === true;
  const NAME = deviceName(ua);

  let pcs = $state<PairedPc[]>(loadPcs());
  let current = $state<string | null>(null);
  let screen = $state<Screen>('home');
  let info = $state<PhoneInfo | null>(null);
  let view = $state.raw<RemoteView | null>(null);
  let lastAt = $state(0);
  let now = $state(Date.now());
  let linkState = $state<LinkState>('connecting');
  /** States of the PCs listed on the home screen. */
  let listStates = $state<Record<string, LinkState>>({});
  /** A pairing link held while an iPhone user decides where to install. */
  let held = $state<Link | null>(null);
  let notice = $state<string | null>(null);
  let pasted = $state('');
  let installPrompt = $state<InstallPrompt | null>(null);
  let tab = $state<Tab>('live');
  let waking = $state<string | null>(null);

  let link: PcLink | null = null;
  let listLinks: PcLink[] = [];

  const pc = $derived(pcs.find((p) => p.room === current));
  /** The PC answers requests (protocol 2), and which this phone may make. */
  const asks = $derived((info?.methods?.length ?? 0) > 0);
  const can = (m: string) => info?.methods?.includes(m) ?? false;
  const ask = <T,>(m: string, p: object = {}) =>
    link ? link.request<T>(m, p) : Promise.reject<T>(new Error('Not connected to the PC.'));
  const others = $derived(sleepers(pcs, current));
  const unit = $derived(info?.unit ?? 'c');
  const stale = $derived(screen === 'pc' && linkState === 'online' && (lastAt === 0 || now - lastAt > STALE_SECS * 1000));
  const STATE_TEXT: Record<LinkState, string> = {
    connecting: 'Connecting…',
    online: 'Online',
    asleep: 'Asleep or off',
    unreachable: 'Can’t reach the relay',
    signedOut: 'Signed out',
    expired: 'Pairing expired',
    elsewhere: 'Open in another window',
    tooOld: 'Update Vyro on the PC',
  };

  $effect(() => {
    document.title = screen === 'pc' ? pageTitle(pc?.name ?? '', view) : 'Vyro';
  });

  function save(next: PairedPc[]): void {
    pcs = next;
    savePcs(next);
  }

  function stopList(): void {
    for (const l of listLinks) l.stop();
    listLinks = [];
  }

  /** The home screen shows each PC's state: a short connection to each, without readings. */
  function startList(): void {
    stopList();
    listStates = {};
    listLinks = pcs.map((p) => {
      const l = new PcLink(
        () => pcs.find((q) => q.room === p.room),
        {
          onState: (s) => (listStates = { ...listStates, [p.room]: s }),
          onInfo: (i) => save(updatePc(pcs, p.room, { name: i.pc || p.name, wake: i.wake?.mac })),
          onView: () => {},
          onRekeyed: () => save(confirmNext(pcs, p.room)),
        },
        { name: NAME, watch: false, base: location.origin },
      );
      l.start();
      return l;
    });
  }

  function goHome(): void {
    link?.stop();
    link = null;
    screen = 'home';
    if (pcs.length) startList();
  }

  function openPc(room: string): void {
    stopList();
    link?.stop();
    current = room;
    info = null;
    view = null;
    lastAt = 0;
    linkState = 'connecting';
    screen = 'pc';
    tab = 'live';
    notice = null;
    save(updatePc(pcs, room, { used: Date.now() }));
    link = new PcLink(
      () => pcs.find((p) => p.room === room),
      {
        onState: (s) => (linkState = s),
        onInfo: (i) => {
          info = i;
          save(updatePc(pcs, room, { name: i.pc || (pcs.find((p) => p.room === room)?.name ?? ''), wake: i.wake?.mac }));
          applyLook({ theme: i.theme as ThemeId, accent: i.accent as AccentChoice, openOn: 'dashboard' });
        },
        onView: (v) => {
          view = v;
          lastAt = Date.now();
        },
        onRekeyed: () => save(confirmNext(pcs, room)),
      },
      { name: NAME, watch: !document.hidden, base: location.origin },
    );
    link.start();
  }

  function pairWith(l: Link): void {
    held = null;
    if (pcs.length >= MAX_PCS && !pcs.some((p) => p.room === l.room)) {
      notice = `This phone is paired with ${MAX_PCS} PCs already. Remove one first.`;
      screen = 'home';
      return;
    }
    // The phone's own secret: the PC takes it in place of the link's, so the link works once.
    save(addPc(pcs, l, b64url(randomBytes(SECRET_LEN)), Date.now()));
    openPc(l.room);
  }

  /** A pairing link in the address (the PC's QR code opened it). */
  function takeLink(): void {
    const hash = location.hash;
    if (!hash) return;
    // Keep the secret out of the history.
    history.replaceState(null, '', location.pathname);
    const l = parseLink(hash);
    if (!l) {
      if (hash.startsWith('#p=')) notice = 'This pairing link is incomplete. Scan the code in Vyro again.';
      return;
    }
    if (ios && !standalone) {
      // A Home Screen app keeps its own storage, apart from Safari's: pair there, not here.
      held = l;
      screen = 'install';
      return;
    }
    pairWith(l);
  }

  function fromText(text: string): void {
    const l = linkFrom(text, location.origin);
    if (l === 'elsewhere') notice = 'That code is for another Vyro relay. Open it with the camera app instead.';
    else if (!l) notice = 'That is not a Vyro pairing link. In Vyro, open Settings → Remote → Pair a phone.';
    else pairWith(l);
  }

  async function leave(): Promise<void> {
    const room = current;
    if (!room) return;
    await link?.signOut();
    link = null;
    forget(room);
  }

  function forget(room: string): void {
    link?.stop();
    link = null;
    save(removePc(pcs, room));
    current = null;
    info = null;
    view = null;
    goHome();
  }

  /** Has another paired PC, awake and allowing this phone, send `target` the wake signal. */
  async function wakeThrough(target: PairedPc): Promise<void> {
    if (!target.wake) return;
    waking = `Asking another PC to wake ${target.name}…`;
    for (const helper of pcs.filter((p) => p.room !== target.room)) {
      const result = await new Promise<string | null>((resolve) => {
        let done = false;
        const finish = (r: string | null) => {
          if (done) return;
          done = true;
          clearTimeout(timer);
          l.stop();
          resolve(r);
        };
        const timer = setTimeout(() => finish(null), 12_000);
        const l: PcLink = new PcLink(
          () => pcs.find((q) => q.room === helper.room),
          {
            onState: (s) => {
              if (s !== 'online' && s !== 'connecting') finish(null);
            },
            onInfo: (i) => {
              if (!i.methods?.includes('wake')) return finish(null);
              l.request('wake', { mac: target.wake, pc: target.name }).then(
                () => finish(helper.name),
                () => finish(null),
              );
            },
            onView: () => {},
            onRekeyed: () => save(confirmNext(pcs, helper.room)),
          },
          { name: NAME, watch: false, base: location.origin },
        );
        l.start();
      });
      if (result) {
        waking = `${result} sent ${target.name} the wake signal. It shows here once it is up.`;
        return;
      }
    }
    waking = `No other PC could send it. One of them has to be on, on the same network, and let this phone control it.`;
  }

  async function install(): Promise<void> {
    const p = installPrompt;
    installPrompt = null;
    await p?.prompt();
  }

  onMount(() => {
    takeLink();
    if (screen === 'home') {
      const first = pcs[0];
      if (first && !notice) openPc(first.room);
      else if (pcs.length) startList();
    }
    const tick = setInterval(() => (now = Date.now()), 1000);
    const onVisibility = () => {
      link?.setWatch(!document.hidden);
      if (!document.hidden) {
        link?.wake();
        for (const l of listLinks) l.wake();
      }
    };
    const onOnline = () => {
      link?.wake();
      for (const l of listLinks) l.wake();
    };
    const onPrompt = (e: Event) => {
      e.preventDefault();
      installPrompt = e as InstallPrompt;
    };
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('online', onOnline);
    window.addEventListener('hashchange', takeLink);
    window.addEventListener('beforeinstallprompt', onPrompt);
    if ('serviceWorker' in navigator && import.meta.env.PROD) {
      void navigator.serviceWorker.register('/sw.js').catch(() => {});
    }
    void navigator.storage?.persist?.().catch(() => false);
    return () => {
      clearInterval(tick);
      link?.stop();
      stopList();
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('online', onOnline);
      window.removeEventListener('hashchange', takeLink);
      window.removeEventListener('beforeinstallprompt', onPrompt);
    };
  });

  const chip = $derived(
    screen !== 'pc' ? null : linkState === 'online' ? (stale ? 'Waiting for readings…' : 'Live') : STATE_TEXT[linkState],
  );
</script>

<div class="page">
  <header>
    <div class="top">
      <p class="brand">
        <span class="logo">VYRO</span>{#if screen === 'pc' && pc}<span class="pc">{pc.name}</span>{/if}
      </p>
      {#if chip}
        <p class="live" class:wait={chip !== 'Live'} role="status">{chip}</p>
      {/if}
    </div>
    <div class="bar-row">
      <p class="tagline">
        End-to-end encrypted{screen === 'pc' && info ? (info.control ? ' · can change this PC' : ' · view only') : ''}
      </p>
      {#if screen === 'pc' || screen === 'scan'}
        <button type="button" class="ghost small-btn" onclick={goHome}>{pcs.length > 1 ? 'All PCs' : 'PCs'}</button>
      {/if}
    </div>
  </header>

  <main>
    {#if notice}
      <p class="note warn" role="alert">{notice}</p>
    {/if}

    {#if screen === 'install' && held}
      <section class="panel" aria-labelledby="install-title">
        <h1 id="install-title">Add Vyro to your Home Screen first</h1>
        <p>On iPhone, an app on the Home Screen keeps its own pairing, apart from Safari’s. So pair it there:</p>
        <ol>
          <li>Tap <b>Share</b> at the bottom of Safari, then <b>Add to Home Screen</b>.</li>
          <li>Open <b>Vyro</b> from your Home Screen.</li>
          <li>Tap <b>Scan the code</b> and point it at the code in Vyro on your PC (it lasts 10 minutes).</li>
        </ol>
        <div class="actions">
          <button type="button" class="ghost" onclick={() => held && pairWith(held)}>Use it in Safari instead</button>
        </div>
      </section>
    {:else if screen === 'scan'}
      {#await import('./Scan.svelte') then m}
        <m.default
          onlink={(text: string) => fromText(text)}
          oncancel={() => (pcs.length ? goHome() : (screen = 'home'))}
        />
      {/await}
    {:else if screen === 'pc' && pc}
      {#if linkState === 'online' && asks}
        <nav class="tabs" aria-label="{pc.name}">
          {#each TABS as t (t.id)}
            <button type="button" class="tab" aria-current={tab === t.id ? 'page' : undefined} onclick={() => (tab = t.id)}>
              {t.label}
            </button>
          {/each}
        </nav>
      {/if}
      {#if linkState === 'online' && asks && tab !== 'live'}
        {#if tab === 'processes'}
          {#await import('./screens/Processes.svelte') then m}<m.default {ask} {can} pc={pc.name} />{/await}
        {:else if tab === 'devices'}
          {#await import('./screens/Devices.svelte') then m}<m.default {ask} {can} pc={pc.name} />{/await}
        {:else if tab === 'health'}
          {#await import('./screens/Health.svelte') then m}<m.default {ask} {can} pc={pc.name} />{/await}
        {:else}
          {#await import('./screens/More.svelte') then m}<m.default {ask} {can} pc={pc.name} sleepers={others} />{/await}
        {/if}
        {#if !info?.control}
          <p class="muted small">
            This phone may only look. To let it change {pc.name}: Settings → Remote → From anywhere on the PC.
          </p>
        {/if}
      {:else if linkState === 'online' && view}
        <LiveView {view} {unit} pc={pc.name} {now} />
        <footer>
          {#if info}
            <p>{info.device} · paired until {fmtDate(info.expires)}, renewed each time you open it</p>
          {/if}
          <button type="button" class="ghost" onclick={() => void leave()}>Sign out of this PC</button>
          {#if info}<p class="muted small">Vyro {info.version} on the PC</p>{/if}
        </footer>
      {:else if linkState === 'online' || linkState === 'connecting'}
        <p class="muted" role="status">{linkState === 'online' ? 'Waiting for the first reading…' : 'Connecting…'}</p>
      {:else if linkState === 'asleep'}
        <section class="panel" aria-labelledby="state-title">
          <h1 id="state-title">{pc.name} is asleep or off</h1>
          <p>Its readings come back here as soon as it wakes, with Vyro running and <b>Reach from anywhere</b> on.</p>
          {#if pc.wake && pcs.length > 1}
            <div class="actions">
              <button type="button" onclick={() => void wakeThrough(pc).catch((e: unknown) => (waking = errText(e)))}>
                Wake {pc.name}
              </button>
            </div>
          {/if}
          {#if waking}<p role="status">{waking}</p>{/if}
        </section>
      {:else if linkState === 'unreachable'}
        <section class="panel" aria-labelledby="state-title">
          <h1 id="state-title">Can’t reach the Vyro relay</h1>
          <p>Check this phone’s connection. Trying again by itself…</p>
        </section>
      {:else if linkState === 'elsewhere'}
        <section class="panel" aria-labelledby="state-title">
          <h1 id="state-title">Open in another window</h1>
          <p>{pc.name} is open in another window of Vyro on this device.</p>
          <div class="actions"><button type="button" onclick={() => openPc(pc.room)}>Use it here</button></div>
        </section>
      {:else if linkState === 'tooOld'}
        <section class="panel" aria-labelledby="state-title">
          <h1 id="state-title">Update Vyro on {pc.name}</h1>
          <p>This app needs a newer Vyro on the PC. Vyro updates from its tray menu → Check for updates.</p>
        </section>
      {:else}
        <section class="panel" aria-labelledby="state-title">
          <h1 id="state-title">{linkState === 'expired' ? 'That pairing code has expired' : `Signed out of ${pc.name}`}</h1>
          <p>
            {linkState === 'expired'
              ? 'It lasts 10 minutes and works once. In Vyro on the PC, show a new one: Settings → Remote → Pair a phone.'
              : 'The PC signed this phone out, or its pairing ran out. Pair it again from Vyro: Settings → Remote → Pair a phone.'}
          </p>
          <div class="actions"><button type="button" onclick={() => forget(pc.room)}>Remove {pc.name}</button></div>
        </section>
      {/if}
    {:else}
      {#if pcs.length}
        <section class="panel" aria-labelledby="pcs-title">
          <h1 id="pcs-title">Your PCs</h1>
          <ul class="pcs">
            {#each pcs as p (p.room)}
              <li>
                <button type="button" class="pc-row" onclick={() => openPc(p.room)}>
                  <span class="pc-name">{p.name}</span>
                  <span class="pc-state {listStates[p.room] ?? 'connecting'}">{STATE_TEXT[listStates[p.room] ?? 'connecting']}</span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
      <section class="panel" aria-labelledby="pair-title">
        <h1 id="pair-title">{pcs.length ? 'Pair another PC' : 'Pair with your PC'}</h1>
        <p>
          In Vyro on the PC, open <b>Settings → Remote → From anywhere → Pair a phone</b>, then scan the code it shows
          {standalone ? 'here' : 'with this phone’s camera'}.
        </p>
        <div class="actions">
          <button type="button" onclick={() => ((screen = 'scan'), (notice = null), stopList())}>Scan the code</button>
        </div>
        <form
          class="paste"
          onsubmit={(e) => {
            e.preventDefault();
            fromText(pasted);
            pasted = '';
          }}
        >
          <label for="paste">Or paste the pairing link</label>
          <div class="paste-row">
            <input id="paste" bind:value={pasted} autocomplete="off" spellcheck="false" inputmode="url" placeholder="https://…" />
            <button type="submit" class="ghost" disabled={!pasted.trim()}>Pair</button>
          </div>
        </form>
      </section>
      {#if !standalone}
        <section class="panel" aria-labelledby="install-help">
          <h2 id="install-help">Install the app</h2>
          {#if installPrompt}
            <p>Vyro works like an app from your Home Screen, with no app store.</p>
            <div class="actions"><button type="button" onclick={() => void install()}>Install Vyro</button></div>
          {:else if ios}
            <p>In Safari, tap <b>Share</b> → <b>Add to Home Screen</b>, then pair from the app there.</p>
          {:else}
            <p>In your browser’s menu, choose <b>Install app</b> or <b>Add to Home screen</b>.</p>
          {/if}
        </section>
      {/if}
      <p class="muted small">
        Everything travels through the Vyro relay sealed with keys only this phone and your PC have; the relay cannot read
        it. Whether this phone may change a PC is decided on that PC.
      </p>
    {/if}
  </main>
</div>

<style>
  .tabs {
    display: flex;
    gap: 0.25rem;
    margin: 0 0 0.9rem;
    overflow-x: auto;
    border-bottom: 1px solid var(--line);
  }
  .tab {
    flex: none;
    padding: 0.5rem 0.75rem;
    border-radius: 8px 8px 0 0;
    background: transparent;
    color: var(--muted);
    font-weight: 600;
  }
  .tab[aria-current='page'] {
    color: var(--text);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .bar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    margin: 0.35rem 0 1rem;
  }
  .bar-row .tagline {
    margin: 0;
  }
  .small-btn {
    padding: 0.35rem 0.75rem;
    font-size: 0.85rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.75rem;
  }
  ol {
    margin: 0.5rem 0;
    padding-left: 1.25rem;
    line-height: 1.5;
  }
  .pcs {
    display: grid;
    gap: 0.5rem;
    margin: 0.5rem 0 0;
    padding: 0;
    list-style: none;
  }
  .pc-row {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.75rem 0.85rem;
    border: 1px solid var(--line);
    background: var(--bg);
    color: var(--text);
    font-weight: 600;
    text-align: left;
  }
  .pc-name {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .pc-state {
    flex: none;
    color: var(--muted);
    font-size: 0.8rem;
    font-weight: 500;
  }
  .pc-state.online {
    color: var(--text);
  }
  .pc-state.online::before {
    content: '';
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.35rem;
    border-radius: 50%;
    background: var(--ok);
  }
  .paste {
    display: grid;
    gap: 0.35rem;
    margin-top: 1rem;
    font-size: 0.85rem;
  }
  .paste-row {
    display: flex;
    gap: 0.5rem;
  }
  .paste-row input {
    flex: 1;
    min-width: 0;
  }
</style>
