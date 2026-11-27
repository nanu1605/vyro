// What Clokrix Pro costs, what its keys and codes look like, and how an activation is signed
// (2.5). Pure functions, tested on their own.

/** What Clokrix Pro costs, once, in the currency's smallest unit as Razorpay counts it: ₹10,000 in
 * India (including all taxes) and $100 elsewhere (the owner, 2026-11-16: "for India keep the pro
 * for 10,000 rupees (INR) and for rest of the countries keep it as 100$"). */
export const PRICES = {
  INR: { amount: 1_000_000, currency: 'INR' },
  USD: { amount: 10_000, currency: 'USD' },
} as const;
export type Currency = keyof typeof PRICES;

/** The price shown first to a buyer in `country` (ISO 3166, as Cloudflare names the connection's
 * country): rupees in India, dollars elsewhere. The buyer can pay the other instead. */
export function currencyFor(country: string | null | undefined): Currency {
  return country?.toUpperCase() === 'IN' ? 'INR' : 'USD';
}

/** `cur` as a buyer's choice of currency (`inr` or `usd`, any case), or null. */
export function chosenCurrency(cur: unknown): Currency | null {
  const c = typeof cur === 'string' ? cur.toUpperCase() : '';
  return c === 'INR' || c === 'USD' ? c : null;
}

/** "₹10,000" or "$100". */
export function shownPrice(c: Currency): string {
  return c === 'INR' ? '₹10,000' : '$100';
}

/** `amount` in `currency` is Clokrix Pro's price in it. */
export function isPrice(amount: number, currency: string): boolean {
  return Object.values(PRICES).some((p) => p.amount === amount && p.currency === currency);
}
/** A key moves to another PC at most once in this long. */
export const MOVE_DAYS = 30;
/** A purchase is refunded in full if asked for within this long (the owner, 2026-11-16). */
export const REFUND_DAYS = 14;
/** Requests one address may make in a minute, beyond which it waits. */
export const RATE_PER_MIN = 40;
/** The key page's passphrase is at least this long (2.5.5): it is all that stands between the
 * page and a key. */
export const MIN_PASSPHRASE = 20;
/** Wrong passphrases the key page's setup takes in an hour, from anyone; then it takes none, the
 * right one too, until the oldest of them is an hour old. */
export const WRONG_PER_HOUR = 20;
/** Devices the key page gives keys to, at most: the first, set up with the passphrase, and
 * those added from it (a phone, a password manager, a security key), as backups. */
export const MAX_DEVICES = 5;
/** A passkey ceremony's challenge is good once, for this long. */
export const CHALLENGE_SECONDS = 300;

/** Crockford's base32: no I, L, O or U, so a key read aloud or typed is not misread. */
const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

/** A new key: CLKX- and 20 random characters in fours of five (100 bits). */
export function newKey(random: (n: number) => Uint8Array = (n) => crypto.getRandomValues(new Uint8Array(n))): string {
  const bytes = random(20);
  const chars = [...bytes].map((b) => ALPHABET[b & 31]).join('');
  return `CLKX-${chars.slice(0, 5)}-${chars.slice(5, 10)}-${chars.slice(10, 15)}-${chars.slice(15, 20)}`;
}

/** A code for adding a device to the key page (2.5.5): 10 random characters in two fives (50
 * bits), shown on a device set up and typed on the other. Good once, for CHALLENGE_SECONDS. */
export function newCode(random: (n: number) => Uint8Array = (n) => crypto.getRandomValues(new Uint8Array(n))): string {
  const chars = [...random(10)].map((b) => ALPHABET[b & 31]).join('');
  return `${chars.slice(0, 5)}-${chars.slice(5)}`;
}

/** A code as typed (any case, spaces or none, O for 0 and so on), or null if it is not one. */
export function normalCode(typed: unknown): string | null {
  if (typeof typed !== 'string' || typed.length > 40) return null;
  const c = typed.toUpperCase().replace(/[\s-]/g, '').replace(/O/g, '0').replace(/[IL]/g, '1');
  if (!/^[0-9A-HJKMNP-TV-Z]{10}$/.test(c)) return null;
  return `${c.slice(0, 5)}-${c.slice(5)}`;
}

const SYSTEMS: Array<[RegExp, string]> = [
  [/iPhone/, 'iPhone'],
  [/iPad/, 'iPad'],
  [/Android/, 'Android'],
  [/CrOS/, 'ChromeOS'],
  [/Windows/, 'Windows'],
  [/Macintosh|Mac OS X/, 'Mac'],
  [/Linux/, 'Linux'],
];
const BROWSERS: Array<[RegExp, string]> = [
  [/Edg(e|A|iOS)?\//, 'Edge'],
  [/OPR\/|OPiOS\//, 'Opera'],
  [/SamsungBrowser\//, 'Samsung Internet'],
  [/Firefox\/|FxiOS\//, 'Firefox'],
  [/Chrome\/|CriOS\//, 'Chrome'],
  [/Safari\//, 'Safari'],
];

/** A name for a device on the key page (2.5.5), so the owner tells them apart when removing one:
 * the kind of system and browser its User-Agent names ("Windows, Edge"), from these fixed words
 * only; nothing else of it is kept. A passkey made on another device from the browser's window
 * (`attachment` "cross-platform") is that device: a phone by its QR code, or a security key.
 * Null if none is known. */
export function deviceName(ua: unknown, attachment?: unknown, transports?: unknown): string | null {
  if (attachment === 'cross-platform') {
    const t = Array.isArray(transports) ? transports : [];
    return t.includes('hybrid') ? 'A phone, by its QR code' : t.length > 0 ? 'A security key' : null;
  }
  if (typeof ua !== 'string') return null;
  const named = [SYSTEMS, BROWSERS].map((words) => words.find(([w]) => w.test(ua))?.[1]).filter((w) => w);
  return named.length > 0 ? named.join(', ') : null;
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

/** The passphrase typed is the one set: compared in the same time whatever either holds, their
 * lengths too. */
export async function samePassphrase(typed: string, set: string): Promise<boolean> {
  return same(await hmacHex('clokrix-key-page', typed), await hmacHex('clokrix-key-page', set));
}

/** What the key page's passkeys are let in under: the setup passphrase's hash, so that a new
 * passphrase starts the setup again. */
export async function setupId(passphrase: string): Promise<string> {
  return hmacHex('clokrix-key-page-setup', passphrase);
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
