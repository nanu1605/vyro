<script lang="ts">
  // The phone wheel (2.6; the owner: "can we use the mobile phones gyro behaviour and drive any
  // driving games like Euro truck simulator 2 or Snowrunner, etc using a phone just like it's a
  // Moza wheel? Also there should be no lag."). Held like a wheel, the phone steers; its sides
  // are the pedals. Everything goes straight to the PC on a channel that never waits for a lost
  // packet, as soon as the sensors move; the PC plays it as an Xbox controller.
  import { errText, type ScreenProps } from './ask';
  import { GATHER_MS, gathered, pathOf, STUN } from './peer';
  import { BUTTONS, decode, encode, pedal, since, Steering, vibration, type Button } from './wheel';

  interface Props extends ScreenProps {
    /** The wheel fills the phone: the app's bars go. */
    onimmersive?: (on: boolean) => void;
    /** A session is on: the app keeps this view while its connection to the relay drops. */
    onsharing?: (on: boolean) => void;
    ice?: () => Promise<RTCIceServer[]>;
  }
  let { ask, can, pc, onimmersive, onsharing, ice }: Props = $props();

  type Mode = 'idle' | 'connecting' | 'on' | 'ended';
  /** No direct connection by then: the phone wheel needs one (the relay is far too slow). */
  const DIRECT_MS = 15_000;
  /** A packet at least this often while nothing moves: the PC lets go after 0.4 s without one. */
  const BEAT_MS = 50;
  /** "Still here" on the notes channel. */
  const HERE_MS = 30_000;
  const RANGE_KEY = 'clokrix.phone.wheel.range';

  const PADS: Array<{ id: Button; label: string }> = [
    { id: 'lb', label: 'LB' },
    { id: 'rb', label: 'RB' },
    { id: 'x', label: 'X' },
    { id: 'y', label: 'Y' },
    { id: 'a', label: 'A' },
    { id: 'b', label: 'B' },
    { id: 'back', label: 'View' },
    { id: 'start', label: 'Menu' },
    { id: 'left', label: '◀' },
    { id: 'up', label: '▲' },
    { id: 'down', label: '▼' },
    { id: 'right', label: '▶' },
  ];

  let mode = $state<Mode>('idle');
  let error = $state<string | null>(null);
  let why = $state<string | null>(null);
  let delay = $state<number | null>(null);
  let relayed = $state(false);
  const firstRange = readRange();
  let range = $state(firstRange);
  let stage = $state<HTMLElement | null>(null);
  let rim = $state<SVGGElement | null>(null);
  let throttleBar = $state<HTMLElement | null>(null);
  let brakeBar = $state<HTMLElement | null>(null);

  let peer: RTCPeerConnection | null = null;
  let wheelCh: RTCDataChannel | null = null;
  let notes: RTCDataChannel | null = null;
  let run = 0;
  let seq = 0;
  let wake: WakeLockSentinel | null = null;
  const steering = new Steering(firstRange);
  let steer = 0;
  // Read by the pedals' sliders as well.
  let throttle = $state(0);
  let brake = $state(0);
  let buttons = 0;
  let lastMotion = 0;
  let lastSent = 0;
  let drawn = false;
  /** Touches on the pedals, by the touch's id. */
  const pedalTouches = new Map<number, 'throttle' | 'brake'>();

  const allowed = $derived(can('wheel.start'));

  function readRange(): number {
    try {
      const v = Number(localStorage.getItem(RANGE_KEY));
      return v >= 30 && v <= 270 ? v : 90;
    } catch {
      return 90;
    }
  }

  function setRange(v: number): void {
    range = v;
    steering.range = v;
    try {
      localStorage.setItem(RANGE_KEY, String(v));
    } catch {
      /* kept for this session only */
    }
  }

  /** The controller as it is now, to the PC. */
  function send(neutral = false): void {
    if (wheelCh?.readyState !== 'open') return;
    seq = (seq + 1) & 0xffff;
    const t = Math.round(performance.now()) & 0xffff;
    const pad = neutral ? null : { steer, throttle, brake, buttons };
    try {
      wheelCh.send(encode(seq, t, pad));
      lastSent = performance.now();
    } catch {
      /* the channel is closing */
    }
    if (!drawn) {
      drawn = true;
      requestAnimationFrame(draw);
    }
  }

  /** The wheel and pedals on screen, once a frame at most (not on every sensor reading). */
  function draw(): void {
    drawn = false;
    if (rim) rim.setAttribute('transform', `rotate(${(steer * range).toFixed(1)} 100 100)`);
    if (throttleBar) throttleBar.style.transform = `scaleY(${throttle})`;
    if (brakeBar) brakeBar.style.transform = `scaleY(${brake})`;
  }

  function onMotion(e: DeviceMotionEvent): void {
    const g = e.accelerationIncludingGravity;
    if (!g || g.x === null || g.y === null || g.z === null) return;
    const now = performance.now();
    const dt = lastMotion ? (now - lastMotion) / 1000 : 0;
    lastMotion = now;
    // Turning the phone in its screen plane is turning about its z axis.
    steer = steering.sample({ x: g.x, y: g.y, z: g.z }, e.rotationRate?.alpha ?? null, dt);
    send();
  }

  function pedalFrom(e: PointerEvent, which: 'throttle' | 'brake'): void {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const v = pedal(e.clientY, r.top, r.bottom);
    if (which === 'throttle') throttle = v;
    else brake = v;
    send();
  }

  function pedalDown(e: PointerEvent, which: 'throttle' | 'brake'): void {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pedalTouches.set(e.pointerId, which);
    pedalFrom(e, which);
  }

  function pedalMove(e: PointerEvent): void {
    const which = pedalTouches.get(e.pointerId);
    if (which) pedalFrom(e, which);
  }

  function pedalUp(e: PointerEvent): void {
    const which = pedalTouches.get(e.pointerId);
    pedalTouches.delete(e.pointerId);
    if (which === 'throttle') throttle = 0;
    else if (which === 'brake') brake = 0;
    send();
  }

  function press(id: Button, down: boolean): void {
    buttons = down ? buttons | BUTTONS[id] : buttons & ~BUTTONS[id];
    send();
  }

  /** What the PC sent: the echo (the delay) and the rumble. */
  function fromPc(data: ArrayBuffer): void {
    const m = decode(data);
    if (!m) return;
    if ('echo' in m) delay = since(performance.now(), m.echo.t);
    else navigator.vibrate?.(vibration(m.rumble.large, m.rumble.small));
  }

  function note(raw: string): void {
    try {
      const n = JSON.parse(raw) as { t?: string; ok?: boolean; why?: string | null };
      if (n.t === 'end') finish(n.why ?? 'the PC ended it');
      else if (n.t === 'pad' && !n.ok) error = n.why ?? 'The PC’s virtual controller stopped.';
    } catch {
      /* not a note */
    }
  }

  /** On the tap that starts it (before any await, or the browser no longer counts the tap):
   * motion sensors (iPhone asks), full screen, landscape. */
  function begin(): void {
    error = null;
    why = null;
    const motion = DeviceMotionEvent as unknown as { requestPermission?: () => Promise<'granted' | 'denied'> };
    const asked = motion.requestPermission ? motion.requestPermission() : Promise.resolve('granted' as const);
    if (stage?.requestFullscreen) {
      void stage
        .requestFullscreen({ navigationUI: 'hide' })
        .then(() => (screen.orientation as ScreenOrientation & { lock?: (o: string) => Promise<void> }).lock?.('landscape'))
        .catch(() => {});
    }
    void asked.then(
      (r) => {
        if (r !== 'granted') {
          error = 'The phone’s motion sensors were not allowed: allow them for this page, then try again.';
          leave();
          return;
        }
        void start();
      },
      () => {
        error = 'The phone’s motion sensors could not be used here.';
        leave();
      },
    );
  }

  async function start(): Promise<void> {
    const my = ++run;
    mode = 'connecting';
    onimmersive?.(true);
    onsharing?.(true);
    steering.recentre();
    try {
      wake = await navigator.wakeLock?.request('screen').catch(() => null);
    } catch {
      wake = null;
    }
    try {
      const turn = (await ice?.().catch(() => [])) ?? [];
      if (my !== run) return;
      const p = new RTCPeerConnection({ iceServers: [...STUN, ...turn] });
      peer = p;
      notes = p.createDataChannel('input');
      notes.onmessage = (e) => note(String(e.data));
      const ch = p.createDataChannel('wheel', { ordered: false, maxRetransmits: 0 });
      ch.binaryType = 'arraybuffer';
      ch.onmessage = (e) => fromPc(e.data as ArrayBuffer);
      ch.onopen = () => {
        if (my !== run) return;
        mode = 'on';
        send();
      };
      wheelCh = ch;
      p.onconnectionstatechange = () => {
        if (my !== run) return;
        if (p.connectionState === 'failed' || p.connectionState === 'closed') finish('the connection to the PC was lost');
      };
      await p.setLocalDescription(await p.createOffer());
      await gathered(p, GATHER_MS);
      const offer = p.localDescription?.sdp;
      if (!offer || my !== run) return;
      const { answer } = await ask<{ answer: string }>('wheel.start', { offer });
      if (my !== run) return;
      await p.setRemoteDescription({ type: 'answer', sdp: answer });
      window.addEventListener('devicemotion', onMotion);
      setTimeout(() => {
        if (my === run && mode === 'connecting')
          finish('the phone could not reach the PC directly. Use the same Wi-Fi as the PC, then start again');
      }, DIRECT_MS);
    } catch (e) {
      if (my === run) {
        error = errText(e);
        leave();
      }
    }
  }

  /** The PC let go: everything stops, saying why. */
  function finish(reason: string): void {
    why = reason;
    leave(false);
  }

  /** Stop: everything let go on the PC first, then the session ends. */
  function leave(tellPc = true): void {
    run++;
    window.removeEventListener('devicemotion', onMotion);
    send(true);
    if (tellPc && mode !== 'idle') void ask('wheel.stop').catch(() => {});
    wheelCh = null;
    notes = null;
    if (peer) {
      peer.onconnectionstatechange = null;
      peer.close();
    }
    peer = null;
    void wake?.release().catch(() => {});
    wake = null;
    steer = throttle = brake = buttons = 0;
    pedalTouches.clear();
    delay = null;
    mode = why ? 'ended' : 'idle';
    onimmersive?.(false);
    onsharing?.(false);
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    (screen.orientation as ScreenOrientation & { unlock?: () => void }).unlock?.();
  }

  // While on: a packet at least every 50 ms, "still here", the path's delay, and letting go when
  // the page is hidden (the PC lets go too, after 0.4 s without a packet).
  $effect(() => {
    if (mode !== 'on') return;
    const beat = setInterval(() => {
      if (!document.hidden && performance.now() - lastSent >= BEAT_MS) send();
    }, BEAT_MS);
    const here = setInterval(() => {
      if (notes?.readyState === 'open') notes.send(JSON.stringify({ t: 'here' }));
    }, HERE_MS);
    const path = setInterval(() => {
      void peer?.getStats().then((s) => (relayed = pathOf(s).relayed)).catch(() => {});
    }, 2000);
    const hidden = () => {
      if (document.hidden) {
        throttle = brake = buttons = 0;
        send(true);
      }
    };
    document.addEventListener('visibilitychange', hidden);
    return () => {
      clearInterval(beat);
      clearInterval(here);
      clearInterval(path);
      document.removeEventListener('visibilitychange', hidden);
    };
  });

  // Leaving the view ends the session.
  $effect(() => () => {
    if (mode === 'on' || mode === 'connecting') leave();
  });
</script>

<section class="wheel" class:on={mode === 'on' || mode === 'connecting'} aria-labelledby="wheel-title" bind:this={stage}>
  {#if mode === 'on' || mode === 'connecting'}
    <h1 id="wheel-title" class="sr-only">Steering {pc}</h1>
    <div class="bar">
      <span class="state" role="status">
        {#if mode === 'connecting'}
          Connecting to {pc}…
        {:else if delay !== null}
          Round trip {delay} ms{relayed ? ' · through a relay server' : ''}
        {:else}
          Steering {pc}
        {/if}
      </span>
      <button type="button" onclick={() => steering.recentre()}>Centre</button>
      <button type="button" class="stop" onclick={() => leave()}>Stop</button>
    </div>
    <div class="play">
      <div
        class="pedal brake"
        role="slider"
        aria-label="Brake"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(brake * 100)}
        tabindex="-1"
        onpointerdown={(e) => pedalDown(e, 'brake')}
        onpointermove={pedalMove}
        onpointerup={pedalUp}
        onpointercancel={pedalUp}
      >
        <span class="fill" bind:this={brakeBar}></span>
        <span class="name">Brake</span>
      </div>
      <div class="middle">
        <svg viewBox="0 0 200 200" class="rim" aria-hidden="true">
          <g bind:this={rim}>
            <circle cx="100" cy="100" r="84" />
            <line x1="100" y1="16" x2="100" y2="44" class="mark" />
            <line x1="20" y1="100" x2="180" y2="100" />
          </g>
        </svg>
        <div class="pads">
          {#each PADS as b (b.id)}
            <button
              type="button"
              class="pad"
              onpointerdown={(e) => {
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                press(b.id, true);
              }}
              onpointerup={() => press(b.id, false)}
              onpointercancel={() => press(b.id, false)}>{b.label}</button
            >
          {/each}
        </div>
      </div>
      <div
        class="pedal throttle"
        role="slider"
        aria-label="Throttle"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(throttle * 100)}
        tabindex="-1"
        onpointerdown={(e) => pedalDown(e, 'throttle')}
        onpointermove={pedalMove}
        onpointerup={pedalUp}
        onpointercancel={pedalUp}
      >
        <span class="fill" bind:this={throttleBar}></span>
        <span class="name">Throttle</span>
      </div>
    </div>
  {:else}
    <div class="intro">
      <h1 id="wheel-title">Phone wheel</h1>
      {#if !allowed}
        <p>This phone may not steer games on {pc}. On the PC: Settings → Remote, this phone, <b>Steer games</b>.</p>
      {:else}
        <p>
          Hold the phone like a steering wheel, its screen towards you. Turning it steers; the left edge is the brake and
          the right the throttle, pressed harder the higher your thumb. The PC plays it as an Xbox controller: in the game,
          choose a controller and set its buttons.
        </p>
        <label class="range">
          <span>Full lock at {range}° of turn</span>
          <input type="range" min="30" max="270" step="15" value={range} oninput={(e) => setRange(Number(e.currentTarget.value))} />
        </label>
        <button type="button" class="go" onclick={begin}>Start steering</button>
        <p class="note">On the same Wi-Fi as the PC for the least delay. iPhones cannot vibrate from a page; Android phones vibrate with the game's rumble.</p>
      {/if}
      {#if why}<p class="note" role="status">Ended: {why}.</p>{/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
    </div>
  {/if}
</section>

<style>
  .wheel {
    display: flex;
    flex-direction: column;
    min-height: 60vh;
  }
  .wheel.on {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: var(--bg, #0b0f16);
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.8rem;
  }
  .state {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: 0.85rem;
    color: var(--muted, #9aa4b5);
  }
  .bar button,
  .go {
    padding: 0.5rem 0.9rem;
    border: 1px solid var(--line, #263042);
    border-radius: 10px;
    background: var(--panel, transparent);
    color: var(--text, #e8ecf3);
    font: inherit;
  }
  .bar .stop {
    border-color: #d14343;
  }
  .play {
    display: grid;
    flex: 1;
    grid-template-columns: minmax(0, 22%) minmax(0, 1fr) minmax(0, 22%);
    gap: 0.6rem;
    padding: 0 0.6rem 0.6rem;
  }
  .pedal {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--line, #263042);
    border-radius: 16px;
    touch-action: none;
  }
  .pedal .fill {
    position: absolute;
    inset: 0;
    transform: scaleY(0);
    transform-origin: bottom;
    opacity: 0.55;
  }
  .brake .fill {
    background: #d14343;
  }
  .throttle .fill {
    background: #22a35a;
  }
  .pedal .name {
    position: absolute;
    bottom: 0.6rem;
    left: 0;
    right: 0;
    text-align: center;
    font-weight: 600;
  }
  .middle {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: space-between;
    min-width: 0;
  }
  .rim {
    width: min(42vh, 100%);
    height: auto;
    fill: none;
    stroke: var(--text, #e8ecf3);
    stroke-width: 10;
    stroke-linecap: round;
  }
  .rim .mark {
    stroke: var(--accent, #5aa2ff);
  }
  .pads {
    display: grid;
    grid-template-columns: repeat(6, minmax(0, 1fr));
    gap: 0.4rem;
    width: 100%;
  }
  .pad {
    min-width: 0;
    min-height: 2.8rem;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--line, #263042);
    border-radius: 12px;
    background: var(--panel, transparent);
    color: var(--text, #e8ecf3);
    font: inherit;
    font-weight: 600;
    touch-action: none;
  }
  .pad:active {
    background: var(--accent, #5aa2ff);
    color: var(--on-accent, #0b1220);
  }
  .intro {
    padding: 1rem;
  }
  .intro h1 {
    margin: 0 0 0.5rem;
  }
  .range {
    display: grid;
    gap: 0.3rem;
    margin: 0.8rem 0;
  }
  .go {
    width: 100%;
    margin: 0.4rem 0;
    border-color: var(--accent, #5aa2ff);
    font-weight: 600;
  }
  .note {
    color: var(--muted, #9aa4b5);
    font-size: 0.85rem;
  }
  .err {
    color: #d14343;
  }
</style>
