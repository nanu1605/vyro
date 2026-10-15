// The companion page's calls to the Vyro app that served it (P26): same origin, the session in
// an HttpOnly cookie the page never sees.

import type { RemoteInfo, RemoteView } from '../types';

export class NotPaired extends Error {}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, { cache: 'no-store', credentials: 'same-origin', ...init });
  if (r.status === 401) throw new NotPaired('This device is not paired.');
  if (!r.ok) {
    const body = (await r.json().catch(() => null)) as { error?: string } | null;
    throw new Error(body?.error ?? `${r.status} ${r.statusText}`);
  }
  return (r.status === 204 ? null : await r.json()) as T;
}

const post = (body: unknown): RequestInit => ({
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(body),
});

export const getInfo = () => call<RemoteInfo>('/api/info');
export const getView = () => call<RemoteView>('/api/view');
export const pair = (code: string, name: string) => call<{ device: string; expires: number }>('/api/pair', post({ code, name }));
export const signOut = () => call<null>('/api/logout', post({}));

/** The view every second until `close`. `onDown` fires when the stream breaks; the caller
 * decides whether to reconnect (it may have been signed out). */
export function stream(onView: (v: RemoteView) => void, onDown: () => void): () => void {
  const es = new EventSource('/api/stream');
  es.onmessage = (e) => {
    try {
      onView(JSON.parse(e.data as string) as RemoteView);
    } catch {
      /* a broken message: wait for the next one */
    }
  };
  es.onerror = () => {
    es.close();
    onDown();
  };
  return () => es.close();
}
