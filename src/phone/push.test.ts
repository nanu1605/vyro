import { afterEach, describe, expect, it, vi } from 'vitest';
import { addPc, loadPcs, savePcs, updatePc } from './pcs';
import { DEFAULT_KINDS, keepUp, sameKey, support, type PushState } from './push';
import WORKER from './sw-push.js?raw';

const ROOM = '00112233445566778899aabbccddeeff';

describe('whether notifications can be had', () => {
  it('needs push, a service worker and notifications; an iPhone only from the Home Screen', () => {
    const all = { push: true, worker: true, notification: true };
    expect(support(all, false, false)).toBe('ok');
    expect(support(all, true, true)).toBe('ok');
    expect(support({ ...all, push: false }, true, false)).toBe('homeScreen');
    expect(support({ ...all, push: false }, true, true)).toBe('no');
    expect(support({ ...all, worker: false }, false, false)).toBe('no');
  });

  it('knows a subscription made with the relay’s key', () => {
    const key = new Uint8Array(65).fill(4);
    expect(sameKey(key.slice().buffer, key)).toBe(true);
    expect(sameKey(new Uint8Array(65).fill(5).buffer, key)).toBe(false);
    expect(sameKey(new Uint8Array(64).buffer, key)).toBe(false);
    expect(sameKey(null, key)).toBe(false);
  });
});

describe('keeping a PC up to date', () => {
  const sub = { endpoint: 'https://fcm.googleapis.com/fcm/send/new', p256dh: 'k', auth: 'a' };
  afterEach(() => vi.unstubAllGlobals());

  function browser(subscription: typeof sub | null, permission: NotificationPermission = 'granted') {
    vi.stubGlobal('Notification', { permission });
    const keys: Record<string, Uint8Array> = { p256dh: new Uint8Array([1, 2]), auth: new Uint8Array([3]) };
    vi.stubGlobal('navigator', {
      serviceWorker: {
        ready: Promise.resolve({
          pushManager: {
            getSubscription: () =>
              Promise.resolve(subscription && { endpoint: subscription.endpoint, getKey: (n: string) => keys[n]!.buffer }),
          },
        }),
      },
    });
  }

  function pc(state: PushState) {
    const asked: Array<[string, object | undefined]> = [];
    const ask = <T,>(m: string, p?: object): Promise<T> => {
      asked.push([m, p]);
      return Promise.resolve((m === 'push.state' ? state : { ...state, on: true }) as T);
    };
    return { ask, asked };
  }

  it('gives the PC this browser’s subscription when it lost it or has an old one, keeping its choices', async () => {
    browser(sub);
    const lost = pc({ on: false, kinds: DEFAULT_KINDS, endpoint: null, relay: true });
    await keepUp(lost.ask);
    expect(lost.asked.map((a) => a[0])).toEqual(['push.state', 'push.set']);
    expect(lost.asked[1]![1]).toEqual({ endpoint: sub.endpoint, p256dh: 'AQI', auth: 'Aw', kinds: DEFAULT_KINDS });

    const mine = { alerts: true, batteries: false, games: true, offline: false };
    const old = pc({ on: true, kinds: mine, endpoint: 'https://fcm.googleapis.com/fcm/send/old', relay: true });
    await keepUp(old.ask);
    expect(old.asked[1]![1]).toMatchObject({ endpoint: sub.endpoint, kinds: mine });
  });

  it('leaves a PC that has it, and asks nobody anything without permission', async () => {
    browser(sub);
    const same = pc({ on: true, kinds: DEFAULT_KINDS, endpoint: sub.endpoint, relay: true });
    await keepUp(same.ask);
    expect(same.asked.map((a) => a[0])).toEqual(['push.state']);

    browser(sub, 'default');
    const unasked = pc({ on: false, kinds: DEFAULT_KINDS, endpoint: null, relay: true });
    await keepUp(unasked.ask);
    expect(unasked.asked.map((a) => a[0])).toEqual(['push.state']);
  });
});

describe("a PC's notification wish", () => {
  it('is kept with the PC, and goes when turned off', () => {
    const m = new Map<string, string>();
    const store = { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
    let pcs = addPc([], { room: ROOM, phone: '0a1b2c3d', secret: 'A'.repeat(43) }, 'B'.repeat(43), 1);
    pcs = updatePc(pcs, ROOM, { push: true });
    savePcs(pcs, store);
    expect(loadPcs(store)[0]!.push).toBe(true);
    pcs = updatePc(pcs, ROOM, { push: false });
    expect('push' in pcs[0]!).toBe(false);
    m.set('vyro.pcs', JSON.stringify([{ ...pcs[0], push: 'yes' }]));
    expect(loadPcs(store)).toEqual([]);
  });
});

/** The service worker's notification part, run against a stand-in for its `self`. */
function worker() {
  const handlers: Record<string, (e: unknown) => void> = {};
  const shown: Array<{ title: string; options: NotificationOptions & { data?: { pc: string | null }; timestamp?: number; renotify?: boolean } }> = [];
  const windows: Array<{ url: string; focus: () => Promise<void>; postMessage: (m: unknown) => void; said: unknown[] }> = [];
  const opened: string[] = [];
  const self = {
    location: { origin: 'https://relay.example' },
    addEventListener: (t: string, f: (e: unknown) => void) => (handlers[t] = f),
    registration: { showNotification: (title: string, options: NotificationOptions) => (shown.push({ title, options }), Promise.resolve()) },
    clients: {
      matchAll: () => Promise.resolve(windows),
      openWindow: (url: string) => (opened.push(url), Promise.resolve()),
    },
  };
  new Function('self', WORKER)(self);
  const run = async (type: string, e: Record<string, unknown>) => {
    let done: Promise<unknown> = Promise.resolve();
    handlers[type]!({ ...e, waitUntil: (p: Promise<unknown>) => (done = p) });
    await done;
  };
  return { shown, windows, opened, run };
}

describe('the service worker', () => {
  const note = { v: 1, pc: ROOM, kind: 'alert', title: 'CPU at 97 °C', body: 'DESKTOP · held', tag: 'cpu-temp', ts: 1_760_000_000 };

  it('shows what the PC said, and a newer note on the same thing replaces the older', async () => {
    const w = worker();
    await w.run('push', { data: { text: () => JSON.stringify(note) } });
    expect(w.shown[0]!.title).toBe('CPU at 97 °C');
    expect(w.shown[0]!.options).toMatchObject({ body: 'DESKTOP · held', tag: `${ROOM}:cpu-temp`, renotify: true, timestamp: 1_760_000_000_000, data: { pc: ROOM } });
    // The relay's "stopped answering" has no time of its own: it shows when it came.
    const before = Date.now();
    await w.run('push', { data: { text: () => JSON.stringify({ ...note, ts: 0 }) } });
    expect(w.shown[1]!.options.timestamp).toBeGreaterThanOrEqual(before);
  });

  it('still shows something for a push that does not read', async () => {
    const w = worker();
    await w.run('push', { data: { text: () => 'garbage' } });
    await w.run('push', {});
    await w.run('push', { data: { text: () => JSON.stringify({ ...note, pc: 'not a room', v: 1 }) } });
    expect(w.shown.map((s) => s.title)).toEqual(['Vyro', 'Vyro', 'CPU at 97 °C']);
    expect(w.shown[0]!.options.body).toBe('Open Vyro to see what your PC said.');
    expect(w.shown[2]!.options.tag).toBeUndefined();
  });

  it('opens the app on that PC, or brings an open one forward and tells it', async () => {
    const w = worker();
    const close = vi.fn();
    await w.run('notificationclick', { notification: { close, data: { pc: ROOM } } });
    expect(close).toHaveBeenCalled();
    expect(w.opened).toEqual([`/?pc=${ROOM}`]);

    const win = { url: 'https://relay.example/', said: [] as unknown[], focus: vi.fn(() => Promise.resolve()), postMessage: (m: unknown) => win.said.push(m) };
    w.windows.push(win);
    await w.run('notificationclick', { notification: { close, data: { pc: ROOM } } });
    expect(win.focus).toHaveBeenCalled();
    expect(win.said).toEqual([{ t: 'open', pc: ROOM }]);
    expect(w.opened).toHaveLength(1);
  });
});
