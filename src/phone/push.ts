// Notifications with the app closed (2.0): this browser's push subscription, made with the relay's
// key and given to each PC that should send them, sealed like everything else the phone tells a
// PC. The PC seals each notification for this browser and the relay passes it on: neither the
// relay nor the push service can read it. The service worker shows them (`sw-push.js`).

import type { PushKinds } from '../bindings/PushKinds';
import { b64url, unb64url } from './seal';

export type { PushKinds };

/** What a PC answers to `push.state`, `push.set` and `push.off`. */
export interface PushState {
  on: boolean;
  kinds: PushKinds;
  /** The push service address the PC has for this phone. */
  endpoint: string | null;
  /** The relay sends notifications. */
  relay: boolean;
}

/** What the PC is given: where the push service takes this browser's notifications, and its keys. */
export interface Subscription {
  endpoint: string;
  p256dh: string;
  auth: string;
}

export const DEFAULT_KINDS: PushKinds = { alerts: true, batteries: true, games: false, offline: true };

export type Support = 'ok' | 'homeScreen' | 'no';

/** Whether this browser can show notifications from a closed app: on an iPhone, only the app
 * added to the Home Screen can. */
export function support(has: { push: boolean; worker: boolean; notification: boolean }, ios: boolean, standalone: boolean): Support {
  if (has.push && has.worker && has.notification) return 'ok';
  return ios && !standalone ? 'homeScreen' : 'no';
}

export const BLOCKED = 'Notifications are blocked for Vyro. Allow them in the browser’s settings for this site (on iPhone: Settings → Notifications → Vyro).';
const NO_WORKER = 'The app’s background part is not running. Reload the app and try again.';
const WORKER_MS = 10_000;

export function fromSubscription(sub: PushSubscription): Subscription {
  const key = sub.getKey('p256dh');
  const auth = sub.getKey('auth');
  if (!key || !auth) throw new Error('This browser gave no keys for notifications.');
  return { endpoint: sub.endpoint, p256dh: b64url(new Uint8Array(key)), auth: b64url(new Uint8Array(auth)) };
}

/** Same bytes. */
export function sameKey(a: ArrayBuffer | null | undefined, b: Uint8Array): boolean {
  if (!a || a.byteLength !== b.length) return false;
  const x = new Uint8Array(a);
  return x.every((v, i) => v === b[i]);
}

async function registration(): Promise<ServiceWorkerRegistration> {
  const wait = new Promise<never>((_, fail) => setTimeout(() => fail(new Error(NO_WORKER)), WORKER_MS));
  return Promise.race([navigator.serviceWorker.ready, wait]);
}

/** The relay's key, which a subscription is made with (`/push/key`). */
async function relayKey(): Promise<Uint8Array> {
  const res = await fetch('/push/key', { cache: 'no-store' });
  if (!res.ok) throw new Error('This relay can’t send notifications.');
  const key = unb64url(((await res.json()) as { key?: string }).key ?? '');
  if (!key || key.length !== 65) throw new Error('This relay can’t send notifications.');
  return key;
}

/** This browser's subscription, made now if it has none (or one made with another key): asks
 * the person first, if they were not asked before. Only from a tap: browsers ask only then. */
export async function subscribe(): Promise<Subscription> {
  if (Notification.permission === 'denied') throw new Error(BLOCKED);
  if (Notification.permission !== 'granted' && (await Notification.requestPermission()) !== 'granted') throw new Error(BLOCKED);
  const [key, reg] = await Promise.all([relayKey(), registration()]);
  let sub = await reg.pushManager.getSubscription();
  if (sub && !sameKey(sub.options.applicationServerKey, key)) {
    await sub.unsubscribe().catch(() => false);
    sub = null;
  }
  sub ??= await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key as BufferSource });
  return fromSubscription(sub);
}

/** This browser's subscription as it is, without asking anyone; `null` when it has none or
 * notifications are not allowed. */
export async function current(): Promise<Subscription | null> {
  if (typeof Notification === 'undefined' || Notification.permission !== 'granted' || !('serviceWorker' in navigator)) return null;
  try {
    const sub = await (await registration()).pushManager.getSubscription();
    return sub ? fromSubscription(sub) : null;
  } catch {
    return null;
  }
}

/** Gives a PC that should send notifications (the phone turned them on for it) this browser's
 * subscription again when it lost it or has an old one: the push service forgot it, or the
 * browser made a new one. Only while notifications are still allowed, without asking. */
export async function keepUp(ask: <T>(m: string, p?: object) => Promise<T>): Promise<PushState | null> {
  const st = await ask<PushState>('push.state');
  const sub = await current();
  if (!sub || (st.on && st.endpoint === sub.endpoint)) return st;
  return ask<PushState>('push.set', { ...sub, kinds: st.on ? st.kinds : DEFAULT_KINDS });
}
