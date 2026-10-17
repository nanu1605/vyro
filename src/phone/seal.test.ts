import { describe, expect, it } from 'vitest';
import raw from '../../crates/vyro-core/src/remote/relay/vectors.json?raw';
import { aad, authHash, b64url, deriveKeys, hex, hkdf, open, OVERHEAD, seal, unb64url, unhex, type Bytes, type Dir } from './seal';

interface Vectors {
  salt: string;
  cases: Array<{ secret: string; room: string; phone: string; dir: Dir; nonce: string; plain: string; auth: string; authHash: string; seal: string; aad: string; frame: string }>;
  refused: Array<{ why: string; case: number; room: string; phone: string; dir: Dir; frame: string }>;
}

const v = JSON.parse(raw) as Vectors;
const h = (s: string): Bytes => unhex(s)!;

describe('sealing matches the PC (shared vectors)', () => {
  it.each(v.cases.map((c, i) => [i, c] as const))('case %i: keys, frame and opening', async (_, c) => {
    const secret = h(c.secret);
    expect(hex(await hkdf(secret, 'auth'))).toBe(c.auth);
    expect(hex(await hkdf(secret, 'seal'))).toBe(c.seal);
    const keys = await deriveKeys(secret);
    expect(await authHash(keys.auth)).toBe(c.authHash);
    const ad = aad(h(c.room), h(c.phone), c.dir);
    expect(hex(ad)).toBe(c.aad);
    const plain = new TextEncoder().encode(c.plain);
    const frame = await seal(keys, ad, plain, h(c.nonce));
    expect(hex(frame)).toBe(c.frame);
    expect(frame.length).toBe(plain.length + OVERHEAD);
    expect(new TextDecoder().decode((await open(keys, ad, frame))!)).toBe(c.plain);
  });

  it.each(v.refused.map((r) => [r.why, r] as const))('refused: %s', async (_, r) => {
    const keys = await deriveKeys(h(v.cases[r.case]!.secret));
    expect(await open(keys, aad(h(r.room), h(r.phone), r.dir), h(r.frame))).toBeNull();
  });

  it('a fresh nonce every time', async () => {
    const keys = await deriveKeys(new Uint8Array(32).fill(7));
    const ad = aad(new Uint8Array(16), new Uint8Array(4), 'toPc');
    const msg = new TextEncoder().encode('same');
    const [a, b] = [await seal(keys, ad, msg), await seal(keys, ad, msg)];
    expect(hex(a)).not.toBe(hex(b));
  });
});

describe('encodings', () => {
  it('hex refuses what is not whole bytes of hex', () => {
    expect(unhex('0a1B')).toEqual(new Uint8Array([10, 27]));
    expect(unhex('abc')).toBeNull();
    expect(unhex('zz')).toBeNull();
  });

  it('base64url round trips without padding and refuses other alphabets', () => {
    for (const n of [0, 1, 2, 3, 31, 32, 33]) {
      const b = Uint8Array.from({ length: n }, (_, i) => (i * 37 + 250) % 256);
      const s = b64url(b);
      expect(s).not.toMatch(/[+/=]/);
      expect(unb64url(s)).toEqual(b);
    }
    expect(unb64url('a+b=')).toBeNull();
    expect(unb64url('abcde')).toBeNull();
  });
});
