// The Vyro relay (1.4): serves the phone app, and passes sealed frames between a PC and its
// phones (`room.ts`). Sockets are `/pc/<room>` and `/phone/<room>/<phone>`; everything else is
// the app's files.

import { assetHeaders, isSocketPath, keyFrom, route } from './rules';

export { Room } from './room';

interface Env {
  ROOMS: DurableObjectNamespace;
  ASSETS: Fetcher;
}

function plain(status: number, text: string): Response {
  return new Response(text, {
    status,
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
}

export default {
  async fetch(req, env): Promise<Response> {
    const url = new URL(req.url);
    if (isSocketPath(url.pathname)) {
      const r = route(url.pathname);
      if (!r) return plain(404, 'Not found');
      if (req.headers.get('Upgrade')?.toLowerCase() !== 'websocket') return plain(426, 'Expected a WebSocket');
      if (!keyFrom(req.headers.get('Sec-WebSocket-Protocol'))) return plain(400, 'Expected a key');
      return env.ROOMS.get(env.ROOMS.idFromName(r.room)).fetch(req);
    }
    if (req.method !== 'GET' && req.method !== 'HEAD') return plain(405, 'Not allowed');
    const res = await env.ASSETS.fetch(req);
    const out = new Response(res.body, res);
    for (const [k, v] of Object.entries(assetHeaders(url))) out.headers.set(k, v);
    return out;
  },
} satisfies ExportedHandler<Env>;
