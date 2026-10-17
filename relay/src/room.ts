// One PC's room on the relay (1.4): a Durable Object that holds the PC's socket and its phones'
// sockets, checks their keys, and passes sealed frames between them without reading them.
//
// It uses the WebSocket Hibernation API and sets no timers, so between messages it may sleep
// and costs nothing; `ping` is answered without waking it. It keeps only hashes of keys.

import { DurableObject } from 'cloudflare:workers';
import {
  allow,
  control,
  cutOff,
  expiry,
  frameFits,
  hex,
  keyFrom,
  PC_RATE,
  PHONE_ID_BYTES,
  PHONE_RATE,
  PROTOCOL,
  route,
  same,
  sha256,
  SOCKETS_PER_PHONE,
  type Bucket,
  type Phone,
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
} as const;

type Who = { role: 'pc' } | { role: 'phone'; id: string };

const OPEN = 1;

function pair(): { client: WebSocket; server: WebSocket } {
  const p = new WebSocketPair();
  return { client: p[0], server: p[1] };
}

function upgraded(client: WebSocket): Response {
  return new Response(null, { status: 101, webSocket: client, headers: { 'Sec-WebSocket-Protocol': PROTOCOL } });
}

/** Accepts only to close at once with `code`: a browser cannot read an HTTP refusal, but it can
 * read a close code, so the phone can tell "signed out" from "the network is down". */
function refuse(code: number, reason: string): Response {
  const { client, server } = pair();
  server.accept();
  server.close(code, reason);
  return upgraded(client);
}

function unhex(s: string): Uint8Array {
  return Uint8Array.from({ length: s.length / 2 }, (_, i) => parseInt(s.slice(i * 2, i * 2 + 2), 16));
}

export class Room extends DurableObject {
  private buckets = new WeakMap<WebSocket, Bucket>();

  constructor(ctx: DurableObjectState, env: Cloudflare.Env) {
    super(ctx, env);
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'));
  }

  override async fetch(req: Request): Promise<Response> {
    const r = route(new URL(req.url).pathname);
    const key = keyFrom(req.headers.get('Sec-WebSocket-Protocol'));
    if (!r || !key || req.headers.get('Upgrade')?.toLowerCase() !== 'websocket') {
      return new Response('Bad request', { status: 400 });
    }
    const hash = await sha256(key);
    return r.kind === 'pc' ? this.pc(hash) : this.phone(r.phone, hash);
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

  private async pc(hash: string): Promise<Response> {
    const known = await this.ctx.storage.get<string>('pc');
    if (known !== undefined && !same(known, hash)) return refuse(CLOSE.refused, 'This room belongs to another PC');
    const now = Date.now();
    await this.ctx.storage.put(known === undefined ? { pc: hash, seen: now } : { seen: now });
    await this.ctx.storage.setAlarm(expiry(now));
    // A reconnect (another network, after sleep) takes the old socket's place.
    for (const old of this.open('pc')) old.close(CLOSE.replaced, 'Replaced by a newer connection');
    const { client, server } = pair();
    this.ctx.acceptWebSocket(server, ['pc']);
    server.serializeAttachment({ role: 'pc' } satisfies Who);
    server.send(JSON.stringify({ t: 'here', phones: this.here() }));
    this.toPhones({ t: 'pc', online: true });
    return upgraded(client);
  }

  private async phone(id: string, hash: string): Promise<Response> {
    const phones = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
    const known = phones.find((p) => p.id === id);
    if (!known || !same(known.hash, hash)) return refuse(CLOSE.refused, 'This phone is not paired with this PC');
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
      for (const p of this.open(`phone:${hex(bytes.subarray(0, PHONE_ID_BYTES))}`)) p.send(body);
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
    const before = (await this.ctx.storage.get<Phone[]>('phones')) ?? [];
    await this.ctx.storage.put('phones', c.list);
    const { gone, rekeyed } = cutOff(before, c.list);
    for (const id of gone) for (const p of this.open(`phone:${id}`)) p.close(CLOSE.refused, 'Signed out');
    for (const id of rekeyed) for (const p of this.open(`phone:${id}`)) p.close(CLOSE.rekeyed, 'Key changed');
    ws.send(JSON.stringify({ t: 'here', phones: this.here() }));
  }

  private gone(ws: WebSocket, code: number, reason: string): void {
    try {
      // Completes the closing handshake; 1005 and 1006 cannot be sent back.
      ws.close(code === 1005 || code === 1006 ? 1000 : code, reason);
    } catch {
      /* already closed */
    }
    const who = ws.deserializeAttachment() as Who | null;
    if (who?.role === 'pc') {
      if (!this.open('pc', ws).length) this.toPhones({ t: 'pc', online: false });
    } else if (who?.role === 'phone') {
      this.toPc({ t: 'here', phones: this.here(ws) });
    }
  }

  override async webSocketClose(ws: WebSocket, code: number, reason: string): Promise<void> {
    this.gone(ws, code, reason);
  }

  override async webSocketError(ws: WebSocket): Promise<void> {
    this.gone(ws, 1011, 'Error');
  }

  /** Forgets the room when its PC has been away for 90 days. */
  override async alarm(): Promise<void> {
    if (this.open('pc').length) {
      await this.ctx.storage.setAlarm(expiry(Date.now()));
      return;
    }
    const seen = await this.ctx.storage.get<number>('seen');
    if (seen !== undefined && Date.now() < expiry(seen)) {
      await this.ctx.storage.setAlarm(expiry(seen));
      return;
    }
    for (const ws of this.ctx.getWebSockets()) ws.close(CLOSE.removed, 'Room removed');
    await this.ctx.storage.deleteAll();
  }
}
