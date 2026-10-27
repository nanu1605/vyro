// One PC's room on the relay (1.4): a Durable Object that holds the PC's socket and its phones'
// sockets, checks their keys, and passes sealed frames between them without reading them.
//
// It uses the WebSocket Hibernation API and sets no timers, so between messages it may sleep
// and costs nothing; `ping` is answered without waking it. It keeps only hashes of keys.
//
// Notifications (2.0): the PC hands it notifications it sealed for a phone's browser, and the
// room posts them to that phone's push service, signed with the relay's key (`keys.ts`). The PC
// also leaves its will: notifications to send should it go quiet. When the PC's connection ends
// without its `leaving`, or its `ping`s stop (a room with a will looks every two minutes; the
// pings are answered without waking it, but the runtime notes when), the phones hear unless the
// PC is back within two minutes; and when it is, the PC hears that they did, and since when it
// was quiet.

import { DurableObject } from 'cloudflare:workers';
import { KEYS_NAME, type Keys } from './keys';
import {
  allow,
  CHECK_MS,
  control,
  cutOff,
  DAILY_BYTES,
  DAILY_PUSHES,
  expiry,
  frameFits,
  GRACE_MS,
  hex,
  keyFrom,
  LEAVING_MS,
  PC_RATE,
  PHONE_ID_BYTES,
  PHONE_RATE,
  PROTOCOL,
  PUSH_RATE,
  pushTarget,
  ROOM_TTL_MS,
  route,
  same,
  sha256,
  SILENT_MS,
  SOCKETS_PER_PHONE,
  spend,
  unb64url,
  type Bucket,
  type Phone,
  type Sealed,
  type Usage,
} from './rules';

/** Close codes the PC and the phones act on. */
export const CLOSE = {
  /** A newer socket took this one's place. */
  replaced: 4000,
  /** The phone's key changed: connect again with the new one. */
  rekeyed: 4001,
  /** Wrong key: this phone is not (or no longer) paired, or the room is another PC's. */
  refused: 4403,
  /** The room was forgotten (its PC was away for 90 days). */
  removed: 4410,
  /** The PC's pings stopped: its connection is taken as gone (2.0). */
  silent: 4002,
} as const;

/** A PC's socket: since when, when it said it was leaving, and whether its going was handled. */
type Who = { role: 'pc'; since?: number; leaving?: number; ended?: boolean } | { role: 'phone'; id: string };

/** What the room is given beyond the defaults: the keys, and the tests' shorter times. */
interface RelayEnv {
  KEYS?: DurableObjectNamespace<Keys>;
  DAILY_BYTES?: string;
  ROOM_TTL_MS?: string;
  /** Push services' hosts a test adds (its own, on plain http), comma-separated. */
  PUSH_HOSTS?: string;
  GRACE_MS?: string;
  CHECK_MS?: string;
  SILENT_MS?: string;
}

/** How long a push service has to answer. */
const PUSH_TIMEOUT_MS = 10_000;
const DAY_MS = 24 * 3600 * 1000;

const OPEN = 1;

function pair(): { client: WebSocket; server: WebSocket } {
  const p = new WebSocketPair();
  return { client: p[0], server: p[1] };
}

function upgraded(client: WebSocket): Response {
  return new Response(null, { status: 101, webSocket: client, headers: { 'Sec-WebSocket-Protocol': PROTOCOL } });
}


function unhex(s: string): Uint8Array {
  return Uint8Array.from({ length: s.length / 2 }, (_, i) => parseInt(s.slice(i * 2, i * 2 + 2), 16));
}

export class Room extends DurableObject {
  private buckets = new WeakMap<WebSocket, Bucket>();
  /** What the PC sent its phones today; read from storage once after waking. */
  private usage: Usage | null = null;
  /** The day the count was saved as it ran out. */
  private savedFull = -1;
  /** The day each phone socket was told the day's allowance is used up. */
  private toldFull = new WeakMap<WebSocket, number>();
  private readonly limit: number;
  private readonly ttl: number;
  private readonly keys: DurableObjectNamespace<Keys> | undefined;
  private readonly pushHosts: string[];
  private readonly grace: number;
  private readonly check: number;
  private readonly silent: number;
  /** Notifications the PC may send now (`PUSH_RATE`). */
  private pushBucket: Bucket | null = null;

  constructor(ctx: DurableObjectState, env: Cloudflare.Env) {
    super(ctx, env);
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'));
    // The tests set a small allowance, a short life and short waits.
    const vars = env as unknown as RelayEnv;
    this.limit = Number(vars.DAILY_BYTES) || DAILY_BYTES;
    this.ttl = Number(vars.ROOM_TTL_MS) || ROOM_TTL_MS;
    this.keys = vars.KEYS;
    this.pushHosts = (vars.PUSH_HOSTS ?? '').split(',').map((h) => h.trim()).filter(Boolean);
    this.grace = Number(vars.GRACE_MS) || GRACE_MS;
    this.check = Number(vars.CHECK_MS) || CHECK_MS;
    this.silent = Number(vars.SILENT_MS) || SILENT_MS;
  }

  /** Whether `n` more bytes may go to the phone sockets `to` today; tells each of them once a day
   * when they may not, so a phone that connects later hears it too. */
  private async allowOut(n: number, to: WebSocket[]): Promise<boolean> {
    this.usage ??= (await this.ctx.storage.get<Usage>('usage')) ?? { day: 0, bytes: 0 };
    const r = spend(this.usage, Date.now(), n, this.limit);
    this.usage = r.usage;
    if (r.save || (!r.ok && this.savedFull !== r.usage.day)) await this.ctx.storage.put('usage', r.usage);
    if (r.ok) return true;
    this.savedFull = r.usage.day;
    for (const ws of to) {
      if (this.toldFull.get(ws) === r.usage.day) continue;
      this.toldFull.set(ws, r.usage.day);
      ws.send(JSON.stringify({ t: 'full' }));
    }
    return false;
  }

  /** Accepts only to close at once with `code`: a browser cannot read an HTTP refusal, but it can
   * read a close code, so the phone can tell "signed out" from "the network is down". The socket
   * is the runtime's to close (hibernation), with no role, so nothing else reaches it. */
  private refuse(code: number, reason: string): Response {
    const { client, server } = pair();
    this.ctx.acceptWebSocket(server, ['refused']);
    server.close(code, reason);
    return upgraded(client);
  }

  override async fetch(req: Request): Promise<Response> {
    const r = route(new URL(req.url).pathname);
    const key = keyFrom(req.headers.get('Sec-WebSocket-Protocol'));
    if (!r || !key || req.headers.get('Upgrade')?.toLowerCase() !== 'websocket') {
      return new Response('Bad request', { status: 400 });
    }
    const hash = await sha256(key);
    return r.kind === 'pc' ? this.pc(hash, new URL(req.url).origin) : this.phone(r.phone, hash);
  }

  /** Open sockets with `tag`, leaving out `except` (one that is closing). */
  private open(tag: string, except?: WebSocket): WebSocket[] {
    return this.ctx.getWebSockets(tag).filter((ws) => ws !== except && ws.readyState === OPEN);
  }

  /** Phones connected now. */
  private here(except?: WebSocket): string[] {
    const ids = new Set<string>();
    for (const ws of this.open('phone', except)) {
      const who = ws.deserializeAttachment() as Who | null;
      if (who?.role === 'phone') ids.add(who.id);
    }
    return [...ids].sort();
  }

  private toPc(msg: unknown): void {
    const text = JSON.stringify(msg);
    for (const ws of this.open('pc')) ws.send(text);
  }

  private toPhones(msg: unknown): void {
    const text = JSON.stringify(msg);
    for (const ws of this.open('phone')) ws.send(text);
  }

  private async pc(hash: string, origin: string): Promise<Response> {
    const known = await this.ctx.storage.get<string>('pc');
    if (known !== undefined && !same(known, hash)) return this.refuse(CLOSE.refused, 'This room belongs to another PC');
    const now = Date.now();
    // `origin`: the relay's own address, which it gives push services as who it is.
    await this.ctx.storage.put(known === undefined ? { pc: hash, seen: now, origin } : { seen: now, origin });
    // Back within its grace: the phones are not told it went quiet. Back after they were: the
    // PC is told since when it was quiet, so it can tell them it is back.
    await this.ctx.storage.delete(['willDue', 'quiet']);
    const back = await this.ctx.storage.get<number>('fired');
    if (back !== undefined) await this.ctx.storage.delete('fired');
    // A reconnect (another network, after sleep) takes the old socket's place.
    for (const old of this.open('pc')) old.close(CLOSE.replaced, 'Replaced by a newer connection');
    const { client, server } = pair();
    this.ctx.acceptWebSocket(server, ['pc']);
    server.serializeAttachment({ role: 'pc', since: now } satisfies Who);
    server.send(JSON.stringify({ t: 'here', phones: this.here(), ...(this.keys ? { push: true } : {}), ...(back !== undefined ? { back } : {}) }));
    this.toPhones({ t: 'pc', online: true });
    await this.schedule();
    return upgraded(client);
  }

  private async phone(id: string, hash: string): Promise<Response> {
    const phones = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
    const known = phones.find((p) => p.id === id);
    if (!known || !same(known.hash, hash)) return this.refuse(CLOSE.refused, 'This phone is not paired with this PC');
    const open = this.open(`phone:${id}`);
    for (const old of open.slice(0, Math.max(0, open.length - SOCKETS_PER_PHONE + 1))) {
      old.close(CLOSE.replaced, 'Replaced by a newer connection');
    }
    const { client, server } = pair();
    this.ctx.acceptWebSocket(server, ['phone', `phone:${id}`]);
    server.serializeAttachment({ role: 'phone', id } satisfies Who);
    server.send(JSON.stringify({ t: 'pc', online: this.open('pc').length > 0 }));
    this.toPc({ t: 'here', phones: this.here() });
    return upgraded(client);
  }

  override async webSocketMessage(ws: WebSocket, msg: string | ArrayBuffer): Promise<void> {
    const who = ws.deserializeAttachment() as Who | null;
    if (!who) return ws.close(1008, 'Unknown socket');
    const rate = who.role === 'pc' ? PC_RATE : PHONE_RATE;
    let bucket = this.buckets.get(ws);
    if (!bucket) {
      bucket = { tokens: rate.burst, at: Date.now() };
      this.buckets.set(ws, bucket);
    }
    if (!allow(bucket, rate, Date.now())) return ws.close(1008, 'Too many messages');

    if (who.role === 'pc') {
      if (typeof msg === 'string') return this.fromPc(ws, msg);
      const bytes = new Uint8Array(msg);
      if (!frameFits(bytes.length, PHONE_ID_BYTES)) return ws.close(1009, 'Frame size');
      const body = bytes.slice(PHONE_ID_BYTES);
      const to = this.open(`phone:${hex(bytes.subarray(0, PHONE_ID_BYTES))}`);
      if (!to.length || !(await this.allowOut(body.length * to.length, to))) return;
      for (const p of to) p.send(body);
      return;
    }
    // A phone sends only sealed frames ("ping" is answered before it gets here).
    if (typeof msg === 'string') return ws.close(1003, 'Sealed frames only');
    const bytes = new Uint8Array(msg);
    if (!frameFits(bytes.length, 0)) return ws.close(1009, 'Frame size');
    const out = new Uint8Array(PHONE_ID_BYTES + bytes.length);
    out.set(unhex(who.id), 0);
    out.set(bytes, PHONE_ID_BYTES);
    for (const pc of this.open('pc')) pc.send(out);
  }

  private async fromPc(ws: WebSocket, text: string): Promise<void> {
    const c = control(text);
    if (!c) return ws.close(1008, 'Unknown message');
    if (c.t === 'leaving') {
      const who = ws.deserializeAttachment() as Who;
      ws.serializeAttachment({ ...who, leaving: Date.now() });
      return;
    }
    if (c.t === 'push') {
      const phones = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
      const sent = phones.some((p) => p.id === c.to) && (await this.allowPush()) ? await this.deliver(c, c.urgency) : null;
      try {
        ws.send(JSON.stringify({ t: 'pushed', to: c.to, ok: sent?.ok ?? false, gone: sent?.gone ?? false }));
      } catch {
        // The PC's connection ended while the push service answered.
      }
      return;
    }
    if (c.t === 'will') {
      const phones = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
      const list = c.list.filter((w) => phones.some((p) => p.id === w.to) && pushTarget(w.endpoint, this.pushHosts));
      await this.ctx.storage.put('will', list);
      await this.schedule();
      return;
    }
    const before = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
    await this.ctx.storage.put('phones', c.list);
    const { gone, rekeyed } = cutOff(before, c.list);
    for (const id of gone) for (const p of this.open(`phone:${id}`)) p.close(CLOSE.refused, 'Signed out');
    for (const id of rekeyed) for (const p of this.open(`phone:${id}`)) p.close(CLOSE.rekeyed, 'Key changed');
    // `listed`: the list is in place, so a pairing link the PC shows from now on works.
    ws.send(JSON.stringify({ t: 'here', phones: this.here(), listed: true }));
  }

  private async gone(ws: WebSocket, code: number, reason: string): Promise<void> {
    try {
      // Completes the closing handshake; 1005 and 1006 cannot be sent back.
      ws.close(code === 1005 || code === 1006 ? 1000 : code, reason);
    } catch {
      /* already closed */
    }
    const who = ws.deserializeAttachment() as Who | null;
    if (who?.role === 'pc') {
      if (!who.ended) await this.pcGone(ws, who);
    } else if (who?.role === 'phone') {
      this.toPc({ t: 'here', phones: this.here(ws) });
    }
  }

  /** The PC's socket `ws` is gone: the phones hear it, and unless the PC said it was leaving,
   * its will falls due after its grace. `quiet`: the PC's last sign of life (its last `ping`,
   * when the relay let it go for having stopped them). */
  private async pcGone(ws: WebSocket, who: Extract<Who, { role: 'pc' }>, quiet = Date.now()): Promise<void> {
    const now = Date.now();
    // The room's 90 days count from now, not from when this connection began.
    await this.ctx.storage.put('seen', now);
    if (this.open('pc', ws).length) return;
    this.toPhones({ t: 'pc', online: false });
    if (who.leaving !== undefined && now - who.leaving < LEAVING_MS) return;
    const will = await this.ctx.storage.get<Sealed[]>('will');
    if (!will?.length || (await this.ctx.storage.get<number>('willDue')) !== undefined) return;
    await this.ctx.storage.put({ willDue: now + this.grace, quiet });
    await this.schedule();
  }

  override async webSocketClose(ws: WebSocket, code: number, reason: string): Promise<void> {
    await this.gone(ws, code, reason);
  }

  override async webSocketError(ws: WebSocket): Promise<void> {
    await this.gone(ws, 1011, 'Error');
  }

  /** The room's one alarm, at the soonest of: its being forgotten (90 days after its PC was last
   * connected), its PC's will falling due, and the next look at a PC with a will. */
  private async schedule(): Promise<void> {
    const now = Date.now();
    const connected = this.open('pc').length > 0;
    const seen = (await this.ctx.storage.get<number>('seen')) ?? now;
    const times = [expiry(connected ? now : seen, this.ttl)];
    const due = await this.ctx.storage.get<number>('willDue');
    if (due !== undefined) times.push(due);
    if (connected && (await this.ctx.storage.get<Sealed[]>('will'))?.length) times.push(now + this.check);
    await this.ctx.storage.setAlarm(Math.min(...times));
  }

  /** A PC whose pings stopped is gone, though its socket never said so (it lost power, or its
   * network); its will falls due. Forgets the room when its PC has been away for 90 days. */
  override async alarm(): Promise<void> {
    const now = Date.now();
    if ((await this.ctx.storage.get<Sealed[]>('will'))?.length) {
      for (const ws of this.open('pc')) {
        const who = ws.deserializeAttachment() as Who | null;
        if (who?.role !== 'pc') continue;
        const pinged = this.ctx.getWebSocketAutoResponseTimestamp(ws)?.getTime() ?? 0;
        const alive = Math.max(pinged, who.since ?? 0);
        if (now - alive <= this.silent) continue;
        ws.serializeAttachment({ ...who, ended: true });
        ws.close(CLOSE.silent, 'No ping');
        await this.pcGone(ws, who, alive || now);
      }
    }
    if (this.open('pc').length) {
      await this.ctx.storage.put('seen', now);
      await this.schedule();
      return;
    }
    const due = await this.ctx.storage.get<number>('willDue');
    if (due !== undefined && now >= due) await this.fireWill();
    const seen = await this.ctx.storage.get<number>('seen');
    if (seen !== undefined && now < expiry(seen, this.ttl)) {
      await this.schedule();
      return;
    }
    for (const ws of this.ctx.getWebSockets()) ws.close(CLOSE.removed, 'Room removed');
    await this.ctx.storage.deleteAll();
  }

  /** Tells the phones that wanted it that the PC went quiet: its will, as the PC sealed it. Once
   * back, the PC hears since when it was quiet (`fired`). */
  private async fireWill(): Promise<void> {
    const quiet = (await this.ctx.storage.get<number>('quiet')) ?? Date.now();
    await this.ctx.storage.delete(['willDue', 'quiet']);
    const will = (await this.ctx.storage.get<Sealed[]>('will')) ?? [];
    if (!will.length) return;
    await this.ctx.storage.put('fired', quiet);
    await Promise.all(will.map((w) => this.deliver(w, 'high')));
  }

  /** Whether the PC may send one more notification now: a few at once, then one every few
   * seconds, and `DAILY_PUSHES` a day. */
  private async allowPush(): Promise<boolean> {
    const now = Date.now();
    this.pushBucket ??= { tokens: PUSH_RATE.burst, at: now };
    if (!allow(this.pushBucket, PUSH_RATE, now)) return false;
    const day = Math.floor(now / DAY_MS);
    const count = (await this.ctx.storage.get<{ day: number; n: number }>('pushes')) ?? { day, n: 0 };
    const n = count.day === day ? count.n : 0;
    if (n >= DAILY_PUSHES) return false;
    await this.ctx.storage.put('pushes', { day, n: n + 1 });
    return true;
  }

  /** Posts a sealed notification to its phone's push service, signed with the relay's key.
   * `gone`: the push service no longer knows the phone's subscription. */
  private async deliver(n: Sealed, urgency: string): Promise<{ ok: boolean; gone: boolean }> {
    const target = pushTarget(n.endpoint, this.pushHosts);
    const body = unb64url(n.body);
    const origin = await this.ctx.storage.get<string>('origin');
    if (!target || !body || !origin || !this.keys) return { ok: false, gone: false };
    try {
      const authorization = await this.keys.get(this.keys.idFromName(KEYS_NAME)).authorization(target.origin, origin);
      const res = await fetch(target.toString(), {
        method: 'POST',
        headers: {
          TTL: String(n.ttl),
          Urgency: urgency,
          'Content-Encoding': 'aes128gcm',
          'Content-Type': 'application/octet-stream',
          Authorization: authorization,
        },
        body,
        signal: AbortSignal.timeout(PUSH_TIMEOUT_MS),
      });
      await res.body?.cancel();
      return { ok: res.ok, gone: res.status === 404 || res.status === 410 };
    } catch {
      return { ok: false, gone: false };
    }
  }
}
