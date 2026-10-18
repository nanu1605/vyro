// The PCs this phone is paired with (1.4), kept in this browser's storage for this relay. Each
// PC gave the phone its own secret; the relay only ever sees a key derived from it.

import { SECRET_LEN, unb64url } from './seal';

export interface PairedPc {
  /** The PC's room on the relay, hex. */
  room: string;
  /** This phone's id there, hex. */
  phone: string;
  /** The secret in use, base64url. */
  secret: string;
  /** The phone's own secret, sent with its first hello, until the PC has taken it. */
  next?: string;
  /** The PC's name, from its last `info`. */
  name: string;
  /** Last opened (ms), so the list starts with the one used most recently. */
  used: number;
  /** Its wired adapter's address, while it lets phones have another PC wake it (1.5). */
  wake?: string;
}

/** A pairing link's contents. */
export interface Link {
  room: string;
  phone: string;
  secret: string;
}

export const MAX_PCS = 8;
const KEY = 'vyro.pcs';

const ROOM = /^[0-9a-f]{32}$/;
const PHONE = /^[0-9a-f]{8}$/;
const MAC = /^[0-9A-F]{2}(-[0-9A-F]{2}){5}$/;

function secretOk(s: string): boolean {
  return unb64url(s)?.length === SECRET_LEN;
}

/** The pairing link in an address's fragment, `#p=1.<room>.<phone>.<secret>` (browsers never
 * send the fragment to a server). */
export function parseLink(hash: string): Link | null {
  const m = /^#p=1\.([0-9a-f]{32})\.([0-9a-f]{8})\.([A-Za-z0-9_-]{43})$/.exec(hash);
  if (!m || !secretOk(m[3]!)) return null;
  return { room: m[1]!, phone: m[2]!, secret: m[3]! };
}

/** A scanned or pasted pairing link, if it is one for this relay (`origin`). */
export function linkFrom(text: string, origin: string): Link | 'elsewhere' | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  const link = parseLink(url.hash);
  if (!link) return null;
  return url.origin === origin ? link : 'elsewhere';
}

function valid(p: unknown): p is PairedPc {
  if (typeof p !== 'object' || p === null) return false;
  const v = p as Partial<PairedPc>;
  return (
    typeof v.room === 'string' &&
    ROOM.test(v.room) &&
    typeof v.phone === 'string' &&
    PHONE.test(v.phone) &&
    typeof v.secret === 'string' &&
    secretOk(v.secret) &&
    (v.next === undefined || (typeof v.next === 'string' && secretOk(v.next))) &&
    typeof v.name === 'string' &&
    typeof v.used === 'number' &&
    (v.wake === undefined || (typeof v.wake === 'string' && MAC.test(v.wake)))
  );
}

/** Most recently used first; entries that do not look right are dropped. */
export function loadPcs(store: Pick<Storage, 'getItem'> = localStorage): PairedPc[] {
  try {
    const v = JSON.parse(store.getItem(KEY) ?? '[]') as unknown;
    return Array.isArray(v) ? v.filter(valid).sort((a, b) => b.used - a.used).slice(0, MAX_PCS) : [];
  } catch {
    return [];
  }
}

export function savePcs(pcs: PairedPc[], store: Pick<Storage, 'setItem'> = localStorage): void {
  try {
    store.setItem(KEY, JSON.stringify(pcs));
  } catch {
    /* storage unavailable: pairing lasts until the page closes */
  }
}

/** Adds a PC from its link (again, if it was paired before), with the phone's own `next` secret
 * to send in its first hello. */
export function addPc(pcs: PairedPc[], link: Link, next: string, now: number): PairedPc[] {
  const before = pcs.find((p) => p.room === link.room);
  const pc: PairedPc = { ...link, next, name: before?.name ?? 'Your PC', used: now };
  return [pc, ...pcs.filter((p) => p.room !== link.room)].slice(0, MAX_PCS);
}

/** The PC took the phone's own secret. */
export function confirmNext(pcs: PairedPc[], room: string): PairedPc[] {
  return pcs.map((p) => {
    if (p.room !== room || !p.next) return p;
    const { next, ...rest } = p;
    return { ...rest, secret: next };
  });
}

export function updatePc(pcs: PairedPc[], room: string, change: Partial<Pick<PairedPc, 'name' | 'used' | 'wake'>>): PairedPc[] {
  return pcs.map((p) => {
    if (p.room !== room) return p;
    const next = { ...p, ...change };
    // No address any more: the owner turned "Wake from phone" off.
    if ('wake' in change && change.wake === undefined) delete next.wake;
    return next;
  });
}

/** Other paired PCs this phone can have woken: they gave their address. */
export function sleepers(pcs: PairedPc[], except: string | null): Array<{ name: string; mac: string; room: string }> {
  return pcs.filter((p) => p.room !== except && p.wake).map((p) => ({ name: p.name, mac: p.wake!, room: p.room }));
}

export function removePc(pcs: PairedPc[], room: string): PairedPc[] {
  return pcs.filter((p) => p.room !== room);
}
