import { describe, expect, it } from 'vitest';
import type { RemoteView } from '../types';
import { confirmNext, type PairedPc } from './pcs';
import { PcLink, type LinkState, type PhoneInfo } from './relay';
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
    expect(await pc0.read(first.sent[0]!)).toEqual({ t: 'hello', name: 'Pixel · Chrome', next: S1 });

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
    expect(await pc1.read(second.sent[0]!)).toEqual({ t: 'hello', name: 'Pixel · Chrome' });
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
    s.binary(await pc1.frame({ t: 'info', pc: 'DESKTOP-7' }));
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
