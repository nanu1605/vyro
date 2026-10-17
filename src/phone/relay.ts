// The phone's connection to one PC through the relay (1.4): a WebSocket to the PC's room,
// everything to and from the PC sealed (`seal.ts`). It reconnects by itself, and tells the page
// how things stand: the PC asleep, this phone signed out, the relay out of reach.

import type { RemoteInfo, RemoteView } from '../types';
import { aad, b64url, deriveKeys, open, seal, unb64url, unhex, type Bytes, type Keys } from './seal';
import type { PairedPc } from './pcs';

/** The sealed messages' version this app speaks. */
export const PROTOCOL = 1;

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

export type PhoneInfo = RemoteInfo & { protocol?: number };

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

export class PcLink {
  private ws: WebSocket | null = null;
  private keys: Keys | null = null;
  private aadIn: Bytes = new Uint8Array(0);
  private aadOut: Bytes = new Uint8Array(0);
  /** Connected with the phone's own secret, before the PC confirmed taking it. */
  private onNext = false;
  private stopped = true;
  private wait = WAIT_MIN;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private pinger: ReturnType<typeof setInterval> | null = null;
  private heard = 0;
  private lastTs = 0;
  private current: LinkState = 'connecting';
  /** Messages and closes are handled one at a time, in order (opening a frame waits). */
  private queue: Promise<void> = Promise.resolve();

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
      if (m.t !== 'pc') return;
      this.wait = WAIT_MIN;
      if (m.online) {
        // The PC is there: say hello (it answers with its info).
        const pc = this.pc();
        await this.send({ t: 'hello', name: this.opts.name, ...(pc?.next && !this.onNext ? { next: pc.next } : {}) });
      } else {
        this.set('asleep');
      }
      return;
    }
    if (!this.keys) return;
    const plain = await open(this.keys, this.aadIn, new Uint8Array(data));
    if (!plain) return;
    let m: { t?: string; view?: RemoteView } & PhoneInfo;
    try {
      m = JSON.parse(new TextDecoder().decode(plain)) as typeof m;
    } catch {
      return;
    }
    if (m.t === 'rekeyed') {
      this.on.onRekeyed();
    } else if (m.t === 'info') {
      if ((m.protocol ?? 0) < PROTOCOL) {
        this.set('tooOld');
        return;
      }
      if (this.onNext) {
        // Connected on its own secret: the PC has it.
        this.onNext = false;
        this.on.onRekeyed();
      }
      this.on.onInfo(m);
      this.set('online');
      if (this.opts.watch) await this.send({ t: 'watch' });
    } else if (m.t === 'view' && m.view) {
      if (m.view.ts < this.lastTs) return;
      this.lastTs = m.view.ts;
      this.on.onView(m.view);
    }
  }

  private async send(msg: object): Promise<void> {
    const ws = this.ws;
    if (!ws || ws.readyState !== 1 || !this.keys) return;
    const frame = await seal(this.keys, this.aadOut, new TextEncoder().encode(JSON.stringify(msg)));
    if (ws === this.ws && ws.readyState === 1) ws.send(frame);
  }
}
