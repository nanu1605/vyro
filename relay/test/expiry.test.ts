// A room is forgotten when its PC has been away for its whole life (90 days; 3 s here), counted
// from when the PC was last connected, however long that connection lasted.

import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, expect, it } from 'vitest';
import { key, relayAt, room } from './sockets';

const TTL = 3000;
let server: TestHarness;
let relay: ReturnType<typeof relayAt>;

beforeAll(async () => {
  server = createTestHarness({ workers: [{ configPath: fileURLToPath(new URL('./wrangler.expiry.test.jsonc', import.meta.url)) }] });
  relay = relayAt((await server.listen()).url.toString().replace(/\/$/, ''));
});

afterAll(async () => {
  await server.close();
});

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Whether phone `x` is let into room `r` now: the relay tells it at once whether the PC is there,
 * and refuses it (closes at once) when the room was forgotten. */
async function letIn(r: string, x: { id: string; auth: Uint8Array }): Promise<boolean> {
  const s = relay.sock(`/phone/${r}/${x.id}`, x.auth);
  const heard = !(await s.quiet(1000));
  s.close();
  return heard;
}

it('a PC connected for longer than a room lives keeps the room for a whole life after it leaves', async () => {
  const r = room();
  const p = await relay.pc(r, key());
  const a = { id: '0000000a', auth: key() };
  const b = { id: '0000000b', auth: key() };
  await relay.register(p, [a, b]);
  const sa = relay.sock(`/phone/${r}/${a.id}`, a.auth);
  expect(await sa.json()).toEqual({ t: 'pc', online: true });
  await p.json();
  // Connected past the room's first check, and leaving just before its second.
  await sleep(TTL * 1.8);
  p.close();
  const left = Date.now();
  expect(await sa.json()).toEqual({ t: 'pc', online: false });
  await sleep(left + TTL / 2 - Date.now());
  expect(await letIn(r, b)).toBe(true);
  await sleep(left + TTL * 1.5 - Date.now());
  expect(await letIn(r, b)).toBe(false);
  expect((await sa.closed).code).toBe(4410);
  // The room is the next PC's to claim.
  const other = await relay.pc(r, key());
  other.close();
});
