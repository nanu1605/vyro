import { describe, expect, it } from 'vitest';
import { allow, assetHeaders, control, cutOff, DAILY_BYTES, frameFits, isSocketPath, keyFrom, MAX_FRAME, MAX_PHONES, MIN_SEALED, route, same, spend, USAGE_STEP } from '../src/rules';

const ROOM = '00112233445566778899aabbccddeeff';
const KEY = 'A'.repeat(43); // 32 bytes of base64url
const HASH = 'ab'.repeat(32);

describe('addresses', () => {
  it('knows the PC and phone sockets, lower-case hex only', () => {
    expect(route(`/pc/${ROOM}`)).toEqual({ kind: 'pc', room: ROOM });
    expect(route(`/phone/${ROOM}/0a1b2c3d`)).toEqual({ kind: 'phone', room: ROOM, phone: '0a1b2c3d' });
    for (const bad of [`/pc/${ROOM.toUpperCase()}`, `/pc/${ROOM}/`, `/pc/${ROOM.slice(1)}`, `/phone/${ROOM}`, `/phone/${ROOM}/0a1b2c3`, `/pc/../${ROOM}`, `//pc/${ROOM}`]) {
      expect(route(bad), bad).toBeNull();
    }
    expect(isSocketPath('/pc/x')).toBe(true);
    expect(isSocketPath('/phone/x')).toBe(true);
    expect(isSocketPath('/assets/pc.js')).toBe(false);
  });
});

describe('the key a socket shows', () => {
  it('needs the protocol and exactly one 32-byte key', () => {
    expect(keyFrom(`vyro.1, k.${KEY}`)?.length).toBe(32);
    expect(keyFrom(`k.${KEY},vyro.1`)?.length).toBe(32);
    expect(keyFrom(`k.${KEY}`)).toBeNull();
    expect(keyFrom('vyro.1')).toBeNull();
    expect(keyFrom(`vyro.1, k.${KEY}, k.${KEY}`)).toBeNull();
    expect(keyFrom(`vyro.1, k.${KEY.slice(1)}`)).toBeNull();
    expect(keyFrom(`vyro.1, k.${KEY.slice(0, 42)}+`)).toBeNull();
    expect(keyFrom(null)).toBeNull();
  });
});

describe("the PC's control messages", () => {
  it('takes a list of up to eight phones with hashes', () => {
    expect(control(JSON.stringify({ t: 'phones', list: [{ id: '0a1b2c3d', hash: HASH }] }))).toEqual({ t: 'phones', list: [{ id: '0a1b2c3d', hash: HASH }] });
    expect(control(JSON.stringify({ t: 'phones', list: [] }))).toEqual({ t: 'phones', list: [] });
  });

  it('refuses anything else', () => {
    const nine = Array.from({ length: MAX_PHONES + 1 }, (_, i) => ({ id: i.toString(16).padStart(8, '0'), hash: HASH }));
    for (const bad of [
      'not json',
      '[]',
      'null',
      JSON.stringify({ t: 'other', list: [] }),
      JSON.stringify({ t: 'phones', list: nine }),
      JSON.stringify({ t: 'phones', list: [{ id: '0A1B2C3D', hash: HASH }] }),
      JSON.stringify({ t: 'phones', list: [{ id: '0a1b2c3d', hash: 'ab' }] }),
      JSON.stringify({ t: 'phones', list: [{ id: '0a1b2c3d', hash: HASH }, { id: '0a1b2c3d', hash: HASH }] }),
      JSON.stringify({ t: 'phones', list: [{ id: '0a1b2c3d', hash: HASH }], pad: 'x'.repeat(5000) }),
    ]) {
      expect(control(bad), bad.slice(0, 60)).toBeNull();
    }
  });

  it('cuts off phones signed out, and those whose key changed', () => {
    const a = { id: '0000000a', hash: HASH };
    const b = { id: '0000000b', hash: HASH };
    const b2 = { id: '0000000b', hash: 'cd'.repeat(32) };
    expect(cutOff([a, b], [b2])).toEqual({ gone: ['0000000a'], rekeyed: ['0000000b'] });
    expect(cutOff([a], [a, b])).toEqual({ gone: [], rekeyed: [] });
  });
});

describe('sizes and rates', () => {
  it('frames carry a seal and stay under the limit', () => {
    expect(frameFits(MIN_SEALED, 0)).toBe(true);
    expect(frameFits(MIN_SEALED - 1, 0)).toBe(false);
    expect(frameFits(4 + MIN_SEALED, 4)).toBe(true);
    expect(frameFits(MIN_SEALED, 4)).toBe(false);
    expect(frameFits(4 + MAX_FRAME, 4)).toBe(true);
    expect(frameFits(4 + MAX_FRAME + 1, 4)).toBe(false);
  });

  it('a token bucket allows the burst, then the rate', () => {
    const rate = { perSecond: 5, burst: 20 };
    const b = { tokens: rate.burst, at: 0 };
    let n = 0;
    while (allow(b, rate, 0)) n++;
    expect(n).toBe(20);
    expect(allow(b, rate, 100)).toBe(false);
    expect(allow(b, rate, 200)).toBe(true);
    expect(allow(b, rate, 60_000)).toBe(true);
    expect(b.tokens).toBe(19);
  });

  it('compares hashes in full', () => {
    expect(same(HASH, HASH)).toBe(true);
    expect(same(HASH, `${HASH.slice(0, -1)}c`)).toBe(false);
    expect(same(HASH, HASH.slice(1))).toBe(false);
  });
});

describe("the app's files", () => {
  it('get a strict policy, sockets only to this relay, and the right caching', () => {
    const page = assetHeaders(new URL('https://relay.example.workers.dev/'));
    expect(page['Content-Security-Policy']).toContain("default-src 'none'");
    expect(page['Content-Security-Policy']).toContain("connect-src 'self' wss://relay.example.workers.dev");
    expect(page['Content-Security-Policy']).toContain("frame-ancestors 'none'");
    // Scripts only from the relay itself, the audio worklet too (no blob: or inline code).
    expect(page['Content-Security-Policy']).toContain("script-src 'self';");
    expect(page['Permissions-Policy']).toBe('camera=(self), microphone=(self), geolocation=()');
    expect(page['Cache-Control']).toBe('no-cache');
    expect(page['Strict-Transport-Security']).toBeDefined();
    const file = assetHeaders(new URL('http://127.0.0.1:8787/assets/phone-1a2b.js'));
    expect(file['Cache-Control']).toContain('immutable');
    expect(file['Content-Security-Policy']).toContain('ws://127.0.0.1:8787');
    expect(file['Strict-Transport-Security']).toBeUndefined();
  });
});

describe("a room's daily allowance", () => {
  const DAY = 24 * 3600 * 1000;
  const noon = 20_000 * DAY + DAY / 2;

  it('counts what the PC sends today and stops at the limit', () => {
    let r = spend({ day: 0, bytes: 0 }, noon, 1000, 5000);
    expect(r).toEqual({ usage: { day: 20_000, bytes: 1000 }, ok: true, save: true });
    r = spend(r.usage, noon + 1, 4000, 5000);
    expect([r.ok, r.usage.bytes]).toEqual([true, 5000]);
    r = spend(r.usage, noon + 2, 1, 5000);
    expect([r.ok, r.usage.bytes, r.save]).toEqual([false, 5000, false]);
    // A new day (UTC) starts over.
    r = spend(r.usage, noon + DAY, 1, 5000);
    expect(r).toEqual({ usage: { day: 20_001, bytes: 1 }, ok: true, save: true });
  });

  it('is written down every few megabytes, not every frame', () => {
    const start = { day: 20_000, bytes: USAGE_STEP - 10 };
    expect(spend(start, noon, 5).save).toBe(false);
    expect(spend(start, noon, 10).save).toBe(true);
    // A day of live readings is far below the default.
    expect(5 * 1024 * 86_400).toBeLessThan(DAILY_BYTES);
  });
});
