// The phone's connection to one PC through the relay (1.4): a WebSocket to the PC's room,
// everything to and from the PC sealed (`seal.ts`). It reconnects by itself, and tells the page
// how things stand: the PC asleep, this phone signed out, the relay out of reach.
//
// Protocol 2 (1.5): the PC's `info` gives this connection a session; every later message
// carries it with a count, so the relay cannot replay one. Requests (`request`) go out as
// `req` and come back as `res`, or a long answer in `part`s.

import type { RemoteInfo, RemoteView } from '../types';
import { aad, b64url, deriveKeys, open, seal, unb64url, unhex, type Bytes, type Keys } from './seal';
import type { PairedPc } from './pcs';

/** The sealed messages' version this app speaks, and the oldest PC it can still show. */
export const PROTOCOL = 2;
export const OLDEST = 1;
/** How long a request waits for its answer. */
export const REQUEST_MS = 30_000;

export type LinkState =
  | 'connecting'
  | 'online'
  /** Connected to the relay; the PC is not (asleep, off, or Vyro not running). */
  | 'asleep'
  /** The relay is out of reach; trying again. */
  | 'unreachable'
  /** The PC signed this phone out. */
  | 'signedOut'
  /** The pairing link expired or was used before the phone could take it. */
  | 'expired'
  /** The same PC is open in another window of this app. */
  | 'elsewhere'
  /** The PC's Vyro is older than this app. */
  | 'tooOld';

export type PhoneInfo = RemoteInfo & {
  protocol?: number;
  /** Protocol 2: this connection's session. */
  session?: string;
  /** This phone may control the PC (the owner allows it on the PC). */
  control?: boolean;
  /** This phone may see and use the PC's screen (1.7; absent: the PC cannot share it). */
  screen?: boolean;
  /** The requests it may make. */
  methods?: string[];
};

/** Said when the relay's daily allowance for this PC is used up (1.7). */
export const FULL = "Today's relay allowance for this PC is used up. It comes back after midnight (UTC).";
const DAY_MS = 24 * 3600 * 1000;

/** A request that failed: the PC refused it, or did not answer. `link`, when the connection to
 * the PC failed it rather than the PC: `unsent`, it never left the phone (asking again does it
 * once); `lost`, it went but its answer did not come (it may have been done). */
export class RequestError extends Error {
  constructor(
    message: string,
    readonly link: 'unsent' | 'lost' | null = null,
  ) {
    super(message);
  }
}
/** Why waiting requests fail when the PC leaves the relay. */
export const GONE = 'The PC went to sleep or off.';

export interface Handlers {
  onState(s: LinkState): void;
  onInfo(info: PhoneInfo): void;
  onView(v: RemoteView): void;
  /** The PC took the phone's own secret: keep it as the one in use. */
  onRekeyed(): void;
}

export interface Options {
  /** This phone's name, as the PC lists it. */
  name: string;
  /** Send readings (the page is showing them). */
  watch: boolean;
  /** The relay, `https://…` (this page's origin). */
  base: string;
  /** For tests. */
  socket?: (url: string, protocols: string[]) => WebSocket;
}

/** Close codes from the relay (`relay/src/room.ts`). */
const CLOSE = { replaced: 4000, rekeyed: 4001, refused: 4403, removed: 4410 } as const;
const PING_MS = 25_000;
const DEAD_MS = 60_000;
const WAIT_MIN = 1000;
const WAIT_MAX = 30_000;
/** The relay closes a phone's socket that sends more than 20 messages at once, or more than 5 a
 * second after that (`PHONE_RATE` in `relay/src/rules.ts`). Messages wait their turn a little
 * under that, so a phone that asks a lot at once (tabs opened in quick succession, a long drag
 * in the slow view) is slowed down, not cut off. */
const PACE = { perSecond: 4, burst: 16 };

export class PcLink {
  private ws: WebSocket | null = null;
  /** The day (UTC) the relay said this PC's allowance was used up. */
  private fullDay = -1;
  private keys: Keys | null = null;
  private aadIn: Bytes = new Uint8Array(0);
  private aadOut: Bytes = new Uint8Array(0);
  /** Connected with the phone's own secret, before the PC confirmed taking it. */
  private onNext = false;
  /** The PC's info gave no session: its Vyro is older than 1.5, and takes no requests. */
  private oldPc = false;
  private stopped = true;
  private wait = WAIT_MIN;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private pinger: ReturnType<typeof setInterval> | null = null;
  private heard = 0;
  private lastTs = 0;
  private current: LinkState = 'connecting';
  /** Messages and closes are handled one at a time, in order (opening a frame waits). */
  private queue: Promise<void> = Promise.resolve();
  /** Protocol 2: this connection's session and the count of messages sent in it. */
  private session: string | null = null;
  private count = 0;
  private nextId = 1;
  private outgoing: Promise<void> = Promise.resolve();
  /** Messages this socket may send now (`PACE`), as of `paceAt`. */
  private allowance = PACE.burst;
  private paceAt = 0;
  private waiting = new Map<number, { ok: (v: unknown) => void; fail: (e: Error) => void; parts: string[] }>();

  constructor(
    private readonly pc: () => PairedPc | undefined,
    private readonly on: Handlers,
    private readonly opts: Options,
  ) {}

  get state(): LinkState {
    return this.current;
  }

  start(): void {
    this.stopped = false;
    void this.connect(false);
  }

  stop(): void {
    this.stopped = true;
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.stopPing();
    const ws = this.ws;
    this.ws = null;
    ws?.close(1000);
  }

  /** Try now (the network came back, the page is shown again), unless connected. */
  wake(): void {
    if (this.stopped || this.ws) return;
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.wait = WAIT_MIN;
    void this.connect(false);
  }

  /** Readings on or off (the page was shown or hidden). */
  setWatch(on: boolean): void {
    if (this.opts.watch === on) return;
    this.opts.watch = on;
    if (this.current === 'online') void this.send({ t: on ? 'watch' : 'unwatch' });
  }

  /** Signs this phone out of the PC, then stops. */
  async signOut(): Promise<void> {
    if (this.ws?.readyState === 1) await this.send({ t: 'bye' });
    this.stop();
  }

  /** Asks the PC (protocol 2). Fails when the PC refuses, is older, or does not answer. */
  request<T = unknown>(method: string, params: object = {}): Promise<T> {
    if (this.fullDay === Math.floor(Date.now() / DAY_MS)) return Promise.reject(new RequestError(FULL));
    if (this.current !== 'online' || !this.session) {
      // Online with no session: an older PC, or one saying hello again.
      return Promise.reject(
        this.current === 'online' && this.oldPc
          ? new RequestError('Update Vyro on the PC.')
          : new RequestError('Not connected to the PC.', 'unsent'),
      );
    }
    const id = this.nextId++;
    return new Promise<T>((ok, fail) => {
      const timer = setTimeout(() => {
        this.waiting.delete(id);
        fail(new RequestError("The PC didn't answer.", 'lost'));
      }, REQUEST_MS);
      this.waiting.set(id, {
        ok: (v) => {
          clearTimeout(timer);
          ok(v as T);
        },
        fail: (e) => {
          clearTimeout(timer);
          fail(e);
        },
        parts: [],
      });
      void this.send({ t: 'req', id, m: method, p: params });
    });
  }

  /** Requests under way fail: the connection they were made on is gone (`link`), or the PC. */
  private dropWaiting(why: string, link: RequestError['link'] = null): void {
    const all = [...this.waiting.values()];
    this.waiting.clear();
    for (const w of all) w.fail(new RequestError(why, link));
  }

  private set(s: LinkState): void {
    if (s === this.current) return;
    this.current = s;
    this.on.onState(s);
  }

  private async connect(useNext: boolean): Promise<void> {
    const pc = this.pc();
    const text = useNext ? pc?.next : pc?.secret;
    const secret = text ? unb64url(text) : null;
    const room = pc ? unhex(pc.room) : null;
    const phone = pc ? unhex(pc.phone) : null;
    if (!pc || !secret || !room || !phone) {
      this.set('signedOut');
      return;
    }
    this.set(this.current === 'online' || this.current === 'asleep' ? this.current : 'connecting');
    const keys = await deriveKeys(secret);
    if (this.stopped) return;
    this.keys = keys;
    this.onNext = useNext;
    this.aadIn = aad(room, phone, 'toPhone');
    this.aadOut = aad(room, phone, 'toPc');
    const url = `${this.opts.base.replace(/^http/, 'ws')}/phone/${pc.room}/${pc.phone}`;
    const protocols = ['vyro.1', `k.${b64url(keys.auth)}`];
    let ws: WebSocket;
    try {
      ws = this.opts.socket ? this.opts.socket(url, protocols) : new WebSocket(url, protocols);
    } catch {
      this.set('unreachable');
      this.later();
      return;
    }
    ws.binaryType = 'arraybuffer';
    this.ws = ws;
    // The relay counts each socket afresh.
    this.allowance = PACE.burst;
    this.paceAt = Date.now();
    ws.onopen = () => {
      this.heard = Date.now();
      this.startPing(ws);
    };
    ws.onmessage = (e: MessageEvent) => {
      const data = e.data as string | ArrayBuffer;
      this.queue = this.queue.then(() => this.message(ws, data)).catch(() => {});
    };
    ws.onclose = (e: CloseEvent) => {
      this.queue = this.queue.then(() => this.closed(ws, e.code));
    };
  }

  private startPing(ws: WebSocket): void {
    this.stopPing();
    this.pinger = setInterval(() => {
      if (ws !== this.ws) return;
      if (Date.now() - this.heard > DEAD_MS) {
        // Asleep in a pocket, or the network changed under it: start over.
        ws.close();
        return;
      }
      ws.send('ping');
    }, PING_MS);
  }

  private stopPing(): void {
    if (this.pinger) clearInterval(this.pinger);
    this.pinger = null;
  }

  private later(): void {
    if (this.stopped) return;
    if (this.retry) clearTimeout(this.retry);
    const wait = this.wait * (0.8 + Math.random() * 0.4);
    this.wait = Math.min(this.wait * 2, WAIT_MAX);
    this.retry = setTimeout(() => {
      this.retry = null;
      void this.connect(false);
    }, wait);
  }

  private closed(ws: WebSocket, code: number): void {
    if (ws !== this.ws) return;
    this.ws = null;
    this.session = null;
    this.dropWaiting('The connection to the PC was lost.', 'lost');
    this.stopPing();
    if (this.stopped) return;
    const pairing = this.pc()?.next !== undefined;
    switch (code) {
      case CLOSE.rekeyed:
        void this.connect(false);
        return;
      case CLOSE.refused:
        // The PC may have taken the phone's own secret without the answer getting here.
        if (!this.onNext && pairing) void this.connect(true);
        else this.set(pairing ? 'expired' : 'signedOut');
        return;
      case CLOSE.removed:
        this.set('signedOut');
        return;
      case CLOSE.replaced:
        this.set('elsewhere');
        return;
      default:
        this.set('unreachable');
        this.later();
    }
  }

  private async message(ws: WebSocket, data: string | ArrayBuffer): Promise<void> {
    if (ws !== this.ws) return;
    this.heard = Date.now();
    if (typeof data === 'string') {
      if (data === 'pong') return;
      let m: { t?: string; online?: boolean };
      try {
        m = JSON.parse(data) as typeof m;
      } catch {
        return;
      }
      if (m.t === 'full') {
        // The PC's answers no longer get through today.
        this.fullDay = Math.floor(Date.now() / DAY_MS);
        this.dropWaiting(FULL);
        return;
      }
      if (m.t !== 'pc') return;
      this.wait = WAIT_MIN;
      if (m.online) {
        // The PC is there: say hello (it answers with its info and, from protocol 2, a session).
        const pc = this.pc();
        this.session = null;
        await this.send({
          t: 'hello',
          name: this.opts.name,
          protocol: PROTOCOL,
          ...(pc?.next && !this.onNext ? { next: pc.next } : {}),
        });
      } else {
        this.session = null;
        this.dropWaiting(GONE);
        this.set('asleep');
      }
      return;
    }
    if (!this.keys) return;
    const plain = await open(this.keys, this.aadIn, new Uint8Array(data));
    if (!plain) return;
    let m: Incoming;
    try {
      m = JSON.parse(new TextDecoder().decode(plain)) as Incoming;
    } catch {
      return;
    }
    if (m.t === 'rekeyed') {
      this.on.onRekeyed();
    } else if (m.t === 'info') {
      if ((m.protocol ?? 0) < OLDEST) {
        this.set('tooOld');
        return;
      }
      if (this.onNext) {
        // Connected on its own secret: the PC has it.
        this.onNext = false;
        this.on.onRekeyed();
      }
      this.oldPc = !m.session;
      // A new session starts its count again; the same one (a permission change) keeps it.
      const fresh = (m.session ?? null) !== this.session;
      this.session = m.session ?? null;
      if (fresh) this.count = 0;
      // Online before the page hears: a request made as it hears (another PC asked to wake one)
      // goes, rather than being refused as not connected.
      const wasOnline = this.current === 'online';
      this.set('online');
      this.on.onInfo(m);
      if (this.opts.watch && (fresh || !wasOnline)) await this.send({ t: 'watch' });
    } else if (m.t === 'res' && m.id != null) {
      this.answered(m.id, m);
    } else if (m.t === 'part' && m.id != null && m.d != null) {
      const w = this.waiting.get(m.id);
      if (!w) return;
      w.parts[m.i ?? 0] = m.d;
      if (w.parts.filter((x) => x != null).length === m.n) {
        try {
          this.answered(m.id, JSON.parse(w.parts.join('')) as Incoming);
        } catch {
          this.waiting.delete(m.id);
          w.fail(new RequestError('The answer did not arrive whole.', 'lost'));
        }
      }
    } else if (m.t === 'view' && m.view) {
      if (m.view.ts < this.lastTs) return;
      this.lastTs = m.view.ts;
      this.on.onView(m.view);
    }
  }

  private answered(id: number, m: Incoming): void {
    const w = this.waiting.get(id);
    if (!w) return;
    this.waiting.delete(id);
    if (m.err != null) w.fail(new RequestError(m.err));
    else w.ok(m.ok);
  }

  /** Messages go out one at a time, in the order of their counts (sealing waits). */
  private send(msg: { t: string } & Record<string, unknown>): Promise<void> {
    const next = this.outgoing.then(() => this.sendNow(msg));
    this.outgoing = next.catch(() => {});
    return next;
  }

  private async sendNow(msg: { t: string } & Record<string, unknown>): Promise<void> {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1 || !this.keys) return;
    if (!(await this.turn(ws))) return;
    // After hello, protocol 2 stamps every message with the session and the next count.
    const stamped = msg.t !== 'hello' && this.session ? { ...msg, s: this.session, n: ++this.count } : msg;
    const frame = await seal(this.keys, this.aadOut, new TextEncoder().encode(JSON.stringify(stamped)));
    if (ws === this.ws && ws.readyState === 1) ws.send(frame);
  }

  /** Waits until `ws` may send one more message (`PACE`) and counts it; false if it closed. */
  private async turn(ws: WebSocket): Promise<boolean> {
    for (;;) {
      if (ws !== this.ws || ws.readyState !== 1) return false;
      const now = Date.now();
      this.allowance = Math.min(PACE.burst, this.allowance + ((now - this.paceAt) / 1000) * PACE.perSecond);
      this.paceAt = now;
      if (this.allowance >= 1) {
        this.allowance -= 1;
        return true;
      }
      await new Promise((r) => setTimeout(r, ((1 - this.allowance) / PACE.perSecond) * 1000));
    }
  }
}

/** A message from the PC, opened. */
type Incoming = { t?: string; view?: RemoteView; id?: number; ok?: unknown; err?: string; i?: number; n?: number; d?: string } & PhoneInfo;
