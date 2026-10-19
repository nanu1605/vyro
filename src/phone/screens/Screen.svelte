<script lang="ts">
  // The PC's screen (1.7): seen and used from here. The phone offers a WebRTC connection through
  // the relay, sealed; the PC answers and sends its screen straight to the phone, and touches
  // come back as its mouse and keyboard. Where no direct path exists, the PC sends a picture at
  // a time through the relay instead (the slow view). The phone draws its own pointer.
  import { unb64url } from '../seal';
  import { errText, type ScreenProps } from './ask';
  import { coalesce, contentBox, Gestures, type Input, type Key, type Pt, toFraction, typed } from './gestures';

  let { ask, can, pc }: ScreenProps = $props();

  interface Monitor {
    index: number;
    name: string;
    width: number;
    height: number;
    primary: boolean;
  }
  interface Slow {
    jpeg?: string;
    w?: number;
    h?: number;
    same?: boolean;
    secure?: boolean;
    left: number;
  }
  /** What the PC sent lately (every 2 s): sizes, rates a second, bits a second, and how bright
   * the picture it took is (luma 16–235) and how much that varies. */
  interface Report {
    from: [number, number];
    sent: [number, number];
    taken: number;
    encoded: number;
    keyframes: number;
    bitrate: number;
    luma: number;
    spread: number;
    asked: number;
  }
  interface Received {
    w: number;
    h: number;
    fps: number;
    keys: number;
    lost: number;
    /** How bright the decoded picture is, on the PC's scale; null when it cannot be read. */
    look: { luma: number; spread: number } | null;
  }
  type Mode = 'idle' | 'connecting' | 'direct' | 'slow' | 'ended';
  type Mod = 'Control' | 'Alt' | 'Shift' | 'Meta';

  const STUN: RTCIceServer[] = [{ urls: 'stun:stun.cloudflare.com:3478' }, { urls: 'stun:stun.l.google.com:19302' }];
  /** No direct connection by then: the slow view. */
  const DIRECT_MS = 15_000;
  /** The longest wait for the phone's own addresses before offering. */
  const GATHER_MS = 3000;
  /** No new picture decoded for this long (a still screen comes again every second): ask the PC
   * for a whole one. */
  const STALL_MS = 3000;
  /** "Still here", while the screen shows (the PC ends a session left 10 minutes). */
  const HERE_MS = 30_000;
  /** The slow view's pace: a picture at most this often, input at most this often. */
  const SLOW_MS = 500;
  const SLOW_INPUT_MS = 300;
  const MAX_ZOOM = 4;

  const KEYS: Array<{ k: Key; label: string; name: string }> = [
    { k: 'Escape', label: 'Esc', name: 'Escape' },
    { k: 'Tab', label: 'Tab', name: 'Tab' },
    { k: 'Enter', label: '⏎', name: 'Enter' },
    { k: 'Backspace', label: '⌫', name: 'Backspace' },
    { k: 'Delete', label: 'Del', name: 'Delete' },
    { k: 'ArrowLeft', label: '←', name: 'Left arrow' },
    { k: 'ArrowUp', label: '↑', name: 'Up arrow' },
    { k: 'ArrowDown', label: '↓', name: 'Down arrow' },
    { k: 'ArrowRight', label: '→', name: 'Right arrow' },
    { k: 'Home', label: 'Home', name: 'Home' },
    { k: 'End', label: 'End', name: 'End' },
    { k: 'PageUp', label: 'PgUp', name: 'Page up' },
    { k: 'PageDown', label: 'PgDn', name: 'Page down' },
  ];
  const MODS: Array<{ k: Mod; label: string; name: string }> = [
    { k: 'Control', label: 'Ctrl', name: 'Control' },
    { k: 'Alt', label: 'Alt', name: 'Alt' },
    { k: 'Shift', label: 'Shift', name: 'Shift' },
    { k: 'Meta', label: 'Win', name: 'Windows key' },
  ];

  let monitors = $state.raw<Monitor[]>([]);
  let monitor = $state(0);
  let mode = $state<Mode>('idle');
  let why = $state<string | null>(null);
  let error = $state<string | null>(null);
  let secure = $state(false);
  /** The picture's size: the shape touches map onto. */
  let size = $state<{ w: number; h: number } | null>(null);
  let slowLeft = $state<number | null>(null);
  /** The direct connection dropped and may come back. */
  let reconnecting = $state(false);
  /** What arrives, as the browser decodes it. */
  let received = $state.raw<Received | null>(null);
  let report = $state.raw<Report | null>(null);
  let slowUrl = $state<string | null>(null);
  let pointer = $state<Pt | null>(null);
  let zoom = $state(1);
  let pan = $state<Pt>({ x: 0, y: 0 });
  let held = $state<Record<Mod, boolean>>({ Control: false, Alt: false, Shift: false, Meta: false });
  let text = $state('');
  let video = $state<HTMLVideoElement | null>(null);
  let surface = $state<HTMLDivElement | null>(null);
  let sw = $state(0);
  let sh = $state(0);

  let peer: RTCPeerConnection | null = null;
  let channel: RTCDataChannel | null = null;
  /** Counts starts: work for an earlier one stops. */
  let run = 0;
  let queue: Input[] = [];
  let sending = false;

  const allowed = $derived(can('screen.start'));
  /** This browser plays the PC's H.264 over WebRTC (phones do; some desktop builds do not). */
  const direct =
    typeof RTCPeerConnection !== 'undefined' &&
    (RTCRtpReceiver.getCapabilities?.('video')?.codecs ?? []).some((c) => c.mimeType.toLowerCase() === 'video/h264');
  const showing = $derived(mode === 'connecting' || mode === 'direct' || mode === 'slow');
  /** Where the picture sits in the surface, before zooming. */
  const box = $derived(size ? contentBox({ left: 0, top: 0, width: sw, height: sh }, size.w, size.h) : null);
  /** A picture that is black: dark and one flat colour. */
  const black = (l: { luma: number; spread: number }) => l.luma < 24 && l.spread < 3;
  /** Where a black picture goes wrong, when it does. */
  const verdict = $derived.by(() => {
    if (mode !== 'direct' || !received?.look || !black(received.look) || received.fps === 0) return null;
    if (report && black(report)) return `The picture ${pc} takes of its screen is black, so the trouble is on the PC.`;
    if (report) return `${pc} sends a picture, but it comes out black on this phone.`;
    return null;
  });
  const percent = (luma: number) => Math.round((Math.min(235, Math.max(16, luma)) - 16) / 2.19);
  const lookText = (l: { luma: number; spread: number }) =>
    `brightness ${percent(l.luma)} %${l.spread < 3 ? ', one flat colour' : ''}`;

  /** A small copy of the video, to read how bright it is (the PC's scale: luma 16–235). */
  let probe: HTMLCanvasElement | null = null;
  function look(v: HTMLVideoElement | null): Received['look'] {
    if (!v?.videoWidth) return null;
    try {
      probe ??= document.createElement('canvas');
      probe.width = 32;
      probe.height = 18;
      const g = probe.getContext('2d', { willReadFrequently: true });
      if (!g) return null;
      g.drawImage(v, 0, 0, 32, 18);
      const px = g.getImageData(0, 0, 32, 18).data;
      const ys: number[] = [];
      for (let i = 0; i + 2 < px.length; i += 4) {
        ys.push(16 + (66 * (px[i] ?? 0) + 129 * (px[i + 1] ?? 0) + 25 * (px[i + 2] ?? 0)) / 256);
      }
      const mean = ys.reduce((a, b) => a + b, 0) / ys.length;
      const spread = Math.sqrt(ys.reduce((a, b) => a + (b - mean) ** 2, 0) / ys.length);
      return { luma: Math.round(mean), spread: Math.round(spread) };
    } catch {
      return null;
    }
  }

  $effect(() => {
    if (!allowed) return;
    ask<Monitor[]>('screen.monitors')
      .then((m) => (monitors = m))
      .catch(() => {});
  });

  // A direct picture: what arrives, and a whole picture asked for when none has for a while (one
  // was lost, or the phone could not decode what came).
  $effect(() => {
    if (mode !== 'direct') return;
    let frames = -1;
    let at = performance.now();
    let asked = 0;
    const t = setInterval(async () => {
      const p = peer;
      if (!p) return;
      let v: {
        framesDecoded?: number;
        frameWidth?: number;
        frameHeight?: number;
        framesPerSecond?: number;
        keyFramesDecoded?: number;
        packetsLost?: number;
      } | null = null;
      (await p.getStats().catch(() => null))?.forEach((r) => {
        if (r.type === 'inbound-rtp' && r.kind === 'video') v = r;
      });
      if (!v) return;
      const { framesDecoded = 0, frameWidth = 0, frameHeight = 0, framesPerSecond = 0, keyFramesDecoded = 0, packetsLost = 0 } = v;
      received = frameWidth
        ? { w: frameWidth, h: frameHeight, fps: Math.round(framesPerSecond), keys: keyFramesDecoded, lost: packetsLost, look: look(video) }
        : null;
      const now = performance.now();
      if (framesDecoded !== frames) {
        frames = framesDecoded;
        at = now;
      } else if (now - at > STALL_MS && now - asked > STALL_MS && channel?.readyState === 'open') {
        asked = now;
        channel.send(JSON.stringify({ t: 'picture' }));
      }
    }, 1000);
    return () => clearInterval(t);
  });

  // While the screen shows: "still here" now and then, and the long-press clock.
  $effect(() => {
    if (!showing) return;
    const here = setInterval(() => {
      if (!document.hidden) send([{ t: 'here' }]);
    }, HERE_MS);
    const tick = setInterval(() => gestures.tick(performance.now()), 100);
    return () => {
      clearInterval(here);
      clearInterval(tick);
    };
  });

  // Leaving the tab ends the session.
  $effect(() => () => {
    if (mode === 'connecting' || mode === 'direct' || mode === 'slow') void ask('screen.stop').catch(() => {});
    closePeer();
    run++;
    if (slowUrl) URL.revokeObjectURL(slowUrl);
  });

  /** Phone pixels to fractions of the PC's monitor. */
  function map(p: Pt): Pt | null {
    if (!surface || !box) return null;
    const r = surface.getBoundingClientRect();
    return toFraction({ x: (p.x - r.left - pan.x) / zoom, y: (p.y - r.top - pan.y) / zoom }, box);
  }

  function clampPan(p: Pt, z: number): Pt {
    return { x: Math.min(0, Math.max(sw * (1 - z), p.x)), y: Math.min(0, Math.max(sh * (1 - z), p.y)) };
  }

  const gestures = new Gestures({
    send: (m) => send(m),
    map,
    zoomed: () => zoom > 1.01,
    zoom: (f, c) => {
      if (!surface) return;
      const r = surface.getBoundingClientRect();
      const z = Math.min(MAX_ZOOM, Math.max(1, zoom * f));
      const ux = (c.x - r.left - pan.x) / zoom;
      const uy = (c.y - r.top - pan.y) / zoom;
      pan = clampPan({ x: c.x - r.left - ux * z, y: c.y - r.top - uy * z }, z);
      zoom = z;
    },
    pan: (dx, dy) => (pan = clampPan({ x: pan.x + dx, y: pan.y + dy }, zoom)),
    pointer: (p) => (pointer = p),
  });

  function send(msgs: Input[]): void {
    if (!msgs.length) return;
    if (mode === 'direct' && channel?.readyState === 'open') {
      for (const m of msgs) channel.send(JSON.stringify(m));
    } else if (mode === 'slow') {
      queue.push(...msgs.filter((m) => m.t !== 'here'));
      void flush(run);
    }
  }

  /** The slow view's input goes in batches, at most every `SLOW_INPUT_MS`. */
  async function flush(my: number): Promise<void> {
    if (sending || !queue.length) return;
    sending = true;
    try {
      while (queue.length && my === run && mode === 'slow') {
        const { batch, rest } = coalesce(queue);
        queue = rest;
        const t0 = performance.now();
        try {
          await ask('screen.input', { inputs: batch });
        } catch (e) {
          error = errText(e);
        }
        await sleep(SLOW_INPUT_MS - (performance.now() - t0));
      }
    } finally {
      sending = false;
    }
  }

  const sleep = (ms: number) => new Promise((r) => setTimeout(r, Math.max(0, ms)));

  /** Waits until the phone knows its own addresses (or `ms`), so one offer carries them. */
  function gathered(p: RTCPeerConnection, ms: number): Promise<void> {
    if (p.iceGatheringState === 'complete') return Promise.resolve();
    return new Promise((done) => {
      const t = setTimeout(done, ms);
      p.addEventListener('icegatheringstatechange', () => {
        if (p.iceGatheringState === 'complete') {
          clearTimeout(t);
          done();
        }
      });
    });
  }

  function closePeer(): void {
    channel = null;
    if (peer) {
      peer.onconnectionstatechange = null;
      peer.close();
    }
    peer = null;
    if (video) video.srcObject = null;
  }

  function reset(): void {
    error = null;
    why = null;
    secure = false;
    reconnecting = false;
    received = null;
    report = null;
    size = null;
    pointer = null;
    zoom = 1;
    pan = { x: 0, y: 0 };
    queue = [];
    held = { Control: false, Alt: false, Shift: false, Meta: false };
  }

  async function start(): Promise<void> {
    const my = ++run;
    reset();
    mode = 'connecting';
    if (!direct) {
      void slow(my);
      return;
    }
    try {
      const p = new RTCPeerConnection({ iceServers: STUN });
      peer = p;
      p.addTransceiver('video', { direction: 'recvonly' });
      const ch = p.createDataChannel('input');
      channel = ch;
      ch.onmessage = (e) => note(String(e.data));
      p.ontrack = (e) => {
        if (!video) return;
        video.srcObject = e.streams[0] ?? new MediaStream([e.track]);
        void video.play().catch(() => {});
      };
      p.onconnectionstatechange = () => {
        if (my !== run) return;
        // A dropped connection may come back (Wi-Fi stalls, the phone changes networks): it says
        // so, and the PC waits a while too.
        reconnecting = p.connectionState === 'disconnected';
        if (p.connectionState === 'connected') mode = 'direct';
        else if (p.connectionState === 'failed' && mode === 'connecting') void slow(my);
        else if ((p.connectionState === 'failed' || p.connectionState === 'closed') && mode === 'direct')
          finish('the connection to the PC was lost');
      };
      await p.setLocalDescription(await p.createOffer());
      await gathered(p, GATHER_MS);
      const offer = p.localDescription?.sdp;
      if (!offer || my !== run) return;
      const { answer } = await ask<{ answer: string }>('screen.start', { offer, monitor });
      if (my !== run) return;
      await p.setRemoteDescription({ type: 'answer', sdp: answer });
      setTimeout(() => {
        if (my === run && mode === 'connecting') void slow(my);
      }, DIRECT_MS);
    } catch {
      // No direct session (the PC cannot encode its screen, or the phone could not offer one):
      // the slow view, which says why if it cannot start either.
      if (my === run) void slow(my);
    }
  }

  /** What the PC says over the data channel. */
  function note(raw: string): void {
    let n: { t?: string; w?: number; h?: number; why?: string } & Partial<Report>;
    try {
      n = JSON.parse(raw);
    } catch {
      return;
    }
    if (n.t === 'sending' && n.w && n.h) {
      secure = false;
      size = { w: n.w, h: n.h };
    } else if (n.t === 'secure') secure = true;
    else if (n.t === 'end') finish(n.why ?? 'the PC ended it');
    else if (n.t === 'report' && n.from && n.sent) report = n as Report;
  }

  function videoSize(): void {
    if (video?.videoWidth && video.videoHeight) size = { w: video.videoWidth, h: video.videoHeight };
  }

  /** No direct path: a picture at a time through the relay. */
  async function slow(my: number): Promise<void> {
    closePeer();
    await ask('screen.stop').catch(() => {});
    if (my !== run) return;
    mode = 'slow';
    secure = false;
    while (my === run && mode === 'slow') {
      if (document.hidden) {
        await sleep(SLOW_MS);
        continue;
      }
      const t0 = performance.now();
      try {
        const r = await ask<Slow>('screen.slow', { monitor });
        if (my !== run) return;
        slowLeft = r.left;
        secure = !!r.secure;
        const bytes = r.jpeg ? unb64url(r.jpeg) : null;
        if (bytes && r.w && r.h) {
          const url = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
          if (slowUrl) URL.revokeObjectURL(slowUrl);
          slowUrl = url;
          size = { w: r.w, h: r.h };
        }
      } catch (e) {
        if (my === run) finish(errText(e));
        return;
      }
      await sleep(SLOW_MS - (performance.now() - t0));
    }
  }

  function finish(reason: string): void {
    run++;
    closePeer();
    mode = 'ended';
    why = reason;
  }

  async function stop(): Promise<void> {
    finish('you stopped it');
    await ask('screen.stop').catch(() => {});
  }

  function choose(index: number): void {
    monitor = index;
    if (showing) {
      void ask('screen.stop')
        .catch(() => {})
        .then(() => start());
    }
  }

  /** A key from the key bar, then the held modifiers go. */
  function press(k: Key): void {
    send([
      { t: 'key', k, down: true },
      { t: 'key', k, down: false },
    ]);
    release();
  }

  function toggle(k: Mod): void {
    const down = !held[k];
    held = { ...held, [k]: down };
    send([{ t: 'key', k, down }]);
  }

  function release(): void {
    const up = MODS.filter((m) => held[m.k]).map((m): Input => ({ t: 'key', k: m.k, down: false }));
    send(up);
    held = { Control: false, Alt: false, Shift: false, Meta: false };
  }

  function type(e: SubmitEvent): void {
    e.preventDefault();
    const modifier = held.Control || held.Alt || held.Meta;
    send(typed(text, modifier));
    text = '';
    if (modifier || held.Shift) release();
  }

  const pt = (e: PointerEvent): Pt => ({ x: e.clientX, y: e.clientY });

  function down(e: PointerEvent): void {
    if (e.button > 0) return;
    surface?.setPointerCapture(e.pointerId);
    gestures.down(e.pointerId, pt(e), performance.now());
  }

  function wheel(e: WheelEvent): void {
    e.preventDefault();
    const notches = Math.round(e.deltaY / 100) || Math.sign(e.deltaY);
    if (notches) send([{ t: 'scroll', dx: 0, dy: notches }]);
  }

  const label = (m: Monitor) => `Screen ${m.index + 1}${m.primary ? ' (main)' : ''}, ${m.width}×${m.height}`;
</script>

<section class="panel" aria-labelledby="screen-title">
  <h1 id="screen-title">{pc}'s screen</h1>
  {#if !allowed}
    <p class="muted">
      This phone may not see {pc}'s screen. To allow it: Settings → Remote → Paired devices on the PC, then “No screen” next
      to this phone.
    </p>
  {:else}
    {#if monitors.length > 1}
      <div class="sorts" role="group" aria-label="Which screen">
        {#each monitors as m (m.index)}
          <button type="button" class="ghost" aria-pressed={monitor === m.index} onclick={() => choose(m.index)}>{label(m)}</button>
        {/each}
      </div>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    {#if mode === 'idle' || mode === 'ended'}
      {#if why}<p class="note" role="status">The session ended: {why}.</p>{/if}
      <p class="muted small">
        {pc} shows a banner while you see its screen, and can stop it at any time. Windows' sign-in and administrator
        prompts can't be seen or used from here.
      </p>
      <div class="actions">
        <button type="button" onclick={() => void start()}>{why ? 'Show it again' : `Show ${pc}'s screen`}</button>
      </div>
    {:else}
      <p class="muted small" role="status">
        {#if mode === 'connecting'}
          Connecting to {pc}…
        {:else if reconnecting}
          The connection to {pc} dropped. Reconnecting…
        {:else if mode === 'slow'}
          No direct path to {pc}: the slow view, a picture at a time.{slowLeft !== null
            ? ` ${Math.round(slowLeft / 60)} min left today.`
            : ''}
        {:else}
          Tap to click, hold for a right click, drag to drag; two fingers scroll, pinch to zoom.
        {/if}
      </p>
      {#if mode === 'direct' && received}
        <details class="arrives">
          <summary class="muted small">Receiving {received.w}×{received.h}, {received.fps} pictures a second.</summary>
          <ul class="muted small">
            <li>
              Here: {received.look ? lookText(received.look) : 'brightness unknown'}; {received.keys} whole
              {received.keys === 1 ? 'picture' : 'pictures'}; {received.lost} packets lost.
            </li>
            {#if report}
              <li>
                {pc}: its screen {report.from[0]}×{report.from[1]}, sent as {report.sent[0]}×{report.sent[1]};
                {report.taken} new and {report.encoded} sent pictures a second, {(report.bitrate / 1e6).toFixed(1)} Mbit/s;
                {lookText(report)}; {report.asked} whole pictures asked for.
              </li>
            {/if}
          </ul>
        </details>
        {#if verdict}<p class="note" role="status">{verdict}</p>{/if}
      {/if}
      <div
        class="surface"
        bind:this={surface}
        bind:clientWidth={sw}
        bind:clientHeight={sh}
        style:aspect-ratio={size ? `${size.w} / ${size.h}` : '16 / 9'}
        role="application"
        aria-label="{pc}'s screen"
        onpointerdown={down}
        onpointermove={(e) => gestures.move(e.pointerId, pt(e), performance.now())}
        onpointerup={(e) => gestures.up(e.pointerId, pt(e), performance.now())}
        onpointercancel={(e) => gestures.cancel(e.pointerId)}
        oncontextmenu={(e) => e.preventDefault()}
        onwheel={wheel}
      >
        <div class="layer" style:transform="translate({pan.x}px, {pan.y}px) scale({zoom})">
          {#if mode === 'slow'}
            {#if slowUrl}<img src={slowUrl} alt="" draggable="false" />{/if}
          {:else}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video bind:this={video} autoplay playsinline muted onresize={videoSize}></video>
          {/if}
          {#if pointer && box}
            <span class="dot" style:left="{box.left + pointer.x * box.width}px" style:top="{box.top + pointer.y * box.height}px"></span>
          {/if}
        </div>
        {#if secure}
          <p class="secure" role="status">{pc} is showing a secure screen (sign-in, lock screen or an administrator prompt). It can't be seen from here.</p>
        {/if}
      </div>

      <div class="keys" role="group" aria-label="Keys">
        {#each MODS as m (m.k)}
          <button type="button" class="ghost key" aria-pressed={held[m.k]} aria-label={m.name} onclick={() => toggle(m.k)}>{m.label}</button>
        {/each}
        {#each KEYS as k (k.k)}
          <button type="button" class="ghost key" aria-label={k.name} onclick={() => press(k.k)}>{k.label}</button>
        {/each}
      </div>
      <p class="muted small">Ctrl, Alt, Shift and Win stay held until the next key or typing.</p>
      <form class="type" onsubmit={type}>
        <label class="grow">
          <span class="visually-hidden">Type on {pc}</span>
          <input type="text" bind:value={text} autocomplete="off" autocapitalize="off" spellcheck="false" placeholder="Type on {pc}" />
        </label>
        <button type="submit" disabled={!text}>Type</button>
      </form>
      <div class="actions">
        {#if zoom > 1.01}
          <button type="button" class="ghost" onclick={() => ((zoom = 1), (pan = { x: 0, y: 0 }))}>Fit the screen</button>
        {/if}
        <button type="button" onclick={() => void stop()}>Stop</button>
      </div>
    {/if}
  {/if}
</section>

<style>
  .surface {
    position: relative;
    width: 100%;
    max-height: 70vh;
    margin: 0.5rem 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: #000;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }
  .layer {
    position: absolute;
    inset: 0;
    transform-origin: 0 0;
  }
  video,
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    pointer-events: none;
  }
  .dot {
    position: absolute;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border: 2px solid #fff;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 0 1px #000;
    pointer-events: none;
  }
  .secure {
    position: absolute;
    inset: auto 0.5rem 0.5rem;
    margin: 0;
    padding: 0.5rem 0.65rem;
    border-radius: 8px;
    background: color-mix(in srgb, var(--panel) 92%, transparent);
    color: var(--text);
    font-size: 0.85rem;
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0.5rem 0 0.25rem;
  }
  .key {
    min-width: 2.75rem;
    min-height: 2.75rem;
    padding: 0.35rem 0.55rem;
  }
  .key[aria-pressed='true'] {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .type {
    display: flex;
    gap: 0.5rem;
    margin: 0.5rem 0;
  }
  .type .grow {
    flex: 1;
  }
  .type input {
    width: 100%;
  }
  .sorts {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0 0 0.5rem;
  }
  .sorts button[aria-pressed='true'] {
    border-color: var(--accent);
  }
  .arrives summary {
    cursor: pointer;
  }
  .arrives ul {
    margin: 0.25rem 0 0;
    padding-left: 1.1rem;
  }
  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
