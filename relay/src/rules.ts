// The relay's rules (1.4), kept free of the Workers runtime so they are tested as is: the
// addresses, the key a socket shows, the PC's control messages, sizes and rates.

/** The protocol a client must offer and the relay answers with. */
export const PROTOCOL = 'vyro.1';
/** Phones a PC may register (as many as pair with it on the local network). */
export const MAX_PHONES = 8;
/** Open sockets per phone (a second tab or a reconnect overlapping the old socket). */
export const SOCKETS_PER_PHONE = 2;
/** Largest data frame, either way. A view is a few kilobytes. */
export const MAX_FRAME = 64 * 1024;
/** Smallest data frame: a sealed empty message (version, nonce, tag). */
export const MIN_SEALED = 1 + 12 + 16;
export const PHONE_ID_BYTES = 4;
/** A room is forgotten this long after its PC was last connected. */
export const ROOM_TTL_MS = 90 * 24 * 3600 * 1000;

/** Messages a second, and the burst allowed above that. */
export const PC_RATE = { perSecond: 100, burst: 200 };
export const PHONE_RATE = { perSecond: 5, burst: 20 };

export type Route = { kind: 'pc'; room: string } | { kind: 'phone'; room: string; phone: string };

const ROOM = /^[0-9a-f]{32}$/;
const PHONE = /^[0-9a-f]{8}$/;
const HASH = /^[0-9a-f]{64}$/;

/** `/pc/<room>` or `/phone/<room>/<phone>`, lower-case hex; anything else is not a socket. */
export function route(path: string): Route | null {
  const parts = path.split('/');
  if (parts[0] !== '') return null;
  if (parts.length === 3 && parts[1] === 'pc' && ROOM.test(parts[2]!)) return { kind: 'pc', room: parts[2]! };
  if (parts.length === 4 && parts[1] === 'phone' && ROOM.test(parts[2]!) && PHONE.test(parts[3]!)) {
    return { kind: 'phone', room: parts[2]!, phone: parts[3]! };
  }
  return null;
}

/** Paths the relay keeps for sockets, so a wrong one is a 404 and never the app's page. */
export function isSocketPath(path: string): boolean {
  return path.startsWith('/pc/') || path.startsWith('/phone/');
}

function unb64url(s: string): Uint8Array | null {
  if (!/^[A-Za-z0-9_-]+$/.test(s) || s.length % 4 === 1) return null;
  try {
    const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4));
    return Uint8Array.from(bin, (c) => c.charCodeAt(0));
  } catch {
    return null;
  }
}

/** The 32-byte key in `Sec-WebSocket-Protocol: vyro.1, k.<base64url>`. Browsers cannot set other
 * headers on a WebSocket, and unlike the address this header is not logged. */
export function keyFrom(header: string | null): Uint8Array | null {
  if (!header) return null;
  const offered = header.split(',').map((s) => s.trim());
  if (!offered.includes(PROTOCOL)) return null;
  const keys = offered.filter((s) => s.startsWith('k.'));
  if (keys.length !== 1) return null;
  const key = unb64url(keys[0]!.slice(2));
  return key && key.length === 32 ? key : null;
}

export function hex(b: Uint8Array): string {
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

export async function sha256(b: Uint8Array): Promise<string> {
  return hex(new Uint8Array(await crypto.subtle.digest('SHA-256', b)));
}

/** Same hex hashes, compared without an early exit. */
export function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let d = 0;
  for (let i = 0; i < a.length; i++) d |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return d === 0;
}

export interface Phone {
  id: string;
  /** SHA-256 of the phone's auth key, hex. */
  hash: string;
}

/** What a PC may tell the relay. */
export type Control = { t: 'phones'; list: Phone[] };

/** A PC's text message, or `null` when it is not one the relay knows (the socket is closed). */
export function control(text: string): Control | null {
  if (text.length > 4096) return null;
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof v !== 'object' || v === null) return null;
  const m = v as { t?: unknown; list?: unknown };
  if (m.t !== 'phones' || !Array.isArray(m.list) || m.list.length > MAX_PHONES) return null;
  const list: Phone[] = [];
  for (const p of m.list as unknown[]) {
    if (typeof p !== 'object' || p === null) return null;
    const { id, hash } = p as { id?: unknown; hash?: unknown };
    if (typeof id !== 'string' || !PHONE.test(id) || typeof hash !== 'string' || !HASH.test(hash)) return null;
    if (list.some((q) => q.id === id)) return null;
    list.push({ id, hash });
  }
  return { t: 'phones', list };
}

/** Phones whose sockets must close after a new list: `gone` were signed out; `rekeyed` hold a
 * key that changed (the phone switched to its own secret, or was paired again). */
export function cutOff(before: Phone[], after: Phone[]): { gone: string[]; rekeyed: string[] } {
  const gone: string[] = [];
  const rekeyed: string[] = [];
  for (const p of before) {
    const now = after.find((q) => q.id === p.id);
    if (!now) gone.push(p.id);
    else if (now.hash !== p.hash) rekeyed.push(p.id);
  }
  return { gone, rekeyed };
}

/** A data frame's size is acceptable: `prefix` bytes of phone id (from the PC) and a seal. */
export function frameFits(length: number, prefix: number): boolean {
  return length >= prefix + MIN_SEALED && length <= prefix + MAX_FRAME;
}

export interface Bucket {
  tokens: number;
  at: number;
}

/** Token bucket: whether one more message is allowed at `now` (ms); updates `b`. */
export function allow(b: Bucket, rate: { perSecond: number; burst: number }, now: number): boolean {
  b.tokens = Math.min(rate.burst, b.tokens + ((now - b.at) / 1000) * rate.perSecond);
  b.at = now;
  if (b.tokens < 1) return false;
  b.tokens -= 1;
  return true;
}

/** When a room is forgotten, given when its PC was last connected. */
export function expiry(lastPc: number, ttl = ROOM_TTL_MS): number {
  return lastPc + ttl;
}

/** Headers for the phone app's files: its own scripts only, sockets only to this relay, no
 * frames, no referrer; hashed files cached for good, the rest checked every time. */
export function assetHeaders(url: URL): Record<string, string> {
  const ws = url.protocol === 'https:' ? `wss://${url.host}` : `ws://${url.host}`;
  const h: Record<string, string> = {
    'Content-Security-Policy': [
      "default-src 'none'",
      "script-src 'self'",
      "style-src 'self' 'unsafe-inline'",
      "img-src 'self' data: blob:",
      "font-src 'self'",
      `connect-src 'self' ${ws}`,
      "manifest-src 'self'",
      "worker-src 'self'",
      "media-src 'self' blob:",
      "frame-ancestors 'none'",
      "base-uri 'none'",
      "form-action 'none'",
    ].join('; '),
    'X-Content-Type-Options': 'nosniff',
    'Referrer-Policy': 'no-referrer',
    'Permissions-Policy': 'camera=(self), microphone=(), geolocation=()',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'X-Frame-Options': 'DENY',
    'X-Robots-Tag': 'noindex, nofollow',
    'Cache-Control': url.pathname.startsWith('/assets/') ? 'public, max-age=31536000, immutable' : 'no-cache',
  };
  if (url.protocol === 'https:') h['Strict-Transport-Security'] = 'max-age=31536000';
  return h;
}
