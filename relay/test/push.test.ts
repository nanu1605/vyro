// Notifications through the relay (2.0), run as deployed (workerd), with the tests' own push
// service: it checks what a real one checks (the relay's signature, made with the key a phone
// subscribes with) and keeps what it was sent. The bodies are stand-ins: the relay never opens
// them.

import { createServer, type Server } from 'node:http';
import { webcrypto } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { b64url, key, relayAt, room, type Sock } from './sockets';

interface Posted {
  path: string;
  headers: Record<string, string | string[] | undefined>;
  body: Buffer;
}

let server: TestHarness;
let base: string;
let relay: ReturnType<typeof relayAt>;
let push: Server;
let pushBase: string;
const posted: Posted[] = [];
/** What the push service answers next (a phone it no longer knows: 410). */
let answer = 201;

beforeAll(async () => {
  push = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      posted.push({ path: req.url ?? '', headers: req.headers, body: Buffer.concat(chunks) });
      res.writeHead(answer).end();
    });
  });
  await new Promise<void>((done) => push.listen(0, '127.0.0.1', done));
  const addr = push.address();
  pushBase = `http://127.0.0.1:${typeof addr === 'object' && addr ? addr.port : 0}`;
  server = createTestHarness({ workers: [{ configPath: fileURLToPath(new URL('./wrangler.push.test.jsonc', import.meta.url)) }] });
  base = (await server.listen()).url.toString().replace(/\/$/, '');
  relay = relayAt(base);
});

afterAll(async () => {
  await server.close();
  push.close();
});

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const body = (fill: number) => b64url(new Uint8Array(200).fill(fill));

/** Posts to the push service from now on (to paths under `under`), until `n` came or `ms`
 * passed. Each test posts under its own paths: a room another test left may still post. */
async function postsAfter(from: number, n: number, ms: number, under = '/'): Promise<Posted[]> {
  const until = Date.now() + ms;
  const mine = () => posted.slice(from).filter((x) => x.path.startsWith(under));
  while (mine().length < n && Date.now() < until) await sleep(50);
  return mine();
}

async function relayKey(): Promise<string> {
  const res = await server.fetch('/push/key');
  expect(res.status).toBe(200);
  expect(res.headers.get('cache-control')).toBe('no-store');
  return ((await res.json()) as { key: string }).key;
}

/** A PC with one phone registered. */
async function pcWithPhone(): Promise<{ r: string; k: Uint8Array; p: Sock; phone: string }> {
  const r = room();
  const k = key();
  const p = await relay.pc(r, k);
  const phone = '0a1b2c3d';
  await relay.register(p, [{ id: phone, auth: key() }]);
  return { r, k, p, phone };
}

describe("the relay's key", () => {
  it('is one P-256 key for the whole relay, the same every time', async () => {
    const k = await relayKey();
    expect(Buffer.from(k, 'base64url').length).toBe(65);
    expect(Buffer.from(k, 'base64url')[0]).toBe(4);
    expect(await relayKey()).toBe(k);
  });
});

describe('a notification from the PC', () => {
  it("is posted to the phone's push service as it is, signed with the relay's key", async () => {
    const { p, phone } = await pcWithPhone();
    const from = posted.length;
    p.ws.send(JSON.stringify({ t: 'push', to: phone, endpoint: `${pushBase}/sub/1`, body: body(5), ttl: 600, urgency: 'high' }));
    expect(await p.json()).toEqual({ t: 'pushed', to: phone, ok: true, gone: false });
    const [got] = await postsAfter(from, 1, 3000, '/sub/1');
    expect(got!.path).toBe('/sub/1');
    expect(got!.body).toEqual(Buffer.from(new Uint8Array(200).fill(5)));
    expect(got!.headers['content-encoding']).toBe('aes128gcm');
    expect(got!.headers.ttl).toBe('600');
    expect(got!.headers.urgency).toBe('high');
    // VAPID: a token for this push service, signed with the key phones subscribe with.
    const m = /^vapid t=([^,]+), k=([A-Za-z0-9_-]+)$/.exec(String(got!.headers.authorization));
    expect(m).not.toBeNull();
    expect(m![2]).toBe(await relayKey());
    const [h, c, sig] = m![1]!.split('.');
    const claims = JSON.parse(Buffer.from(c!, 'base64url').toString()) as { aud: string; exp: number; sub: string };
    expect(claims.aud).toBe(pushBase);
    expect(claims.sub).toBe(base);
    expect(claims.exp * 1000).toBeGreaterThan(Date.now() + 11 * 3600_000);
    const pub = await webcrypto.subtle.importKey('raw', Buffer.from(m![2]!, 'base64url'), { name: 'ECDSA', namedCurve: 'P-256' }, false, ['verify']);
    expect(await webcrypto.subtle.verify({ name: 'ECDSA', hash: 'SHA-256' }, pub, Buffer.from(sig!, 'base64url'), new TextEncoder().encode(`${h}.${c}`))).toBe(true);
    p.close();
  });

  it('tells the PC when the push service no longer knows the phone', async () => {
    const { p, phone } = await pcWithPhone();
    answer = 410;
    p.ws.send(JSON.stringify({ t: 'push', to: phone, endpoint: `${pushBase}/sub/gone`, body: body(6), ttl: 60, urgency: 'normal' }));
    expect(await p.json()).toEqual({ t: 'pushed', to: phone, ok: false, gone: true });
    answer = 201;
    p.close();
  });

  it('goes nowhere else: not to another address, nor for a phone the PC does not have', async () => {
    const { p, phone } = await pcWithPhone();
    const from = posted.length;
    p.ws.send(JSON.stringify({ t: 'push', to: phone, endpoint: 'https://example.com/steal', body: body(7), ttl: 60, urgency: 'high' }));
    expect(await p.json()).toEqual({ t: 'pushed', to: phone, ok: false, gone: false });
    p.ws.send(JSON.stringify({ t: 'push', to: 'ffffffff', endpoint: `${pushBase}/sub/2`, body: body(7), ttl: 60, urgency: 'high' }));
    expect(await p.json()).toEqual({ t: 'pushed', to: 'ffffffff', ok: false, gone: false });
    expect(await postsAfter(from, 1, 500, '/sub/2')).toEqual([]);
    p.close();
  });

  it('comes a few at once, then slowly', async () => {
    const { p, phone } = await pcWithPhone();
    const results: boolean[] = [];
    for (let i = 0; i < 30; i++) {
      p.ws.send(JSON.stringify({ t: 'push', to: phone, endpoint: `${pushBase}/sub/burst`, body: body(8), ttl: 60, urgency: 'normal' }));
      results.push(((await p.json()) as { ok: boolean }).ok);
    }
    expect(results.filter(Boolean).length).toBe(24);
    p.close();
  });
});

describe("a PC's will", () => {
  const will = (phone: string, path: string) => JSON.stringify({ t: 'will', list: [{ to: phone, endpoint: `${pushBase}${path}`, body: body(9), ttl: 3600 }] });

  it('is sent when the PC goes without saying so and is not back in time; then the PC hears it was', async () => {
    const { r, k, p, phone } = await pcWithPhone();
    p.ws.send(will(phone, '/will/1'));
    await sleep(200);
    const from = posted.length;
    const closed = Date.now();
    p.close();
    // Not before its grace (1.5 s here).
    expect(await postsAfter(from, 1, 800, '/will/1')).toEqual([]);
    const [got] = await postsAfter(from, 1, 4000, '/will/1');
    expect(got!.path).toBe('/will/1');
    expect(got!.headers.urgency).toBe('high');
    // Once.
    expect(await postsAfter(from, 2, 1500, '/will/1')).toHaveLength(1);
    const back = relay.sock(`/pc/${r}`, k);
    const hello = (await back.json()) as { t: string; back?: number };
    expect(hello.t).toBe('here');
    // Since when it was quiet: when its connection ended, not when the phones were told.
    expect(hello.back).toBeGreaterThanOrEqual(closed - 100);
    expect(hello.back).toBeLessThan(closed + 1000);
    // Its will stays until it leaves another: going quiet again would tell the phones again.
    back.ws.send('{"t":"leaving"}');
    await sleep(200);
    back.close();
  });

  it('is not sent when the PC said it was leaving, nor when it is back within its grace', async () => {
    const { r, k, p, phone } = await pcWithPhone();
    p.ws.send(will(phone, '/will/2'));
    p.ws.send('{"t":"leaving"}');
    await sleep(200);
    const from = posted.length;
    p.close();
    expect(await postsAfter(from, 1, 3000, '/will/2')).toEqual([]);

    const again = relay.sock(`/pc/${r}`, k);
    await again.json();
    again.ws.send(will(phone, '/will/3'));
    await sleep(200);
    again.close();
    await sleep(500);
    const soon = relay.sock(`/pc/${r}`, k);
    expect(await soon.json()).toEqual({ t: 'here', phones: [], push: true });
    expect(await postsAfter(from, 1, 3000, '/will/3')).toEqual([]);
    soon.ws.send('{"t":"leaving"}');
    await sleep(200);
    soon.close();
  });

  it("is sent when the PC's pings stop, though its connection never closed", async () => {
    const { r, k, p, phone } = await pcWithPhone();
    p.ws.send(will(phone, '/will/4'));
    const from = posted.length;
    // Pinging, it stays.
    let pinged = 0;
    for (let i = 0; i < 8; i++) {
      p.ws.send('ping');
      expect(await p.next()).toBe('pong');
      pinged = Date.now();
      await sleep(500);
    }
    expect(await postsAfter(from, 1, 10, '/will/4')).toEqual([]);
    // Silent: the relay lets it go (2.5 s, looking every 1 s), then its grace (1.5 s).
    expect((await p.closed).code).toBe(4002);
    const [got] = await postsAfter(from, 1, 4000, '/will/4');
    expect(got!.path).toBe('/will/4');
    // Back: quiet since its last ping, not since the relay noticed.
    const back = relay.sock(`/pc/${r}`, k);
    const hello = (await back.json()) as { back?: number };
    expect(Math.abs(hello.back! - pinged)).toBeLessThan(500);
    back.ws.send('{"t":"leaving"}');
    await sleep(200);
    back.close();
  });
});
