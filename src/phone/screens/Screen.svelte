<script lang="ts">
  // The PC's screen (1.7): seen and used from here. The phone offers a WebRTC connection through
  // the relay, sealed; the PC answers and sends its screen straight to the phone, and touches
  // come back as its mouse and keyboard. Where no direct path exists, the PC sends a picture at
  // a time through the relay instead (the slow view). The phone draws its own pointer.
  import AudioLines from '@lucide/svelte/icons/audio-lines';
  import ClipboardPaste from '@lucide/svelte/icons/clipboard-paste';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Maximize from '@lucide/svelte/icons/maximize';
  import Mic from '@lucide/svelte/icons/mic';
  import Minimize from '@lucide/svelte/icons/minimize';
  import SignalHigh from '@lucide/svelte/icons/signal-high';
  import Square from '@lucide/svelte/icons/square';
  import { RequestError } from '../relay';
  import { b64url, unb64url } from '../seal';
  import Sheet from '../ui/Sheet.svelte';
  import { toast } from '../ui/toast.svelte';
  import { errText, type ScreenProps } from './ask';
  import ClipSheet from './ClipSheet.svelte';
  import { capture, CLIP_SECS, frame, Framer, level, micError, PIECE_DIRECT, PIECE_RELAY, pieces, type Capture } from './voice';
  import { coalesce, contentBox, Gestures, type Input, type Key, type Pt, toFraction, typed } from './gestures';
  import {
    dataText,
    KEY_MOBILE,
    keyFor,
    MB_A_MINUTE,
    onMobileData,
    pasteKeys,
    readQuality,
    startWith,
    type Connection,
    type Quality,
  } from './quality';

  interface Props extends ScreenProps {
    /** The screen fills the phone: the app's bars go (full screen). */
    onimmersive?: (on: boolean) => void;
    /** A session is on, or starting: the app keeps this view while its connection to the relay
     * drops, which a direct session does not need and the slow view rides out. */
    onsharing?: (on: boolean) => void;
  }
  let { ask, can, pc, onimmersive, onsharing }: Props = $props();

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
    /** How the PC takes its pictures: "Desktop Duplication", or "GDI" when that gave blanks. */
    capture: string;
    from: [number, number];
    sent: [number, number];
    taken: number;
    encoded: number;
    keyframes: number;
    bitrate: number;
    /** Bits a second the connection carries, as the PC estimates it (0: not yet; older PCs
     * leave it out). */
    allowed?: number;
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
  /** The slow view goes on through a dropped connection to the PC for this long (the PC ends a
   * slow view nobody asked a picture of for a minute). */
  const SLOW_RETRY_MS = 45_000;
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
  /** HD or SD (1.7), and whether the phone is on mobile data (null: it does not say). */
  let quality = $state<Quality>('hd');
  let mobile = $state<boolean | null>(null);
  /** The question asked before starting on mobile data. */
  let asking = $state(false);
  let rememberMobile = $state(true);
  /** Mobile data came during a session in HD: offer SD. */
  let toMobile = $state(false);
  /** Bytes the picture brought this session. */
  let used = $state(0);
  /** The screen fills the phone (1.7), its controls showing or not, the keys open or not. */
  let full = $state(false);
  let bar = $state(true);
  let keysOpen = $state(false);
  let stage = $state<HTMLDivElement | null>(null);
  /** The voice clip's sheet (1.7). */
  let clipOpen = $state(false);
  /** The phone as the PC's microphone (1.7): sending, how loud, what the PC said of it, and
   * whether the PC has a cable for it. */
  let micOn = $state(false);
  let micLevel = $state(0);
  let micAt = $state<{ into: string | null; default: boolean } | null>(null);
  let audio = $state.raw<{ cable: string | null; switchDefault: boolean } | null>(null);
  let micHelp = $state(false);
  let micCapture: Capture | null = null;
  let micChannel: RTCDataChannel | null = null;

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
        bytesReceived?: number;
      } | null = null;
      (await p.getStats().catch(() => null))?.forEach((r) => {
        if (r.type === 'inbound-rtp' && r.kind === 'video') v = r;
      });
      if (!v) return;
      const { framesDecoded = 0, frameWidth = 0, frameHeight = 0, framesPerSecond = 0, keyFramesDecoded = 0, packetsLost = 0, bytesReceived = 0 } = v;
      used = bytesReceived;
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

  $effect(() => {
    onsharing?.(showing);
    return () => onsharing?.(false);
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
          // The connection dropped (the status says so): what never left goes once it is back;
          // what went may have been done, so it does not go again.
          if (!(e instanceof RequestError && e.link)) error = errText(e);
          else if (e.link === 'unsent') queue = [...batch, ...queue];
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
    stopMic();
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
    used = 0;
    toMobile = false;
    // Asked again each session: a cable installed meanwhile counts.
    audio = null;
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
      const { answer } = await ask<{ answer: string }>('screen.start', { offer, monitor, quality });
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
    let n: { t?: string; w?: number; h?: number; why?: string | null; on?: boolean; into?: string | null; default?: boolean } & Partial<Report>;
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
    else if (n.t === 'mic') {
      if (n.on) micAt = { into: n.into ?? null, default: !!n.default };
      else {
        micAt = null;
        // Off with a reason: it could not start. Off without one: it went quiet, and the next
        // frames start it again.
        if (n.why) {
          stopMic();
          error = n.why;
        }
      }
    }
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
    reconnecting = false;
    let answered = performance.now();
    while (my === run && mode === 'slow') {
      if (document.hidden) {
        await sleep(SLOW_MS);
        continue;
      }
      const t0 = performance.now();
      try {
        const r = await ask<Slow>('screen.slow', { monitor, quality });
        if (my !== run) return;
        answered = performance.now();
        reconnecting = false;
        slowLeft = r.left;
        secure = !!r.secure;
        const bytes = r.jpeg ? unb64url(r.jpeg) : null;
        if (bytes) used += bytes.length;
        if (bytes && r.w && r.h) {
          const url = URL.createObjectURL(new Blob([bytes], { type: 'image/jpeg' }));
          if (slowUrl) URL.revokeObjectURL(slowUrl);
          slowUrl = url;
          size = { w: r.w, h: r.h };
        }
      } catch (e) {
        if (my !== run) return;
        // The connection, not the PC, failed it (patchy mobile data): asked again, for a while.
        if (!(e instanceof RequestError && e.link) || performance.now() - answered > SLOW_RETRY_MS) {
          finish(errText(e));
          return;
        }
        reconnecting = true;
      }
      await sleep(SLOW_MS - (performance.now() - t0));
    }
  }

  function finish(reason: string): void {
    run++;
    closePeer();
    mode = 'ended';
    why = reason;
    leaveFull();
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

  const storage = {
    get(k: string): string | null {
      try {
        return localStorage.getItem(k);
      } catch {
        return null;
      }
    },
    set(k: string, v: string): void {
      try {
        localStorage.setItem(k, v);
      } catch {
        /* not remembered */
      }
    },
  };
  const connection = () => (navigator as Navigator & { connection?: Connection & EventTarget }).connection;

  /** The quality chosen before for this PC, or for mobile data on it. */
  const chosen = (m: boolean | null) => startWith(m, readQuality(storage.get(KEY_MOBILE)), readQuality(storage.get(keyFor(pc)))).quality;

  // Mobile data, as the phone says it now and when it changes. Between sessions the quality
  // follows the choice made for the connection; a session keeps its own until the phone switches
  // it, and in HD a change to mobile data offers SD. (`mobile` is not read here: a change of it
  // would run this again and take the session's quality away.)
  $effect(() => {
    const c = connection();
    const m = onMobileData(c);
    mobile = m;
    quality = chosen(m);
    if (!c?.addEventListener) return;
    const changed = () => {
      const was = mobile;
      mobile = onMobileData(c);
      if (!showing) quality = chosen(mobile);
      else if (mobile && !was && quality === 'hd') toMobile = true;
    };
    c.addEventListener('change', changed);
    return () => c.removeEventListener('change', changed);
  });

  /** Show it, as chosen on the card: on mobile data with no choice made for it yet, ask first. */
  function show(): void {
    if (startWith(mobile, readQuality(storage.get(KEY_MOBILE)), null).ask) {
      asking = true;
      return;
    }
    storage.set(keyFor(pc), quality);
    if (mobile) storage.set(KEY_MOBILE, quality);
    void start();
  }

  function chooseAndStart(q: Quality): void {
    asking = false;
    quality = q;
    if (rememberMobile) storage.set(KEY_MOBILE, q);
    storage.set(keyFor(pc), q);
    void start();
  }

  /** HD or SD from now on: the PC changes at once (the slow view with its next picture). */
  function setQuality(q: Quality): void {
    quality = q;
    toMobile = false;
    storage.set(keyFor(pc), q);
    if (mobile) storage.set(KEY_MOBILE, q);
    if (mode === 'direct' && channel?.readyState === 'open') channel.send(JSON.stringify({ t: 'quality', q }));
    toast(q === 'sd' ? 'SD: smaller and lighter on data.' : 'HD: as sharp as the connection carries.');
  }

  /** The PC keeps voice clips (1.7). */
  const canClip = $derived(can('screen.clip'));

  /** A clip over the direct connection: its own channel, the file in pieces, the PC's answer.
   * Cancelled (`stop`), the channel closes, and the PC drops what came of it. */
  async function clipDirect(bytes: Uint8Array, progress: (done: number) => void, stop: AbortSignal): Promise<string> {
    const p = peer;
    if (!p) throw new Error(`Not connected to ${pc}.`);
    const ch = p.createDataChannel('clip', { ordered: true });
    const lost = () => new Error(`The connection to ${pc} was lost.`);
    const cancel = () => ch.close();
    stop.addEventListener('abort', cancel);
    try {
      await new Promise<void>((ok, fail) => {
        const t = setTimeout(() => fail(new Error(`${pc} did not answer.`)), 10_000);
        ch.onopen = () => (clearTimeout(t), ok());
        ch.onerror = () => (clearTimeout(t), fail(new Error(`${pc} did not take the clip.`)));
        ch.onclose = () => (clearTimeout(t), fail(lost()));
      });
      const answer = new Promise<string>((ok, fail) => {
        const t = setTimeout(() => fail(new Error(`${pc} did not say it kept the clip.`)), 60_000);
        ch.onclose = () => (clearTimeout(t), fail(lost()));
        ch.onmessage = (e) => {
          clearTimeout(t);
          let m: { t?: string; name?: string; why?: string } = {};
          try {
            m = JSON.parse(String(e.data));
          } catch {
            /* not an answer */
          }
          if (m.t === 'saved') ok(m.name ?? '');
          else fail(new Error(m.why ?? `${pc} could not keep the clip.`));
        };
      });
      // Not awaited when the sending below fails first.
      answer.catch(() => {});
      // Waits while much is queued; once the connection is lost what waits never goes, so it
      // stops then rather than wait forever.
      const ready = async () => {
        while (ch.readyState === 'open' && ch.bufferedAmount > 1 << 20) await sleep(40);
        if (ch.readyState !== 'open') throw lost();
      };
      ch.send(JSON.stringify({ t: 'clip', bytes: bytes.length }));
      const parts = pieces(bytes, PIECE_DIRECT);
      for (let i = 0; i < parts.length; i++) {
        await ready();
        ch.send(parts[i]!.slice());
        progress((i + 1) / parts.length);
      }
      await ready();
      ch.send(JSON.stringify({ t: 'end' }));
      return await answer;
    } finally {
      stop.removeEventListener('abort', cancel);
      ch.close();
    }
  }

  /** A clip through the relay (the slow view): requests of a piece each, until cancelled
   * (`stop`; the PC drops a clip whose pieces stopped once the next comes, or the session ends). */
  async function clipRelay(bytes: Uint8Array, progress: (done: number) => void, stop: AbortSignal): Promise<string> {
    const parts = pieces(bytes, PIECE_RELAY);
    const id = b64url(crypto.getRandomValues(new Uint8Array(8)));
    let name = '';
    for (let i = 0; i < parts.length; i++) {
      if (stop.aborted) throw new Error('Cancelled.');
      const r = await ask<{ saved?: string }>('screen.clip', { id, i, n: parts.length, d: b64url(parts[i]!) });
      progress((i + 1) / parts.length);
      if (r?.saved) name = r.saved;
    }
    return name;
  }

  /** The PC can take the phone's microphone (1.7), on the direct connection only. */
  const canMic = $derived(can('screen.audio') && mode === 'direct');

  async function toggleMic(): Promise<void> {
    if (micOn) {
      stopMic();
      return;
    }
    if (!audio) {
      try {
        audio = await ask<{ cable: string | null; switchDefault: boolean }>('screen.audio');
      } catch (e) {
        error = errText(e);
        return;
      }
    }
    if (!audio.cable) {
      micHelp = true;
      return;
    }
    await startMic();
  }

  /** 20 ms frames on their own channel, never resent: a late frame is no use to a voice. */
  async function startMic(): Promise<void> {
    const p = peer;
    // A second tap while it starts opens no second channel.
    if (!p || micChannel) return;
    error = null;
    const ch = p.createDataChannel('mic', { ordered: false, maxRetransmits: 0 });
    ch.binaryType = 'arraybuffer';
    micChannel = ch;
    const framer = new Framer();
    let seq = 0;
    try {
      micCapture = await capture((pcm) => {
        micLevel = level(pcm);
        if (ch.readyState !== 'open') return;
        for (const f of framer.push(pcm)) {
          // A stalled link drops frames rather than queueing them.
          if (ch.bufferedAmount < 32 * 1024) ch.send(frame(seq, f));
          seq++;
        }
      });
      if (micChannel !== ch) {
        micCapture.stop();
        micCapture = null;
        return;
      }
      micOn = true;
    } catch (e) {
      stopMic();
      error = micError(e);
    }
  }

  function stopMic(): void {
    micCapture?.stop();
    micCapture = null;
    micChannel?.close();
    micChannel = null;
    micOn = false;
    micLevel = 0;
    micAt = null;
  }

  function paste(): void {
    send(pasteKeys());
    release();
  }

  // Full screen (1.7): the screen fills the phone. Where the browser can (Android, iPad), it
  // really goes full screen and turns sideways for a wide screen; on iPhone it fills the app's
  // window instead. The controls hide after a while; the corner button brings them back.
  const canFull = typeof document !== 'undefined' && typeof document.documentElement.requestFullscreen === 'function';
  let barTimer: ReturnType<typeof setTimeout> | undefined;

  function showBar(): void {
    bar = true;
    clearTimeout(barTimer);
    barTimer = setTimeout(() => {
      if (full && !keysOpen) bar = false;
    }, 3000);
  }

  async function enterFull(): Promise<void> {
    full = true;
    onimmersive?.(true);
    showBar();
    if (canFull && stage) {
      try {
        await stage.requestFullscreen({ navigationUI: 'hide' });
        const o = screen.orientation as ScreenOrientation & { lock?: (o: string) => Promise<void> };
        if (size && size.w > size.h && innerHeight > innerWidth) await o.lock?.('landscape').catch(() => {});
      } catch {
        /* the stage still fills the window */
      }
    }
  }

  function leaveFull(): void {
    if (!full) return;
    full = false;
    bar = true;
    keysOpen = false;
    clearTimeout(barTimer);
    onimmersive?.(false);
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    try {
      (screen.orientation as ScreenOrientation & { unlock?: () => void }).unlock?.();
    } catch {
      /* not locked */
    }
  }

  // Back or Esc left the browser's full screen: leave ours too.
  $effect(() => {
    const onChange = () => {
      if (!document.fullscreenElement && full && canFull) leaveFull();
    };
    document.addEventListener('fullscreenchange', onChange);
    return () => {
      document.removeEventListener('fullscreenchange', onChange);
      clearTimeout(barTimer);
    };
  });

  // Leaving the tab leaves full screen.
  $effect(() => () => leaveFull());

  const label = (m: Monitor) => `Screen ${m.index + 1}${m.primary ? ' (main)' : ''}, ${m.width}×${m.height}`;
</script>

{#snippet tools()}
  <div class="tools" role="toolbar" aria-label="Screen controls">
    <button type="button" class="tool" aria-label={full ? 'Exit full screen' : 'Full screen'} onclick={() => (full ? leaveFull() : void enterFull())}>
      {#if full}<Minimize size={20} aria-hidden="true" />{:else}<Maximize size={20} aria-hidden="true" />{/if}
      <span>{full ? 'Exit' : 'Full'}</span>
    </button>
    <button
      type="button"
      class="tool"
      aria-label="Quality: {quality === 'hd' ? 'HD' : 'SD'}. Switch to {quality === 'hd' ? 'SD' : 'HD'}"
      onclick={() => setQuality(quality === 'hd' ? 'sd' : 'hd')}
    >
      <SignalHigh size={20} aria-hidden="true" />
      <span>{quality === 'hd' ? 'HD' : 'SD'}</span>
    </button>
    <button type="button" class="tool" aria-pressed={keysOpen} onclick={() => ((keysOpen = !keysOpen), full && showBar())}>
      <Keyboard size={20} aria-hidden="true" />
      <span>Keys</span>
    </button>
    <button type="button" class="tool" aria-label="Paste (Ctrl+V on {pc})" onclick={paste}>
      <ClipboardPaste size={20} aria-hidden="true" />
      <span>Paste</span>
    </button>
    {#if canClip}
      <button type="button" class="tool" aria-label="Voice clip for {pc}" onclick={() => (clipOpen = true)}>
        <AudioLines size={20} aria-hidden="true" />
        <span>Clip</span>
      </button>
    {/if}
    {#if canMic}
      <button type="button" class="tool mic" aria-label="Microphone for {pc}" aria-pressed={micOn} onclick={() => void toggleMic()}>
        <Mic size={20} aria-hidden="true" />
        <span>Mic</span>
        {#if micOn}<i class="lvl" aria-hidden="true" style:transform="scaleX({Math.max(0.08, micLevel)})"></i>{/if}
      </button>
    {/if}
    <button type="button" class="tool stop" onclick={() => void stop()}>
      <Square size={18} aria-hidden="true" />
      <span>Stop</span>
    </button>
  </div>
{/snippet}

{#snippet keyboard()}
  <div class="keys" role="group" aria-label="Keys">
    {#each MODS as m (m.k)}
      <button type="button" class="ghost key" aria-pressed={held[m.k]} aria-label={m.name} onclick={() => toggle(m.k)}>{m.label}</button>
    {/each}
    {#each KEYS as k (k.k)}
      <button type="button" class="ghost key" aria-label={k.name} onclick={() => press(k.k)}>{k.label}</button>
    {/each}
  </div>
  <p class="muted small hint">Ctrl, Alt, Shift and Win stay held until the next key or typing.</p>
  <form class="type" onsubmit={type}>
    <label class="grow">
      <span class="visually-hidden">Type on {pc}</span>
      <input type="text" bind:value={text} autocomplete="off" autocapitalize="off" spellcheck="false" placeholder="Type on {pc}" />
    </label>
    <button type="submit" disabled={!text}>Type</button>
  </form>
{/snippet}

<section class="panel screen" aria-labelledby="screen-title">
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

    <!-- In full screen these show over the picture instead, where they can be seen. -->
    {#if error && !full}<p class="error" role="alert">{error}</p>{/if}

    {#if mode === 'idle' || mode === 'ended'}
      {#if why}<p class="note" role="status">The session ended: {why}.</p>{/if}
      <div class="quality" role="group" aria-label="Quality">
        <span class="muted small">Quality</span>
        <div class="seg">
          <button type="button" class="ghost" aria-pressed={quality === 'hd'} onclick={() => (quality = 'hd')}>HD</button>
          <button type="button" class="ghost" aria-pressed={quality === 'sd'} onclick={() => (quality = 'sd')}>SD</button>
        </div>
        <span class="muted small">
          {mobile ? 'On mobile data. ' : ''}SD uses at most about {MB_A_MINUTE.sd} MB a minute, HD up to {MB_A_MINUTE.hd}.
        </span>
      </div>
      <p class="muted small">
        {pc} shows a banner while you see its screen, and can stop it at any time. Windows' sign-in and administrator
        prompts can't be seen or used from here.
      </p>
      <div class="actions">
        <button type="button" class="go" onclick={show}>{why ? 'Show it again' : `Show ${pc}'s screen`}</button>
      </div>
    {:else}
      {#if toMobile && !full}
        <p class="note warn" role="status">
          You’re on mobile data now.
          <button type="button" class="ghost inline" onclick={() => setQuality('sd')}>Switch to SD</button>
        </p>
      {/if}
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
      <div class="stage" class:full bind:this={stage}>
        <div
          class="surface"
          bind:this={surface}
          bind:clientWidth={sw}
          bind:clientHeight={sh}
          style:aspect-ratio={full ? null : size ? `${size.w} / ${size.h}` : '16 / 9'}
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
        {#if full}
          <button type="button" class="handle" aria-label={bar ? 'Hide the controls' : 'Show the controls'} onclick={() => (bar ? (bar = false) : showBar())}>
            <Keyboard size={16} aria-hidden="true" />
          </button>
          {#if bar}
            <div class="floating">{@render tools()}</div>
          {/if}
          {#if keysOpen}
            <div class="drawer">{@render keyboard()}</div>
          {/if}
          {#if error || reconnecting || toMobile}
            <div class="over">
              {#if error}
                <p class="note hot" role="alert">{error}</p>
              {:else if reconnecting}
                <p class="note">The connection to {pc} dropped. Reconnecting…</p>
              {:else}
                <p class="note warn" role="status">
                  You’re on mobile data now.
                  <button type="button" class="ghost inline" onclick={() => setQuality('sd')}>Switch to SD</button>
                </p>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
      {#if !full}
        {@render tools()}
        {#if micOn}
          <p class="muted small" role="status">
            {#if micAt}
              {pc} hears you as <b>{micAt.into ?? 'its cable'}</b>{micAt.default
                ? ', its default microphone now.'
                : '. Choose it as the microphone in the app you talk into.'}
            {:else}
              Microphone on: connecting to {pc}'s cable…
            {/if}
          </p>
        {/if}
        {#if keysOpen}<div class="inline-keys">{@render keyboard()}</div>{/if}
        <div class="actions">
          {#if zoom > 1.01}
            <button type="button" class="ghost" onclick={() => ((zoom = 1), (pan = { x: 0, y: 0 }))}>Fit the screen</button>
          {/if}
        </div>
        {#if mode === 'direct' && received}
          <details class="arrives">
            <summary class="muted small">Receiving {received.w}×{received.h}, {received.fps} pictures a second · {dataText(used)} so far.</summary>
            <ul class="muted small">
              <li>
                Here: {received.look ? lookText(received.look) : 'brightness unknown'}; {received.keys} whole
                {received.keys === 1 ? 'picture' : 'pictures'}; {received.lost} packets lost.
              </li>
              {#if report}
                <li>
                  {pc}: its screen {report.from[0]}×{report.from[1]} through {report.capture}, sent as {report.sent[0]}×{report.sent[1]};
                  {report.taken} new and {report.encoded} sent pictures a second, {(report.bitrate / 1e6).toFixed(1)} Mbit/s{report.allowed ? ` of the ${(report.allowed / 1e6).toFixed(1)} the connection carries` : ''};
                  {lookText(report)}; {report.asked} whole pictures asked for.
                </li>
              {/if}
            </ul>
          </details>
          {#if verdict}<p class="note" role="status">{verdict}</p>{/if}
        {:else if mode === 'slow'}
          <p class="muted small">{dataText(used)} so far.</p>
        {/if}
      {/if}
    {/if}
  {/if}
</section>

{#if clipOpen}
  <ClipSheet
    open={clipOpen}
    {pc}
    limit={mode === 'slow' ? CLIP_SECS.relay : CLIP_SECS.direct}
    send={(bytes, progress, stop) => (mode === 'direct' ? clipDirect(bytes, progress, stop) : clipRelay(bytes, progress, stop))}
    onpaste={paste}
    onclose={() => (clipOpen = false)}
  />
{/if}

<Sheet open={micHelp} title="The phone as {pc}'s microphone" onclose={() => (micHelp = false)}>
  <p class="muted">
    For this, {pc} needs a virtual cable: install the free VB-CABLE on it (<b>vb-audio.com/Cable</b>), then restart it.
    Apps on {pc} then hear this phone, WhatsApp's voice notes too.
  </p>
  <p class="muted">Until then, a voice clip needs nothing installed: record it here and paste it into the chat.</p>
  <div class="actions">
    {#if canClip}
      <button type="button" onclick={() => ((micHelp = false), (clipOpen = true))}>Record a clip instead</button>
    {/if}
    <button type="button" class="ghost" onclick={() => (micHelp = false)}>Close</button>
  </div>
</Sheet>

<Sheet open={asking} title="You’re on mobile data" onclose={() => (asking = false)}>
  <p class="muted">Seeing {pc}'s screen uses data while it changes. Which would you like?</p>
  <div class="choices">
    <button type="button" class="choice" onclick={() => chooseAndStart('sd')}>
      <b>SD</b>
      <span>Recommended. Up to about {MB_A_MINUTE.sd} MB a minute.</span>
    </button>
    <button type="button" class="choice ghost" onclick={() => chooseAndStart('hd')}>
      <b>HD</b>
      <span>Sharper. Up to about {MB_A_MINUTE.hd} MB a minute.</span>
    </button>
  </div>
  <label class="remember"><input type="checkbox" bind:checked={rememberMobile} /> Remember on mobile data</label>
</Sheet>

<style>
  .stage {
    position: relative;
    min-width: 0;
  }
  .surface {
    position: relative;
    width: 100%;
    max-height: 70vh;
    margin: 0.5rem 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: #000;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }
  /* Full screen (1.7): the picture fills the phone, letterboxed; the controls float over it. */
  .stage.full {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: #000;
  }
  .stage.full .surface {
    width: 100%;
    height: 100%;
    max-height: none;
    margin: 0;
    border: 0;
    border-radius: 0;
  }
  .handle {
    position: absolute;
    top: calc(env(safe-area-inset-top) + 0.5rem);
    right: calc(env(safe-area-inset-right) + 0.5rem);
    display: grid;
    width: 2.75rem;
    min-height: 2.75rem;
    place-items: center;
    padding: 0;
    border: 1px solid rgb(255 255 255 / 25%);
    border-radius: 50%;
    background: rgb(0 0 0 / 45%);
    color: #fff;
    opacity: 0.75;
  }
  .floating {
    position: absolute;
    top: calc(env(safe-area-inset-top) + 0.5rem);
    right: calc(env(safe-area-inset-right) + 3.75rem);
    left: calc(env(safe-area-inset-left) + 0.5rem);
    animation: drop var(--dur-2) var(--ease-out);
  }
  .floating .tools {
    margin: 0;
    padding: 0.35rem;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: 16px;
    background: rgb(10 13 18 / 72%);
    -webkit-backdrop-filter: blur(12px);
    backdrop-filter: blur(12px);
  }
  /* Over the picture: one row of icons; their words stay for screen readers. */
  .floating .tools {
    grid-template-columns: repeat(auto-fit, minmax(2.75rem, 1fr));
  }
  .floating .tool {
    min-height: 2.75rem;
    color: #fff;
  }
  .floating .tool span {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  /* In full screen, what the page says above the picture otherwise: over it, below the controls. */
  .over {
    position: absolute;
    top: calc(env(safe-area-inset-top) + 4.25rem);
    right: calc(env(safe-area-inset-right) + 0.5rem);
    left: calc(env(safe-area-inset-left) + 0.5rem);
    display: grid;
    justify-items: center;
    pointer-events: none;
  }
  .over .note {
    max-width: min(100%, 30rem);
    margin: 0;
    background: color-mix(in srgb, var(--panel) 94%, transparent);
    pointer-events: auto;
  }
  .drawer {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    max-height: 60%;
    padding: 0.6rem max(0.75rem, env(safe-area-inset-right)) calc(0.6rem + env(safe-area-inset-bottom))
      max(0.75rem, env(safe-area-inset-left));
    overflow-y: auto;
    border-top: 1px solid var(--line);
    border-radius: 18px 18px 0 0;
    background: color-mix(in srgb, var(--panel) 94%, transparent);
    animation: up var(--dur-3) var(--ease-out);
  }
  .tools {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(3.6rem, 1fr));
    gap: 0.35rem;
    margin: 0.25rem 0 0.5rem;
  }
  .tool {
    display: grid;
    justify-items: center;
    gap: 0.15rem;
    min-width: 0;
    min-height: 3.4rem;
    padding: 0.35rem 0.2rem;
    border: 1px solid transparent;
    background: color-mix(in srgb, var(--muted) 10%, transparent);
    color: var(--text);
    font-size: 0.72rem;
    font-weight: 600;
  }
  .tool span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool[aria-pressed='true'] {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }
  .tool.stop {
    color: var(--hot);
  }
  .tool.mic {
    position: relative;
    overflow: hidden;
  }
  /* How loud the phone hears you, along the button's foot. */
  .tool .lvl {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    height: 3px;
    background: var(--accent);
    transform-origin: left;
    transition: transform 80ms linear;
  }
  .floating .tool.stop {
    color: #ff8a8a;
  }
  .quality {
    display: grid;
    gap: 0.35rem;
    margin: 0.25rem 0 0.75rem;
  }
  .seg {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.35rem;
    max-width: 16rem;
  }
  .go {
    width: 100%;
    min-height: 3rem;
  }
  .inline {
    min-height: 2.25rem;
    margin-left: 0.4rem;
    padding: 0.2rem 0.7rem;
  }
  .choices {
    display: grid;
    gap: 0.5rem;
    margin: 0.5rem 0;
  }
  .choice {
    display: grid;
    gap: 0.15rem;
    padding: 0.75rem 0.9rem;
    text-align: left;
  }
  .choice span {
    font-weight: 500;
    opacity: 0.85;
  }
  .remember {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    min-height: 2.75rem;
  }
  .remember input {
    width: 1.25rem;
    height: 1.25rem;
    min-height: 0;
  }
  .hint {
    margin: 0 0 0.25rem;
  }
  @keyframes drop {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
  }
  @keyframes up {
    from {
      transform: translateY(100%);
    }
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
    flex: 1 0 auto;
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
