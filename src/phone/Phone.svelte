<script lang="ts">
  // The phone app (1.4): a PC's readings from anywhere, through the Vyro relay, sealed end to
  // end. Installable from the browser (no app store). From 1.5 it also shows what runs on the
  // PC, its devices and Health, and, on a phone the owner allowed on the PC, changes it: ends a
  // process, shuts it down, wakes another PC, and the rest Vyro does (screens/). Redesigned in
  // 1.7: a bar at the top, tabs at the bottom, sheets for choices, toasts for results, and views
  // that move in from the side they are on.
  import { onMount } from 'svelte';
  import Activity from '@lucide/svelte/icons/activity';
  import HeartPulse from '@lucide/svelte/icons/heart-pulse';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import List from '@lucide/svelte/icons/list';
  import Monitor from '@lucide/svelte/icons/monitor';
  import { fmtDate } from '../format';
  import { applyLook, type AccentChoice, type ThemeId } from '../themes';
  import type { RemoteView } from '../types';
  import { deviceName, pageTitle, STALE_SECS } from '../remote/companion';
  import LiveView from '../remote/LiveView.svelte';
  import { addPc, confirmNext, linkFrom, loadPcs, MAX_PCS, parseLink, removePc, savePcs, sleepers, updatePc, type Link, type PairedPc } from './pcs';
  import { PcLink, type LinkState, type PhoneInfo } from './relay';
  import { errText } from './screens/ask';
  import { b64url, randomBytes, SECRET_LEN } from './seal';
  import AppBar, { type ChipTone } from './ui/AppBar.svelte';
  import BottomNav from './ui/BottomNav.svelte';
  import Footer from './ui/Footer.svelte';
  import { direction, ms } from './ui/motion';
  import Toasts from './ui/Toasts.svelte';

  type Screen = 'home' | 'pc' | 'scan' | 'install';
  /** A PC's tabs (1.5); only Live for a PC whose Vyro is older, Screen from 1.7. Devices sit
   * at the top of More (1.7). */
  type Tab = 'live' | 'processes' | 'screen' | 'health' | 'more';
  const TABS: Array<{ id: Tab; label: string; icon: typeof Activity }> = [
    { id: 'live', label: 'Live', icon: Activity },
    { id: 'processes', label: 'Processes', icon: List },
    { id: 'screen', label: 'Screen', icon: Monitor },
    { id: 'health', label: 'Health', icon: HeartPulse },
    { id: 'more', label: 'More', icon: LayoutGrid },
  ];
  /** Low batteries are asked for this often (a dot on More). */
  const BATTERY_MS = 120_000;
  const NEWS_KEY = 'vyro.phone.news';

  /** Chrome's install prompt (Android, desktop). */
  interface InstallPrompt extends Event {
    prompt(): Promise<void>;
  }

  const ua = navigator.userAgent;
  const ios = /iPhone|iPad|iPod/.test(ua) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
  const standalone =
    matchMedia('(display-mode: standalone)').matches || (navigator as Navigator & { standalone?: boolean }).standalone === true;
  const NAME = deviceName(ua);

  const saved = loadPcs();
  /** Paired before this open: a phone pairing now for the first time has no news to hear. */
  const pairedBefore = saved.length > 0;
  let pcs = $state<PairedPc[]>(saved);
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
  let installPrompt = $state<InstallPrompt | null>(null);
  let tab = $state<Tab>('live');
  /** The side the open view came in from. */
  let dir = $state(0);
  let waking = $state<string | null>(null);
  let pcSheet = $state(false);
  /** The PC sheet's code, once first opened. */
  let sheetLoaded = $state(false);
  /** A low battery on the PC: the dot on More, and what it says. */
  let lowBattery = $state<string | null>(null);
  /** The phone's What's new: shown after the notes up to `from` (null: none yet), up to `upTo`. */
  let news = $state<{ from: string | null; upTo: string } | null>(null);
  /** Something in the open tab hides the bars (the PC's screen, full screen). */
  let immersive = $state(false);

  let link: PcLink | null = null;
  let listLinks: PcLink[] = [];

  const pc = $derived(pcs.find((p) => p.room === current));
  /** The PC answers requests (protocol 2), and which this phone may make. */
  const asks = $derived((info?.methods?.length ?? 0) > 0);
  const can = (m: string) => info?.methods?.includes(m) ?? false;
  const ask = <T,>(m: string, p: object = {}) =>
    link ? link.request<T>(m, p) : Promise.reject<T>(new Error('Not connected to the PC.'));
  const others = $derived(sleepers(pcs, current));
  const tabs = $derived(
    TABS.filter((t) => t.id !== 'screen' || info?.screen !== undefined).map((t) => ({
      ...t,
      dot: t.id === 'more' ? lowBattery : null,
    })),
  );
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
  const navOn = $derived(screen === 'pc' && linkState === 'online' && asks && !immersive);

  $effect(() => {
    document.title = screen === 'pc' ? pageTitle(pc?.name ?? '', view) : 'Vyro';
  });

  // Low batteries on the PC: asked now and then while a PC is open, for the dot on More.
  $effect(() => {
    if (screen !== 'pc' || linkState !== 'online' || !info?.methods?.includes('devices')) {
      lowBattery = null;
      return;
    }
    const check = () => {
      if (document.hidden) return;
      ask<{ devices: Array<{ name: string; low?: string | null }> }>('devices')
        .then((d) => {
          const low = d.devices.filter((x) => x.low);
          lowBattery = low.length ? `low battery: ${low.map((x) => x.name).join(', ')}` : null;
        })
        .catch(() => {});
    };
    check();
    const t = setInterval(check, BATTERY_MS);
    return () => clearInterval(t);
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
    pcSheet = false;
    link?.stop();
    link = null;
    immersive = false;
    screen = 'home';
    if (pcs.length) startList();
  }

  function openPc(room: string): void {
    pcSheet = false;
    stopList();
    link?.stop();
    current = room;
    info = null;
    view = null;
    lastAt = 0;
    linkState = 'connecting';
    screen = 'pc';
    tab = 'live';
    dir = 0;
    immersive = false;
    notice = null;
    waking = null;
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

  function select(t: Tab): void {
    if (t === tab) {
      window.scrollTo({ top: 0, behavior: ms(1) ? 'smooth' : 'auto' });
      return;
    }
    const ids = tabs.map((x) => x.id);
    dir = direction(ids.indexOf(tab), ids.indexOf(t));
    tab = t;
    immersive = false;
    window.scrollTo({ top: 0 });
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

  function scan(): void {
    pcSheet = false;
    link?.stop();
    link = null;
    screen = 'scan';
    notice = null;
    stopList();
  }

  async function leave(): Promise<void> {
    const room = current;
    if (!room) return;
    pcSheet = false;
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

  /** The phone's What's new: once after the app changed, for a phone already in use, and only
   * what the open PC's Vyro (`pc`) has too; the rest once the PC is updated. */
  async function checkNews(pc: string): Promise<void> {
    let seen: string | null = null;
    try {
      seen = localStorage.getItem(NEWS_KEY);
    } catch {
      return;
    }
    const { NOTES, phoneNotes } = await import('../whatsnew');
    const newest = NOTES[0]?.version ?? '';
    if (seen === newest) return;
    const { upTo, items } = phoneNotes(seen, pc, NOTES);
    // A phone new to Vyro has nothing to compare with: it starts from here.
    if (!pairedBefore) return seen ? undefined : remember(upTo);
    if (items.length) news = { from: seen, upTo };
    else if (upTo === newest) remember(newest);
  }

  // Asked once the open PC says which Vyro it runs.
  let newsFor = '';
  $effect(() => {
    const pc = info?.version;
    if (!pc || pc === newsFor) return;
    newsFor = pc;
    void checkNews(pc).catch(() => {});
  });

  function remember(version: string): void {
    news = null;
    try {
      localStorage.setItem(NEWS_KEY, version);
    } catch {
      /* shown again next time */
    }
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

  const chip = $derived.by((): { text: string; tone: ChipTone } | null => {
    if (screen !== 'pc') return null;
    if (linkState === 'online') return stale ? { text: 'Waiting for readings…', tone: 'wait' } : { text: 'Live', tone: 'ok' };
    return { text: STATE_TEXT[linkState], tone: linkState === 'connecting' ? 'wait' : 'off' };
  });
  const sub = $derived(
    `End-to-end encrypted${screen === 'pc' && info ? (info.control ? ' · can change this PC' : ' · view only') : ''}`,
  );
  const back = $derived(
    screen === 'scan' || screen === 'install' ? { label: pcs.length ? 'Your PCs' : 'Back', go: () => (pcs.length ? goHome() : (screen = 'home')) } : null,
  );
  /** What decides which view shows, for its entrance. */
  const viewKey = $derived(screen === 'pc' ? `pc:${current}:${navOn || immersive ? tab : linkState}` : screen);
</script>

<div class="app" class:with-nav={navOn} class:immersive>
  {#if !immersive}
    <AppBar
      pc={screen === 'pc' && pc ? pc.name : null}
      {chip}
      {sub}
      onpcs={screen === 'pc' ? () => ((sheetLoaded = true), (pcSheet = true)) : undefined}
      {back}
    />
  {/if}

  <main>
    {#if notice}
      <p class="note warn" role="alert">{notice}</p>
    {/if}

    {#key viewKey}
      <div class="view" style:--dx="{dir * 18}px" style:--dy="{dir ? 0 : 8}px">
        {#if screen === 'install' && held}
          <section class="panel rise" aria-labelledby="install-title">
            <h1 id="install-title">Add Vyro to your Home Screen first</h1>
            <p>On iPhone, an app on the Home Screen keeps its own pairing, apart from Safari’s. So pair it there:</p>
            <ol class="list">
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
            <m.default onlink={(text: string) => fromText(text)} oncancel={() => (pcs.length ? goHome() : (screen = 'home'))} />
          {/await}
        {:else if screen === 'pc' && pc}
          {#if linkState === 'online' && asks && tab !== 'live'}
            {#if tab === 'processes'}
              {#await import('./screens/Processes.svelte') then m}<m.default {ask} {can} pc={pc.name} />{/await}
            {:else if tab === 'health'}
              {#await import('./screens/Health.svelte') then m}<m.default {ask} {can} pc={pc.name} />{/await}
            {:else if tab === 'screen'}
              {#await import('./screens/Screen.svelte') then m}
                <m.default {ask} {can} pc={pc.name} onimmersive={(on: boolean) => (immersive = on)} />
              {/await}
            {:else}
              {#await import('./screens/More.svelte') then m}
                <m.default {ask} {can} pc={pc.name} sleepers={others} low={lowBattery} />
              {/await}
            {/if}
            {#if !info?.control && tab !== 'screen'}
              <p class="muted small">
                This phone may only look. To let it change {pc.name}: Settings → Remote → From anywhere on the PC.
              </p>
            {/if}
          {:else if linkState === 'online' && view}
            <LiveView {view} {unit} pc={pc.name} {now} />
            {#if info}
              <section class="panel link-card" aria-label="This phone and {pc.name}">
                <p class="muted small">{info.device} · paired until {fmtDate(info.expires)}, renewed each time you open it · Vyro {info.version} on the PC</p>
                <button type="button" class="ghost" onclick={() => void leave()}>Sign out of this PC</button>
              </section>
            {/if}
          {:else if linkState === 'online' || linkState === 'connecting'}
            <div class="skeleton" role="status" aria-label={linkState === 'online' ? 'Waiting for the first reading…' : 'Connecting…'}>
              <p class="muted">{linkState === 'online' ? 'Waiting for the first reading…' : 'Connecting…'}</p>
              <span></span><span></span><span></span>
            </div>
          {:else}
            {#await import('./Offline.svelte') then m}
              <m.default
                state={linkState}
                pc={pc.name}
                canWake={!!pc.wake && pcs.length > 1}
                paired={pcs.length}
                {waking}
                onwake={() => void wakeThrough(pc).catch((e: unknown) => (waking = errText(e)))}
                onuse={() => openPc(pc.room)}
                onforget={() => forget(pc.room)}
              />
            {/await}
          {/if}
        {:else}
          {#await import('./Home.svelte') then m}
            <m.default
              {pcs}
              states={listStates}
              words={STATE_TEXT}
              {standalone}
              {ios}
              installable={installPrompt != null}
              onopen={openPc}
              onscan={scan}
              onpaste={fromText}
              oninstall={() => void install()}
            />
          {/await}
        {/if}
        {#if !immersive}<Footer />{/if}
      </div>
    {/key}
  </main>

  {#if navOn && pc}
    <BottomNav label={pc.name} {tabs} current={tab} onselect={select} />
  {/if}
  <Toasts raised={navOn} />

  {#if sheetLoaded}
    {#await import('./PcSheet.svelte') then m}
      <m.default
        open={pcSheet}
        {pcs}
        {current}
        onclose={() => (pcSheet = false)}
        onopen={openPc}
        onall={goHome}
        onpair={scan}
        onleave={() => void leave()}
      />
    {/await}
  {/if}

  {#if news}
    {#await import('./PhoneNews.svelte') then m}
      <m.default from={news.from} upTo={news.upTo} onclose={remember} />
    {/await}
  {/if}
</div>


<style>
  .view {
    min-width: 0;
    animation: enter var(--dur-3) var(--ease-out);
  }
  @keyframes enter {
    from {
      opacity: 0;
      transform: translate(var(--dx, 0), var(--dy, 0));
    }
  }
  .list {
    margin: 0.5rem 0;
    padding-left: 1.25rem;
    line-height: 1.5;
  }
  .link-card {
    display: grid;
    gap: 0.5rem;
    justify-items: start;
  }
  .link-card p {
    margin: 0;
  }
  .immersive main {
    padding: 0;
  }
</style>
