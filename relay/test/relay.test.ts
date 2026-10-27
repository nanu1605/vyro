// The relay run as it is deployed (workerd, through Wrangler's test harness), with PCs and
// phones played by WebSockets. The frames here are stand-ins: the relay never opens them.

import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { MAX_FRAME, MIN_SEALED } from '../src/rules';
import { key, relayAt, room, sealed, sha } from './sockets';

let server: TestHarness;
let sock: ReturnType<typeof relayAt>['sock'];
let pc: ReturnType<typeof relayAt>['pc'];
let register: ReturnType<typeof relayAt>['register'];

beforeAll(async () => {
  server = createTestHarness({ workers: [{ configPath: fileURLToPath(new URL('./wrangler.test.jsonc', import.meta.url)) }] });
  ({ sock, pc, register } = relayAt((await server.listen()).url.toString().replace(/\/$/, '')));
});

afterAll(async () => {
  await server.close();
});

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

  it("a PC that used up the day's allowance sends no more, and its phones are told once", async () => {
    // The test relay allows 300 000 bytes a day.
    const r = room();
    const p = await pc(r, key());
    const a = { id: '0000000a', auth: key() };
    const b = { id: '0000000b', auth: key() };
    await register(p, [a, b]);
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    await sa.json();
    await p.json();
    const big = sealed(3, 60_000);
    for (let i = 0; i < 5; i++) {
      p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...big]));
      expect((await sa.next()) as Uint8Array).toHaveLength(60_000);
    }
    p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...big]));
    expect(await sa.json()).toEqual({ t: 'full' });
    p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...sealed(4)]));
    expect(await sa.quiet()).toBe(true);
    // The phone still reaches the PC.
    sa.ws.send(sealed(9));
    expect(await p.next()).toEqual(new Uint8Array([0, 0, 0, 0x0a, ...sealed(9)]));
    // A phone that connects later is told too, the first time something for it is held back.
    const sb = sock(`/phone/${r}/${b.id}`, b.auth);
    expect(await sb.json()).toEqual({ t: 'pc', online: true });
    p.ws.send(new Uint8Array([0, 0, 0, 0x0b, ...sealed(4)]));
    expect(await sb.json()).toEqual({ t: 'full' });
    p.ws.send(new Uint8Array([0, 0, 0, 0x0b, ...sealed(4)]));
    expect(await sb.quiet()).toBe(true);
    expect(await sa.quiet()).toBe(true);
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

describe('limits and comings and goings', () => {
  it('a frame of up to 64 KiB passes either way; a larger one from the PC closes it', async () => {
    const r = room();
    const p = await pc(r, key());
    const a = { id: '0000000a', auth: key() };
    await register(p, [a]);
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    await sa.json();
    await p.json();
    p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...sealed(1, MAX_FRAME)]));
    expect((await sa.next()) as Uint8Array).toHaveLength(MAX_FRAME);
    sa.ws.send(sealed(2, MAX_FRAME));
    expect((await p.next()) as Uint8Array).toHaveLength(4 + MAX_FRAME);
    p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...sealed(3, MAX_FRAME + 1)]));
    expect((await p.closed).code).toBe(1009);
    expect(await sa.json()).toEqual({ t: 'pc', online: false });
    sa.close();
  });

  it('a PC may send a burst of frames at once, and is cut off well past its rate', async () => {
    const r = room();
    const p = await pc(r, key());
    const a = { id: '0000000a', auth: key() };
    await register(p, [a]);
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    await sa.json();
    await p.json();
    for (let i = 0; i < 195; i++) p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...sealed(i)]));
    for (let i = 0; i < 195; i++) expect(await sa.next()).toEqual(sealed(i));
    for (let i = 0; i < 260; i++) p.ws.send(new Uint8Array([0, 0, 0, 0x0a, ...sealed(1)]));
    expect((await p.closed).code).toBe(1008);
    sa.close();
  });

  it("a phone that connects while the PC is away waits quietly, and hears when it is back; a PC's reconnect is not an absence", async () => {
    const r = room();
    const k = key();
    const p = await pc(r, k);
    const a = { id: '0000000a', auth: key() };
    await register(p, [a]);
    p.close();
    await new Promise((done) => setTimeout(done, 200));
    const sa = sock(`/phone/${r}/${a.id}`, a.auth);
    expect(await sa.json()).toEqual({ t: 'pc', online: false });
    sa.ws.send(sealed(1));
    expect(await sa.quiet()).toBe(true);

    const back = sock(`/pc/${r}`, k);
    expect(await back.json()).toEqual({ t: 'here', phones: [a.id], push: true });
    expect(await sa.json()).toEqual({ t: 'pc', online: true });
    sa.ws.send(sealed(2));
    expect(await back.next()).toEqual(new Uint8Array([0, 0, 0, 0x0a, ...sealed(2)]));

    const again = sock(`/pc/${r}`, k);
    expect(await again.json()).toEqual({ t: 'here', phones: [a.id], push: true });
    expect((await back.closed).code).toBe(4000);
    expect(await sa.json()).toEqual({ t: 'pc', online: true });
    expect(await sa.quiet(500)).toBe(true);
    again.close();
    sa.close();
  });
});
