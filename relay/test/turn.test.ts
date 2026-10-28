// TURN for a phone's screen session (2.0.1), run as deployed (workerd), with the tests' own TURN
// service standing in for Cloudflare's: it checks the key and token the relay asks with, and
// answers as the real one does.

import { createServer, type Server } from 'node:http';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { key, relayAt, room, type Sock } from './sockets';

interface Asked {
  path: string;
  auth: string | undefined;
  body: string;
}

let server: TestHarness;
let relay: ReturnType<typeof relayAt>;
let turn: Server;
const asked: Asked[] = [];
/** What the TURN service answers next. */
let status = 201;
/** Rooms the owner lets use TURN; another room may not. */
const allowed = [room(), room()];

const ANSWER = {
  iceServers: [
    { urls: ['stun:stun.cloudflare.com:3478', 'stun:stun.cloudflare.com:53'] },
    {
      urls: ['turn:turn.cloudflare.com:3478?transport=udp', 'turn:turn.cloudflare.com:53?transport=udp', 'turns:turn.cloudflare.com:443?transport=tcp'],
      username: 'user-1',
      credential: 'secret-1',
    },
  ],
};
const GIVEN = [{ urls: ['turn:turn.cloudflare.com:3478?transport=udp', 'turns:turn.cloudflare.com:443?transport=tcp'], username: 'user-1', credential: 'secret-1' }];

beforeAll(async () => {
  turn = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      asked.push({ path: req.url ?? '', auth: req.headers.authorization, body: Buffer.concat(chunks).toString() });
      res.writeHead(status, { 'Content-Type': 'application/json' }).end(status === 201 ? JSON.stringify(ANSWER) : '{}');
    });
  });
  await new Promise<void>((done) => turn.listen(0, '127.0.0.1', done));
  const addr = turn.address();
  const turnBase = `http://127.0.0.1:${typeof addr === 'object' && addr ? addr.port : 0}`;
  server = createTestHarness({
    workers: [
      {
        configPath: fileURLToPath(new URL('./wrangler.push.test.jsonc', import.meta.url)),
        vars: { TURN_API: `${turnBase}/v1/turn/keys`, TURN_ROOMS: allowed.join(',') },
        secrets: { TURN_KEY_ID: 'key-1', TURN_KEY_API_TOKEN: 'token-1' },
      },
    ],
  });
  relay = relayAt((await server.listen()).url.toString().replace(/\/$/, ''));
});

afterAll(async () => {
  await server.close();
  turn.close();
});

/** A PC in room `r` with one phone, and that phone connected; what the phone heard first. */
async function phoneIn(r: string): Promise<{ s: Sock; first: unknown }> {
  const p = await relay.pc(r, key());
  const auth = key();
  await relay.register(p, [{ id: '0a1b2c3d', auth }]);
  const s = relay.sock(`/phone/${r}/0a1b2c3d`, auth);
  await s.opened;
  return { s, first: await s.json() };
}

describe('TURN for the screen', () => {
  it("is offered to a listed room's phones, from the TURN service with the owner's key", async () => {
    const { s, first } = await phoneIn(allowed[0]!);
    expect(first).toEqual({ t: 'pc', online: true, turn: true });
    const from = asked.length;
    s.ws.send('{"t":"ice"}');
    expect(await s.json()).toEqual({ t: 'ice', servers: GIVEN });
    expect(asked.slice(from)).toEqual([
      { path: '/v1/turn/keys/key-1/credentials/generate-ice-servers', auth: 'Bearer token-1', body: JSON.stringify({ ttl: 86400 }) },
    ]);
    // Asked again: the same, while most of their day is left.
    s.ws.send('{"t":"ice"}');
    expect(await s.json()).toEqual({ t: 'ice', servers: GIVEN });
    expect(asked.length).toBe(from + 1);
  });

  it('is not offered to a room not listed, whose phones may send it no text', async () => {
    const { s, first } = await phoneIn(room());
    expect(first).toEqual({ t: 'pc', online: true });
    const from = asked.length;
    s.ws.send('{"t":"ice"}');
    expect((await s.closed).code).toBe(1003);
    expect(asked.length).toBe(from);
  });

  it('gives none when the TURN service refuses, and the phone stays connected', async () => {
    status = 500;
    try {
      const { s } = await phoneIn(allowed[1]!);
      s.ws.send('{"t":"ice"}');
      expect(await s.json()).toEqual({ t: 'ice', servers: [] });
      expect(s.ws.readyState).toBe(WebSocket.OPEN);
    } finally {
      status = 201;
    }
  });
});
