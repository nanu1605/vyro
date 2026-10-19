import { describe, expect, it, vi } from 'vitest';
import type { RemoteView } from '../types';
import { confirmNext, type PairedPc } from './pcs';
import { PcLink, RequestError, type LinkState, type PhoneInfo } from './relay';
import { aad, b64url, deriveKeys, open, seal, unhex, type Bytes } from './seal';

const ROOM = '00112233445566778899aabbccddeeff';
const PHONE = '0a1b2c3d';
const S0 = b64url(new Uint8Array(32).fill(1));
const S1 = b64url(new Uint8Array(32).fill(2));

/** A WebSocket the test plays the relay through. */
class FakeSocket {
  readyState = 0;
  binaryType = 'blob';
  sent: Array<string | Bytes> = [];
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string | ArrayBuffer }) => void) | null = null;
  onclose: ((e: { code: number }) => void) | null = null;
  constructor(
    readonly url: string,
    readonly protocols: string[],
  ) {}
  send(d: string | Bytes) {
    this.sent.push(d);
  }
  close(code = 1005) {
    this.end(code);
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  text(s: string) {
    this.onmessage?.({ data: s });
  }
  binary(b: Bytes) {
    this.onmessage?.({ data: b.slice().buffer });
  }
  end(code: number) {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.onclose?.({ code });
  }
}

const flush = async () => {
  for (let i = 0; i < 20; i++) await new Promise((r) => setTimeout(r, 0));
};

/** The PC's side of the sealing, for one secret. */
async function pcSide(secret: string) {
  const keys = await deriveKeys(new Uint8Array(32).fill(secret === S0 ? 1 : 2));
  const room = unhex(ROOM)!;
  const phone = unhex(PHONE)!;
  return {
    auth: `k.${b64url(keys.auth)}`,
    read: async (frame: string | Bytes) => {
      if (typeof frame === 'string') return null;
      const plain = await open(keys, aad(room, phone, 'toPc'), frame);
      return plain ? (JSON.parse(new TextDecoder().decode(plain)) as Record<string, unknown>) : null;
    },
    frame: async (msg: object) => seal(keys, aad(room, phone, 'toPhone'), new TextEncoder().encode(JSON.stringify(msg))),
  };
}

function setup(start: PairedPc) {
  let pcs = [start];
  const sockets: FakeSocket[] = [];
  const states: LinkState[] = [];
  const infos: PhoneInfo[] = [];
  const views: RemoteView[] = [];
  const link = new PcLink(
    () => pcs.find((p) => p.room === ROOM),
    {
      onState: (s) => states.push(s),
      onInfo: (i) => infos.push(i),
      onView: (v) => views.push(v),
      onRekeyed: () => (pcs = confirmNext(pcs, ROOM)),
    },
    {
      name: 'Pixel · Chrome',
      watch: true,
      base: 'https://relay.example.dev',
      socket: (url, protocols) => {
        const s = new FakeSocket(url, protocols);
        sockets.push(s);
        return s as unknown as WebSocket;
      },
    },
  );
  return { link, sockets, states, infos, views, pcs: () => pcs };
}

const VIEW = (ts: number) => ({ ts }) as unknown as RemoteView;

describe('a phone and its PC through the relay', () => {
  it('pairs: hands the PC its own secret, reconnects on it, then reads', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S0, next: S1, name: 'Your PC', used: 1 });
    const [pc0, pc1] = [await pcSide(S0), await pcSide(S1)];
    t.link.start();
    await flush();
    const first = t.sockets[0]!;
    expect(first.url).toBe(`wss://relay.example.dev/phone/${ROOM}/${PHONE}`);
    expect(first.protocols).toEqual(['vyro.1', pc0.auth]);
    first.open();
    first.text('{"t":"pc","online":true}');
    await flush();
    expect(await pc0.read(first.sent[0]!)).toEqual({ t: 'hello', name: 'Pixel · Chrome', protocol: 2, next: S1 });

    first.binary(await pc0.frame({ t: 'rekeyed' }));
    first.end(4001);
    await flush();
    expect(t.pcs()[0]).toMatchObject({ secret: S1 });
    expect(t.pcs()[0]!.next).toBeUndefined();

    const second = t.sockets[1]!;
    expect(second.protocols).toEqual(['vyro.1', pc1.auth]);
    second.open();
    second.text('{"t":"pc","online":true}');
    await flush();
    expect(await pc1.read(second.sent[0]!)).toEqual({ t: 'hello', name: 'Pixel · Chrome', protocol: 2 });
    second.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 1 }));
    await flush();
    expect(t.infos[0]?.pc).toBe('DESKTOP-7');
    expect(t.states.at(-1)).toBe('online');
    expect(await pc1.read(second.sent[1]!)).toEqual({ t: 'watch' });

    second.binary(await pc1.frame({ t: 'view', view: VIEW(5) }));
    second.binary(await pc1.frame({ t: 'view', view: VIEW(4) }));
    await flush();
    expect(t.views.map((v) => v.ts)).toEqual([5]);

    // Frames sealed for another phone, or by someone else, are ignored.
    second.binary(await pc0.frame({ t: 'view', view: VIEW(9) }));
    await flush();
    expect(t.views).toHaveLength(1);
    t.link.stop();
  });

  it('when the answer was lost, it tries its own secret; when both are refused, the code expired', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S0, next: S1, name: 'Your PC', used: 1 });
    t.link.start();
    await flush();
    t.sockets[0]!.end(4403);
    await flush();
    expect(t.sockets[1]!.protocols).toEqual(['vyro.1', (await pcSide(S1)).auth]);
    t.sockets[1]!.end(4403);
    await flush();
    expect(t.states.at(-1)).toBe('expired');
  });

  it('knows a PC that is asleep, a phone signed out, an old PC, and an unreachable relay', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S1, name: 'DESKTOP-7', used: 1 });
    const pc1 = await pcSide(S1);
    t.link.start();
    await flush();
    const s = t.sockets[0]!;
    s.open();
    s.text('{"t":"pc","online":false}');
    await flush();
    expect(t.states.at(-1)).toBe('asleep');
    s.text('{"t":"pc","online":true}');
    await flush();
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 0 }));
    await flush();
    expect(t.states.at(-1)).toBe('tooOld');

    s.end(1006);
    await flush();
    expect(t.states.at(-1)).toBe('unreachable');
    t.link.wake();
    await flush();
    expect(t.sockets).toHaveLength(2);
    t.sockets[1]!.end(4403);
    await flush();
    expect(t.states.at(-1)).toBe('signedOut');
    t.link.stop();
  });

  it('asks a protocol 2 PC: every message after hello carries the session and a new count', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S1, name: 'DESKTOP-7', used: 1 });
    const pc1 = await pcSide(S1);
    t.link.start();
    await flush();
    const s = t.sockets[0]!;
    s.open();
    s.text('{"t":"pc","online":true}');
    await flush();
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 2, session: 'abc', control: false, methods: ['processes'] }));
    await flush();
    expect(t.infos.at(-1)).toMatchObject({ session: 'abc', control: false });
    expect(await pc1.read(s.sent.at(-1)!)).toEqual({ t: 'watch', s: 'abc', n: 1 });

    const asked = t.link.request<{ pid: number }[]>('processes');
    const p2 = t.link.request('power', { action: 'lock' }).then(
      () => null,
      (e: Error) => e,
    );
    await flush();
    const r1 = (await pc1.read(s.sent.at(-2)!))!;
    const r2 = (await pc1.read(s.sent.at(-1)!))!;
    expect(r1).toEqual({ t: 'req', id: 1, m: 'processes', p: {}, s: 'abc', n: 2 });
    expect(r2).toMatchObject({ t: 'req', id: 2, m: 'power', p: { action: 'lock' }, n: 3 });

    // A long answer comes in parts, in any order.
    const text = JSON.stringify({ t: 'res', id: 1, ok: [{ pid: 4242 }] });
    const half = Math.floor(text.length / 2);
    s.binary(await pc1.frame({ t: 'part', id: 1, i: 1, n: 2, d: text.slice(half) }));
    s.binary(await pc1.frame({ t: 'part', id: 1, i: 0, n: 2, d: text.slice(0, half) }));
    s.binary(await pc1.frame({ t: 'res', id: 2, err: 'This phone may only look.' }));
    await flush();
    expect(await asked).toEqual([{ pid: 4242 }]);
    expect((await p2)?.message).toMatch(/may only look/);

    // The owner allows control: the same session, so the count goes on.
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 2, session: 'abc', control: true, methods: ['processes', 'power'] }));
    await flush();
    expect(t.infos.at(-1)?.control).toBe(true);
    void t.link.request('power', { action: 'lock' }).catch(() => {});
    await flush();
    expect(await pc1.read(s.sent.at(-1)!)).toMatchObject({ m: 'power', n: 4 });

    // The PC goes to sleep: what was asked fails.
    const lost = t.link.request('processes');
    s.text('{"t":"pc","online":false}');
    await expect(lost).rejects.toBeInstanceOf(RequestError);
    t.link.stop();
  });

  it('asks no faster than the relay takes: a burst waits its turn instead of being cut off', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S1, name: 'DESKTOP-7', used: 1 });
    const pc1 = await pcSide(S1);
    t.link.start();
    await flush();
    const s = t.sockets[0]!;
    const at: number[] = [];
    const send = s.send.bind(s);
    s.send = (d) => {
      at.push(Date.now());
      send(d);
    };
    s.open();
    s.text('{"t":"pc","online":true}');
    await flush();
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 2, session: 'abc', methods: ['processes'] }));
    await flush();
    // A tab opening asks for more at once than the relay takes in one go.
    for (let i = 0; i < 24; i++) void t.link.request('processes').catch(() => {});
    await vi.waitFor(() => expect(s.sent).toHaveLength(26), { timeout: 5000, interval: 50 });
    // The relay's rule (`PHONE_RATE` and `allow` in relay/src/rules.ts): 20 at once, then 5 a second.
    let tokens = 20;
    let last = at[0]!;
    const refused = at.filter((when) => {
      tokens = Math.min(20, tokens + ((when - last) / 1000) * 5);
      last = when;
      if (tokens < 1) return true;
      tokens -= 1;
      return false;
    });
    expect(refused).toEqual([]);
    // Every one went, in order.
    const counts = await Promise.all(s.sent.slice(1).map(async (f) => (await pc1.read(f))?.n));
    expect(counts).toEqual(Array.from({ length: 25 }, (_, i) => i + 1));
    t.link.stop();
  }, 10_000);

  it('shows a protocol 1 PC, but asks it nothing', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S1, name: 'DESKTOP-7', used: 1 });
    const pc1 = await pcSide(S1);
    t.link.start();
    await flush();
    const s = t.sockets[0]!;
    s.open();
    s.text('{"t":"pc","online":true}');
    await flush();
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7', protocol: 1 }));
    await flush();
    expect(t.states.at(-1)).toBe('online');
    expect(await pc1.read(s.sent.at(-1)!)).toEqual({ t: 'watch' });
    await expect(t.link.request('processes')).rejects.toThrow('Update Vyro on the PC');
    t.link.stop();
  });

  it('signing out tells the PC', async () => {
    const t = setup({ room: ROOM, phone: PHONE, secret: S1, name: 'DESKTOP-7', used: 1 });
    const pc1 = await pcSide(S1);
    t.link.start();
    await flush();
    const s = t.sockets[0]!;
    s.open();
    await t.link.signOut();
    expect(await pc1.read(s.sent.at(-1)!)).toEqual({ t: 'bye' });
    expect(s.readyState).toBe(3);
  });
});
