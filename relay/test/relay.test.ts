// The relay run as it is deployed (workerd, through Wrangler's test harness), with PCs and
// phones played by WebSockets. The frames here are stand-ins: the relay never opens them.

import { createHash, randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { MIN_SEALED } from '../src/rules';

type Msg = string | Uint8Array;

class Sock {
  readonly ws: WebSocket;
  private queue: Msg[] = [];
  private waiters: Array<(m: Msg) => void> = [];
  closed: Promise<{ code: number; reason: string }>;
  opened: Promise<void>;

  constructor(url: string, protocols: string[]) {
    this.ws = new WebSocket(url, protocols);
    this.ws.binaryType = 'arraybuffer';
    this.opened = new Promise((resolve, reject) => {
      this.ws.addEventListener('open', () => resolve());
      this.ws.addEventListener('error', () => reject(new Error('socket failed')));
    });
    this.closed = new Promise((resolve) => this.ws.addEventListener('close', (e) => resolve({ code: e.code, reason: e.reason })));
    this.ws.addEventListener('message', (e) => {
      const m: Msg = typeof e.data === 'string' ? e.data : new Uint8Array(e.data as ArrayBuffer);
      const w = this.waiters.shift();
      if (w) w(m);
      else this.queue.push(m);
    });
  }

  next(ms = 5000): Promise<Msg> {
    const q = this.queue.shift();
    if (q !== undefined) return Promise.resolve(q);
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error('no message')), ms);
      this.waiters.push((m) => {
        clearTimeout(t);
        resolve(m);
      });
    });
  }

  async json(ms?: number): Promise<unknown> {
    const m = await this.next(ms);
    if (typeof m !== 'string') throw new Error('expected text');
    return JSON.parse(m) as unknown;
  }

  /** Nothing arrives for `ms`. */
  async quiet(ms = 300): Promise<boolean> {
    try {
      await this.next(ms);
      return false;
    } catch {
      return true;
    }
  }

  close(): void {
    this.ws.close();
  }
}

const b64url = (b: Uint8Array) => Buffer.from(b).toString('base64url');
const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
const room = () => randomBytes(16).toString('hex');
const key = () => new Uint8Array(randomBytes(32));
/** A stand-in for a sealed frame (version, nonce, ciphertext, tag). */
const sealed = (fill: number, n = MIN_SEALED + 8) => new Uint8Array(n).fill(fill);

let server: TestHarness;
let base: string;

beforeAll(async () => {
  server = createTestHarness({ workers: [{ configPath: fileURLToPath(new URL('./wrangler.test.jsonc', import.meta.url)) }] });
  base = (await server.listen()).url.toString().replace(/\/$/, '');
});

afterAll(async () => {
  await server.close();
});

function sock(path: string, k?: Uint8Array): Sock {
  return new Sock(`${base.replace(/^http/, 'ws')}${path}`, k ? ['vyro.1', `k.${b64url(k)}`] : ['vyro.1']);
}

async function pc(r: string, k: Uint8Array): Promise<Sock> {
  const s = sock(`/pc/${r}`, k);
  await s.opened;
  expect(await s.json()).toEqual({ t: 'here', phones: [] });
  return s;
}

async function register(p: Sock, phones: Array<{ id: string; auth: Uint8Array }>): Promise<void> {
  p.ws.send(JSON.stringify({ t: 'phones', list: phones.map((x) => ({ id: x.id, hash: sha(x.auth) })) }));
  expect(await p.json()).toMatchObject({ t: 'here' });
}

describe("the phone app's files", () => {
  it('come with the security headers; sockets have their own paths', async () => {
    const page = await server.fetch('/');
    expect(page.status).toBe(200);
    expect(page.headers.get('content-security-policy')).toContain("default-src 'none'");
    expect(page.headers.get('cache-control')).toBe('no-cache');
    expect(page.headers.get('x-content-type-options')).toBe('nosniff');
    const file = await server.fetch('/assets/app-test.js');
    expect(file.headers.get('cache-control')).toContain('immutable');
    expect((await server.fetch('/', { method: 'POST' })).status).toBe(405);
    expect((await server.fetch('/pc/nope')).status).toBe(404);
    expect((await server.fetch(`/pc/${room()}`)).status).toBe(426);
  });
});

describe('a PC and its room', () => {
  it('the first PC claims the room; another key is refused; a reconnect replaces the old socket', async () => {
    const r = room();
    const k = key();
    const first = await pc(r, k);
    const other = sock(`/pc/${r}`, key());
    expect((await other.closed).code).toBe(4403);
    const again = await pc(r, k);
    expect((await first.closed).code).toBe(4000);
    again.close();
  });

  it('a socket without a key never opens', async () => {
    const s = sock(`/pc/${room()}`);
    await expect(s.opened).rejects.toThrow();
  });

  it('an unknown control message closes the socket', async () => {
    const p = await pc(room(), key());
    p.ws.send('{"t":"open sesame"}');
    expect((await p.closed).code).toBe(1008);
  });
});

describe('phones', () => {
  it('only registered phones get in, and they hear whether the PC is there', async () => {
    const r = room();
    const p = await pc(r, key());
    const phone = { id: '0a1b2c3d', auth: key() };
    const early = sock(`/phone/${r}/${phone.id}`, phone.auth);
    expect((await early.closed).code).toBe(4403);

    await register(p, [phone]);
    const s = sock(`/phone/${r}/${phone.id}`, phone.auth);
    expect(await s.json()).toEqual({ t: 'pc', online: true });
    expect(await p.json()).toEqual({ t: 'here', phones: [phone.id] });

    const wrong = sock(`/phone/${r}/${phone.id}`, key());
    expect((await wrong.closed).code).toBe(4403);

    p.close();
    expect(await s.json()).toEqual({ t: 'pc', online: false });
    s.close();
  });

  it('frames go only to the phone they are for, and back to the PC with the phone named', async () => {
    const r = room();
    const p = await pc(r, key());
    const a = { id: '0000000a', auth: key() };
    const b = { id: '0000000b', auth: key() };
    await register(p, [a, b]);
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    const sb = sock(`/phone/${r}/${b.id}`, b.auth);
    await sa.json();
    await sb.json();
    await p.json();
    await p.json();

    const body = sealed(7);
    p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...body]));
    expect(await sa.next()).toEqual(body);
    expect(await sb.quiet()).toBe(true);

    const up = sealed(9);
    sb.ws.send(up);
    expect(await p.next()).toEqual(new Uint8Array([0, 0, 0, 0x0b, ...up]));

    sa.ws.send('ping');
    expect(await sa.next()).toBe('pong');
    sa.close();
    sb.close();
    p.close();
  });

  it('signing a phone out, or changing its key, closes its socket at once', async () => {
    const r = room();
    const p = await pc(r, key());
    const a = { id: '0000000a', auth: key() };
    const b = { id: '0000000b', auth: key() };
    await register(p, [a, b]);
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    const sb = sock(`/phone/${r}/${b.id}`, b.auth);
    await sa.json();
    await sb.json();
    await p.json();
    await p.json();

    p.ws.send(JSON.stringify({ t: 'phones', list: [{ id: b.id, hash: sha(key()) }] }));
    expect((await sa.closed).code).toBe(4403);
    expect((await sb.closed).code).toBe(4001);
    p.close();
  });

  it('a phone that sends text, tiny frames or too many frames is cut off', async () => {
    const r = room();
    const p = await pc(r, key());
    const phones = ['0000000a', '0000000b', '0000000c'].map((id) => ({ id, auth: key() }));
    await register(p, phones);
    const [s1, s2, s3] = phones.map((x) => sock(`/phone/${r}/${x.id}`, x.auth));
    for (const s of [s1!, s2!, s3!]) await s.json();

    s1!.ws.send('hello');
    expect((await s1!.closed).code).toBe(1003);
    s2!.ws.send(new Uint8Array(MIN_SEALED - 1));
    expect((await s2!.closed).code).toBe(1009);
    for (let i = 0; i < 40; i++) s3!.ws.send(sealed(i));
    expect((await s3!.closed).code).toBe(1008);
    p.close();
  });
});
