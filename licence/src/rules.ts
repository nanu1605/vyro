// What Clokrix Pro costs, what its keys and codes look like, and how an activation is signed
// (2.5). Pure functions, tested on their own.

/** $100, once (Razorpay counts in the currency's smallest unit). */
export const PRICE = { amount: 10000, currency: 'USD' } as const;
/** A key moves to another PC at most once in this long. */
export const MOVE_DAYS = 30;
/** Requests one address may make in a minute, beyond which it waits. */
export const RATE_PER_MIN = 40;

/** Crockford's base32: no I, L, O or U, so a key read aloud or typed is not misread. */
const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/** A new key: CLKX- and 20 random characters in fours of five (100 bits). */
export function newKey(random: (n: number) => Uint8Array = (n) => crypto.getRandomValues(new Uint8Array(n))): string {
  const bytes = random(20);
  const chars = [...bytes].map((b) => ALPHABET[b & 31]).join('');
  return `CLKX-${chars.slice(0, 5)}-${chars.slice(5, 10)}-${chars.slice(10, 15)}-${chars.slice(15, 20)}`;
}

/** A key as typed (any case, spaces or none, O for 0 and so on), or null if it is not one. */
export function normalKey(typed: string): string | null {
  const s = typed.toUpperCase().replace(/[\s-]/g, '');
  if (!s.startsWith('CLKX')) return null;
  const c = s.slice(4).replace(/O/g, '0').replace(/[IL]/g, '1');
  if (!/^[0-9A-HJKMNP-TV-Z]{20}$/.test(c)) return null;
  return `CLKX-${c.slice(0, 5)}-${c.slice(5, 10)}-${c.slice(10, 15)}-${c.slice(15, 20)}`;
}

/** The code the app makes for a purchase it waits for: 32 hex digits. */
export function isClaim(s: string | null | undefined): s is string {
  return !!s && /^[0-9a-f]{32}$/.test(s);
}

/** A PC as the app names it: SHA-256 of its Windows id with Clokrix's salt, 64 hex digits. */
export function isPc(s: unknown): s is string {
  return typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);
}

export function hex(b: Uint8Array): string {
  return [...b].map((x) => x.toString(16).padStart(2, '0')).join('');
}

export function b64url(b: Uint8Array): string {
  let s = '';
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function unb64(s: string): Uint8Array {
  const t = atob(s.replace(/-/g, '+').replace(/_/g, '/'));
  return Uint8Array.from(t, (c) => c.charCodeAt(0));
}

/** HMAC-SHA256 of `text` with `secret`, as hex: how Razorpay signs a payment and a webhook. */
export async function hmacHex(secret: string, text: string): Promise<string> {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return hex(new Uint8Array(await crypto.subtle.sign('HMAC', key, enc.encode(text))));
}

/** Two strings compared in the same time whatever they hold. */
export function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return d === 0;
}

/** What an activation says: this licence is Pro on this PC. */
export interface Activation {
  v: 1;
  /** The licence's id (not its key). */
  lic: string;
  pc: string;
  edition: 'pro';
  /** When it was signed, Unix seconds. */
  issued: number;
}

/** The app's private key (PKCS #8, base64), as set on the Worker. */
export async function signingKey(pkcs8: string): Promise<CryptoKey> {
  return crypto.subtle.importKey('pkcs8', unb64(pkcs8.trim()), { name: 'Ed25519' }, false, ['sign']);
}

/** `<payload>.<signature>`, both base64url: the payload's exact bytes are what is signed. */
export async function signActivation(key: CryptoKey, a: Activation): Promise<string> {
  const payload = new TextEncoder().encode(JSON.stringify(a));
  const sig = new Uint8Array(await crypto.subtle.sign('Ed25519', key, payload));
  return `${b64url(payload)}.${b64url(sig)}`;
}
