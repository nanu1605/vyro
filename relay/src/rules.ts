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

export function unb64url(s: string): Uint8Array | null {
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

/** A notification for one phone (2.0), sealed by the PC for the phone's browser: the relay posts
 * it to the phone's push service and cannot read it. */
export interface Sealed {
  /** The phone's id. */
  to: string;
  /** Its push service's address for it. */
  endpoint: string;
  /** The sealed body, base64url. */
  body: string;
  /** Seconds the push service keeps it for a phone that is off. */
  ttl: number;
}

/** What a PC may tell the relay: its phones; and from 2.0 a notification to pass on, the ones to
 * send should it go quiet (its will), and that it is about to go (it quits, sleeps or shuts
 * down), so that its going is not taken for going quiet. */
export type Control =
  | { t: 'phones'; list: Phone[] }
  | ({ t: 'push'; urgency: 'high' | 'normal' } & Sealed)
  | { t: 'will'; list: Sealed[] }
  | { t: 'leaving' };

/** A push service takes at most 4096 bytes; a sealed notification has at least its header (86),
 * its end mark and its tag. */
export const MAX_PUSH_BODY = 4096;
const MIN_PUSH_BODY = 86 + 1 + 16;
export const MAX_ENDPOINT = 1024;
export const MAX_TTL = 24 * 3600;
/** Longest text a PC may send: a will for every phone. */
const MAX_CONTROL = 64 * 1024;
const B64URL = /^[A-Za-z0-9_-]+$/;

function sealedFrom(v: unknown): Sealed | null {
  if (typeof v !== 'object' || v === null) return null;
  const { to, endpoint, body, ttl } = v as Record<string, unknown>;
  if (typeof to !== 'string' || !PHONE.test(to)) return null;
  if (typeof endpoint !== 'string' || endpoint.length > MAX_ENDPOINT) return null;
  if (typeof body !== 'string' || !B64URL.test(body)) return null;
  const bytes = Math.floor((body.length * 3) / 4);
  if (bytes < MIN_PUSH_BODY || bytes > MAX_PUSH_BODY) return null;
  if (typeof ttl !== 'number' || !Number.isInteger(ttl) || ttl < 0 || ttl > MAX_TTL) return null;
  return { to, endpoint, body, ttl };
}

/** A PC's text message, or `null` when it is not one the relay knows (the socket is closed). */
export function control(text: string): Control | null {
  if (text.length > MAX_CONTROL) return null;
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof v !== 'object' || v === null) return null;
  const m = v as { t?: unknown; list?: unknown; urgency?: unknown };
  if (m.t === 'leaving') return { t: 'leaving' };
  if (m.t === 'push') {
    const sealed = sealedFrom(v);
    if (!sealed || (m.urgency !== 'high' && m.urgency !== 'normal')) return null;
    return { t: 'push', urgency: m.urgency, ...sealed };
  }
  if (m.t === 'will') {
    if (!Array.isArray(m.list) || m.list.length > MAX_PHONES) return null;
    const list = (m.list as unknown[]).map(sealedFrom);
    if (list.some((x) => x === null)) return null;
    return { t: 'will', list: list as Sealed[] };
  }
  if (text.length > 4096) return null;
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

/** Bytes a room's PC may send its phones in a day (UTC), in data frames (1.7). A day of live
 * readings is far less, and so is the screen's slow view (20 minutes a day): this only stops a
 * room from streaming through the shared relay without end. */
export const DAILY_BYTES = 1024 ** 3;
/** What a room sent today is saved each time it passes another this many bytes. */
export const USAGE_STEP = 16 * 1024 * 1024;
const DAY_MS = 24 * 3600 * 1000;

export interface Usage {
  /** Days since 1970 (UTC). */
  day: number;
  bytes: number;
}

/**
 * Counts `n` more bytes at `now` (ms) against `limit`: `ok` when they may go (bytes over the
 * limit are not counted), `save` when the count should be written down.
 */
export function spend(u: Usage, now: number, n: number, limit = DAILY_BYTES): { usage: Usage; ok: boolean; save: boolean } {
  const day = Math.floor(now / DAY_MS);
  const today = u.day === day ? u : { day, bytes: 0 };
  if (today.bytes + n > limit) return { usage: today, ok: false, save: today !== u };
  const bytes = today.bytes + n;
  return {
    usage: { day, bytes },
    ok: true,
    save: today !== u || Math.floor(bytes / USAGE_STEP) !== Math.floor(today.bytes / USAGE_STEP),
  };
}

/** Push services the relay posts to (2.0): the browsers' own. Anything else is refused, so the
 * relay cannot be used to post elsewhere. */
export const PUSH_HOSTS = ['fcm.googleapis.com', 'android.googleapis.com', 'push.services.mozilla.com', 'push.apple.com', 'notify.windows.com'];

/** `endpoint` when it is a push service's address: `https`, on one of `PUSH_HOSTS` or a host
 * under one (`web.push.apple.com`); `extra` hosts (a test's own) may also use plain `http`. */
export function pushTarget(endpoint: string, extra: string[] = []): URL | null {
  let url: URL;
  try {
    url = new URL(endpoint);
  } catch {
    return null;
  }
  if (url.username || url.password) return null;
  const host = url.hostname.toLowerCase();
  if (extra.includes(host) && (url.protocol === 'http:' || url.protocol === 'https:')) return url;
  if (url.protocol !== 'https:' || (url.port !== '' && url.port !== '443')) return null;
  return PUSH_HOSTS.some((h) => host === h || host.endsWith(`.${h}`)) ? url : null;
}

/** Notifications a room's PC may have sent at once, and after that (one every 5 s). */
export const PUSH_RATE = { perSecond: 0.2, burst: 24 };
/** Notifications a room's PC may send in a day (UTC). */
export const DAILY_PUSHES = 300;
/** The PC sends a `ping` every 30 s; this long without one, it went quiet. */
export const SILENT_MS = 100_000;
/** How often a room with a will checks that its PC still pings. */
export const CHECK_MS = 120_000;
/** A PC whose connection ended unannounced has this long to come back before its phones hear. */
export const GRACE_MS = 120_000;
/** A PC's `leaving` covers its connection ending within this long after. */
export const LEAVING_MS = 10 * 60_000;

/** The JSON Web Token a push service asks of the relay (VAPID, RFC 8292): ES256 over the push
 * service's origin `aud`, valid until `exp` (s), naming the relay `sub`. */
export async function vapidToken(key: CryptoKey, aud: string, sub: string, exp: number): Promise<string> {
  const enc = (o: unknown) => b64url(new TextEncoder().encode(JSON.stringify(o)));
  const data = `${enc({ typ: 'JWT', alg: 'ES256' })}.${enc({ aud, exp, sub })}`;
  const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, key, new TextEncoder().encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}

export function b64url(b: Uint8Array): string {
  let s = '';
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
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
    // The camera scans a pairing code; the microphone records voice for the PC (1.7).
    'Permissions-Policy': 'camera=(self), microphone=(self), geolocation=()',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'X-Frame-Options': 'DENY',
    'X-Robots-Tag': 'noindex, nofollow',
    'Cache-Control': url.pathname.startsWith('/assets/') ? 'public, max-age=31536000, immutable' : 'no-cache',
  };
  if (url.protocol === 'https:') h['Strict-Transport-Security'] = 'max-age=31536000';
  return h;
}

/** A TURN server for a phone's screen session (2.0.1): where no direct path exists (most mobile
 * networks), the picture goes through it, still sealed end to end, instead of the slow view.
 * The credentials come from Cloudflare's TURN service, with the owner's key, for the rooms the
 * owner lists; they last a day, and one is handed out again while at least half of that is
 * left. */
export const TURN_API = 'https://rtc.live.cloudflare.com/v1/turn/keys';
export const TURN_TTL_S = 24 * 3600;
export const TURN_REUSE_MS = (TURN_TTL_S * 1000) / 2;
/** After a failed request, the next waits this long. */
export const TURN_RETRY_MS = 60_000;
/** How long the TURN service has to answer. */
export const TURN_TIMEOUT_MS = 5000;

export interface IceServer {
  urls: string[];
  username?: string;
  credential?: string;
}

/** Whether `room` may use TURN: `rooms` is the owner's list of rooms (comma-separated), or `*`
 * for every room on the relay. None given: none. */
export function turnAllowed(rooms: string | undefined, room: string): boolean {
  const list = (rooms ?? '')
    .split(',')
    .map((r) => r.trim().toLowerCase())
    .filter(Boolean);
  return list.includes('*') || list.includes(room.toLowerCase());
}

/** The TURN servers in the TURN service's answer, as a phone's browser takes them: no STUN (the
 * phone has its own), no port 53 (browsers refuse it), each with its username and credential.
 * Null when the answer has none. */
export function iceServers(body: unknown): IceServer[] | null {
  const raw = (body as { iceServers?: unknown } | null)?.iceServers;
  const list = Array.isArray(raw) ? raw : raw ? [raw] : [];
  const out: IceServer[] = [];
  for (const s of list as { urls?: unknown; username?: unknown; credential?: unknown }[]) {
    const urls = (Array.isArray(s?.urls) ? s.urls : [s?.urls]).filter(
      (u): u is string => typeof u === 'string' && /^turns?:/.test(u) && !/:53(\?|$)/.test(u),
    );
    if (!urls.length || typeof s.username !== 'string' || typeof s.credential !== 'string') continue;
    out.push({ urls, username: s.username, credential: s.credential });
  }
  return out.length ? out : null;
}

/** What a phone may say to the relay itself, as text (2.0.1): only "ice", asking for the TURN
 * servers, and only after the relay said it has them. */
export function phoneAsksIce(text: string): boolean {
  try {
    return (JSON.parse(text) as { t?: unknown })?.t === 'ice';
  } catch {
    return false;
  }
}
