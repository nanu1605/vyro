// Passkeys for the key page (2.5.5): only the device the owner set up makes a key, and only once
// it has checked them (fingerprint, face or PIN: Windows Hello on a PC). The owner, 2026-11-26:
// "Use some safety and privacy so only i can generate the key in whole jainav infotech".
//
// WebAuthn as browsers speak it, checked here with WebCrypto, without a library. No attestation:
// a passkey is let in by the setup passphrase, not by who made the authenticator. Nothing about
// the owner is kept: the passkey's name is the same for everyone, and its user id is random.

import { b64url, CHALLENGE_SECONDS, unb64 } from './rules';

/** The algorithms a passkey may sign with, as COSE numbers: ES256, EdDSA, and RS256 (which older
 * Windows Hello uses). */
export const ALGS = [-7, -8, -257] as const;

/** Bit 0 of the flags: the user was present. Bit 2: the user was verified (fingerprint, face or
 * PIN). */
const UP = 0x01;
const UV = 0x04;

async function mac(secret: string, data: Uint8Array): Promise<Uint8Array> {
  const key = await crypto.subtle.importKey('raw', new TextEncoder().encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', key, data)).slice(0, 16);
}

/** A challenge for a ceremony of `kind`, made at `now` (Unix seconds): the time, 16 random bytes
 * and a MAC of them with `secret`. Nothing is kept for it, so asking for many costs the checkout
 * nothing and keeps no one out; a challenge is marked used only once a ceremony with it is
 * complete. */
export async function newChallenge(secret: string, kind: string, now: number): Promise<string> {
  const body = new Uint8Array(20);
  new DataView(body.buffer).setUint32(0, now);
  body.set(crypto.getRandomValues(new Uint8Array(16)), 4);
  const tag = await mac(secret, new Uint8Array([...new TextEncoder().encode(kind), ...body]));
  return b64url(new Uint8Array([...body, ...tag]));
}

/** `value` is a challenge for `kind` this checkout made with `secret` in the last
 * CHALLENGE_SECONDS. */
export async function freshChallenge(secret: string, kind: string, value: string, now: number): Promise<boolean> {
  const b = bytes(value, 64);
  if (!b || b.length !== 36) return false;
  const body = b.subarray(0, 20);
  const tag = await mac(secret, new Uint8Array([...new TextEncoder().encode(kind), ...body]));
  let d = 0;
  for (let i = 0; i < 16; i++) d |= tag[i]! ^ b[20 + i]!;
  const at = new DataView(body.buffer, body.byteOffset, 4).getUint32(0);
  return d === 0 && at <= now && now - at <= CHALLENGE_SECONDS;
}

export async function sha256(b: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', b));
}

/** base64url (as the page sends bytes), or null if it is not that or is longer than `max` bytes. */
export function bytes(v: unknown, max = 4096): Uint8Array | null {
  if (typeof v !== 'string' || v.length === 0 || v.length > Math.ceil((max * 4) / 3) || !/^[A-Za-z0-9_-]+$/.test(v)) return null;
  try {
    return unb64(v);
  } catch {
    return null;
  }
}

/** The start of the authenticator's data: what it vouches for and how. */
export interface AuthData {
  rpIdHash: Uint8Array;
  flags: number;
  /** The signature counter: authenticators that keep one raise it at every use. */
  count: number;
}

export function authData(b: Uint8Array): AuthData | null {
  if (b.length < 37) return null;
  return { rpIdHash: b.slice(0, 32), flags: b[32]!, count: new DataView(b.buffer, b.byteOffset + 33, 4).getUint32(0) };
}

/** The authenticator speaks for this website (`rpId`), and the user was there and was checked. */
export async function vouched(a: AuthData, rpId: string): Promise<boolean> {
  const want = await sha256(new TextEncoder().encode(rpId));
  return a.rpIdHash.every((x, i) => x === want[i]) && (a.flags & UP) !== 0 && (a.flags & UV) !== 0;
}

/** What the browser says of a ceremony: its kind, the challenge it answers, and the page's
 * origin. */
export interface ClientData {
  type: string;
  challenge: string;
  origin: string;
  crossOrigin?: boolean;
}

export function clientData(b: Uint8Array): ClientData | null {
  try {
    const v: unknown = JSON.parse(new TextDecoder().decode(b));
    if (!v || typeof v !== 'object') return null;
    const c = v as Record<string, unknown>;
    if (typeof c.type !== 'string' || typeof c.challenge !== 'string' || typeof c.origin !== 'string') return null;
    return { type: c.type, challenge: c.challenge, origin: c.origin, crossOrigin: c.crossOrigin === true };
  } catch {
    return null;
  }
}

/** The ceremony the browser ran is this page's own: of `type`, on `origin`, not in another's
 * frame. */
export function ours(c: ClientData, type: 'webauthn.create' | 'webauthn.get', origin: string): boolean {
  return c.type === type && c.origin === origin && !c.crossOrigin;
}

function params(alg: number): { key: SubtleCryptoImportKeyAlgorithm; verify: SubtleCryptoSignAlgorithm } | null {
  switch (alg) {
    case -7:
      return { key: { name: 'ECDSA', namedCurve: 'P-256' }, verify: { name: 'ECDSA', hash: 'SHA-256' } };
    case -8:
      return { key: { name: 'Ed25519' }, verify: { name: 'Ed25519' } };
    case -257:
      return { key: { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, verify: { name: 'RSASSA-PKCS1-v1_5' } };
    default:
      return null;
  }
}

/** A passkey's public key (SPKI, as the browser gives it at setup), or null if it is not one of
 * ALGS. */
export async function publicKey(spki: Uint8Array, alg: number): Promise<CryptoKey | null> {
  const p = params(alg);
  if (!p) return null;
  try {
    return await crypto.subtle.importKey('spki', spki, p.key, false, ['verify']);
  } catch {
    return null;
  }
}

/** An ECDSA signature as authenticators give it (DER) as WebCrypto takes it: r and s, 32 bytes
 * each; null if it is not one. */
export function derToRaw(d: Uint8Array): Uint8Array | null {
  if (d.length < 8 || d[0] !== 0x30 || d[1] !== d.length - 2) return null;
  const out = new Uint8Array(64);
  let i = 2;
  for (const at of [0, 32]) {
    if (d[i] !== 0x02) return null;
    const len = d[i + 1]!;
    let n = d.subarray(i + 2, i + 2 + len);
    if (len === 0 || n.length !== len) return null;
    while (n.length > 32 && n[0] === 0) n = n.subarray(1);
    if (n.length > 32) return null;
    out.set(n, at + 32 - n.length);
    i += 2 + len;
  }
  return i === d.length ? out : null;
}

/** The passkey signed this sign-in: its signature over the authenticator's data and the hash of
 * what the browser said. */
export async function signed(key: CryptoKey, alg: number, auth: Uint8Array, client: Uint8Array, sig: Uint8Array): Promise<boolean> {
  const p = params(alg);
  const s = alg === -7 ? derToRaw(sig) : sig;
  if (!p || !s) return false;
  const data = new Uint8Array(auth.length + 32);
  data.set(auth);
  data.set(await sha256(client), auth.length);
  try {
    return await crypto.subtle.verify(p.verify, key, s, data);
  } catch {
    return false;
  }
}
