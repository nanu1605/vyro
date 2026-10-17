// End-to-end sealing with the PC (1.4), in WebCrypto: the same keys and frames as
// `vyro_core::remote::relay::seal`, checked against the same vectors (`seal.test.ts`). The relay
// sees the auth key; the seal key never leaves this device.

export type Bytes = Uint8Array<ArrayBuffer>;

export const SALT: Bytes = new TextEncoder().encode('vyro-relay-1');
export const VERSION = 1;
export const NONCE_LEN = 12;
const TAG_LEN = 16;
/** Bytes a seal adds to a message. */
export const OVERHEAD = 1 + NONCE_LEN + TAG_LEN;
export const SECRET_LEN = 32;
export const ROOM_LEN = 16;
export const PHONE_LEN = 4;

/** Who a frame is for. */
export type Dir = 'toPhone' | 'toPc';

export interface Keys {
  /** Shown to the relay. */
  auth: Bytes;
  seal: CryptoKey;
}

/** 32 bytes of HKDF-SHA256 from a phone's secret. */
export async function hkdf(secret: Bytes, info: string): Promise<Bytes> {
  const base = await crypto.subtle.importKey('raw', secret, 'HKDF', false, ['deriveBits']);
  const bits = await crypto.subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt: SALT, info: new TextEncoder().encode(info) }, base, 256);
  return new Uint8Array(bits);
}

export async function deriveKeys(secret: Bytes): Promise<Keys> {
  const [auth, raw] = await Promise.all([hkdf(secret, 'auth'), hkdf(secret, 'seal')]);
  const seal = await crypto.subtle.importKey('raw', raw, 'AES-GCM', false, ['encrypt', 'decrypt']);
  return { auth, seal };
}

/** What a frame is bound to: the room, the phone and the direction. */
export function aad(room: Bytes, phone: Bytes, dir: Dir): Bytes {
  const tag = new TextEncoder().encode('vyro1');
  const out = new Uint8Array(tag.length + ROOM_LEN + PHONE_LEN + 1);
  out.set(tag, 0);
  out.set(room, tag.length);
  out.set(phone, tag.length + ROOM_LEN);
  out[out.length - 1] = dir === 'toPhone' ? 0 : 1;
  return out;
}

export function randomBytes(n: number): Bytes {
  return crypto.getRandomValues(new Uint8Array(n));
}

/** Seals `plain` with a fresh nonce (pass one only to check against the vectors). */
export async function seal(keys: Keys, ad: Bytes, plain: Bytes, nonce: Bytes = randomBytes(NONCE_LEN)): Promise<Bytes> {
  const sealed = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce, additionalData: ad }, keys.seal, plain));
  const frame = new Uint8Array(1 + NONCE_LEN + sealed.length);
  frame[0] = VERSION;
  frame.set(nonce, 1);
  frame.set(sealed, 1 + NONCE_LEN);
  return frame;
}

/** The message in a frame, or `null` when it was not sealed with these keys for this place and
 * direction, or was changed on the way. */
export async function open(keys: Keys, ad: Bytes, frame: Bytes): Promise<Bytes | null> {
  if (frame.length < OVERHEAD || frame[0] !== VERSION) return null;
  try {
    const plain = await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv: frame.slice(1, 1 + NONCE_LEN), additionalData: ad },
      keys.seal,
      frame.slice(1 + NONCE_LEN),
    );
    return new Uint8Array(plain);
  } catch {
    return null;
  }
}

/** SHA-256 of the auth key, hex: what the relay keeps for this phone. */
export async function authHash(auth: Bytes): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest('SHA-256', auth)));
}

export function hex(b: Uint8Array): string {
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

/** `null` unless `s` is whole bytes of lower- or upper-case hex. */
export function unhex(s: string): Bytes | null {
  if (s.length % 2 || !/^[0-9a-fA-F]*$/.test(s)) return null;
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
  return out;
}

/** Base64url without padding, as in pairing links and the relay's key header. */
export function b64url(b: Uint8Array): string {
  let s = '';
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function unb64url(s: string): Bytes | null {
  if (!/^[A-Za-z0-9_-]*$/.test(s) || s.length % 4 === 1) return null;
  try {
    const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}
