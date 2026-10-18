import { describe, expect, it } from 'vitest';
import { addPc, confirmNext, linkFrom, loadPcs, MAX_PCS, parseLink, removePc, savePcs, sleepers, updatePc, type PairedPc } from './pcs';

const ROOM = '00112233445566778899aabbccddeeff';
const SECRET = 'A'.repeat(43);
const NEXT = 'B'.repeat(43);
const HASH = `#p=1.${ROOM}.0a1b2c3d.${SECRET}`;

function memory(): Storage {
  const m = new Map<string, string>();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => void m.set(k, v),
    removeItem: (k) => void m.delete(k),
    clear: () => m.clear(),
    key: () => null,
    get length() {
      return m.size;
    },
  };
}

describe('pairing links', () => {
  it('read the room, the phone and a 32-byte secret', () => {
    expect(parseLink(HASH)).toEqual({ room: ROOM, phone: '0a1b2c3d', secret: SECRET });
    for (const bad of [`#p=2.${ROOM}.0a1b2c3d.${SECRET}`, `#p=1.${ROOM}.0a1b2c3.${SECRET}`, `#p=1.${ROOM.toUpperCase()}.0a1b2c3d.${SECRET}`, `#p=1.${ROOM}.0a1b2c3d.${SECRET}A`, `#p=1.${ROOM}.0a1b2c3d`, '#pair=K7QM4XPA', '']) {
      expect(parseLink(bad), bad).toBeNull();
    }
  });

  it('a scanned link counts only for this relay', () => {
    expect(linkFrom(`https://relay.example.dev/${HASH}`, 'https://relay.example.dev')).toMatchObject({ room: ROOM });
    expect(linkFrom(`https://other.example.dev/${HASH}`, 'https://relay.example.dev')).toBe('elsewhere');
    expect(linkFrom('not a link', 'https://relay.example.dev')).toBeNull();
    expect(linkFrom('https://relay.example.dev/', 'https://relay.example.dev')).toBeNull();
  });
});

describe('the paired PCs', () => {
  it('keep the phone’s own secret until the PC takes it', () => {
    let pcs = addPc([], parseLink(HASH)!, NEXT, 1);
    expect(pcs[0]).toMatchObject({ secret: SECRET, next: NEXT, name: 'Your PC' });
    pcs = confirmNext(pcs, ROOM);
    expect(pcs[0]!.secret).toBe(NEXT);
    expect(pcs[0]!.next).toBeUndefined();
    pcs = updatePc(pcs, ROOM, { name: 'DESKTOP-7' });
    // Paired again from a new link: one entry, its name kept.
    pcs = addPc(pcs, parseLink(HASH)!, NEXT, 2);
    expect(pcs).toHaveLength(1);
    expect(pcs[0]!.name).toBe('DESKTOP-7');
    expect(removePc(pcs, ROOM)).toEqual([]);
  });

  it('are saved, read back newest first, and damaged entries are dropped', () => {
    const store = memory();
    const a: PairedPc = { room: ROOM, phone: '0a1b2c3d', secret: SECRET, name: 'A', used: 1 };
    const b: PairedPc = { ...a, room: 'f'.repeat(32), name: 'B', used: 5 };
    savePcs([a, b, { ...a, room: 'nope' } as PairedPc], store);
    expect(loadPcs(store).map((p) => p.name)).toEqual(['B', 'A']);
    store.setItem('vyro.pcs', '{broken');
    expect(loadPcs(store)).toEqual([]);
  });

  it('hold at most eight', () => {
    let pcs: PairedPc[] = [];
    for (let i = 0; i < MAX_PCS + 2; i++) pcs = addPc(pcs, { room: i.toString(16).padStart(32, '0'), phone: '00000001', secret: SECRET }, NEXT, i);
    expect(pcs).toHaveLength(MAX_PCS);
  });

  it('keeps a PC\'s wake address while it gives one, and lists the others to wake', () => {
    const a: PairedPc = { room: ROOM, phone: '0a1b2c3d', secret: 'A'.repeat(43), name: 'Desk', used: 2 };
    const b: PairedPc = { ...a, room: 'ff'.repeat(16), name: 'Laptop', used: 1 };
    let list = updatePc([a, b], ROOM, { wake: 'A8-A1-59-12-34-56' });
    expect(sleepers(list, b.room)).toEqual([{ name: 'Desk', mac: 'A8-A1-59-12-34-56', room: ROOM }]);
    expect(sleepers(list, ROOM)).toEqual([]);
    const store = new Map<string, string>();
    savePcs(list, { setItem: (k, v) => store.set(k, v) });
    expect(loadPcs({ getItem: (k) => store.get(k) ?? null })[0]?.wake).toBe('A8-A1-59-12-34-56');
    // Turned off on the PC: the address goes.
    list = updatePc(list, ROOM, { wake: undefined });
    expect(list[0]).not.toHaveProperty('wake');
    // A stored address that does not look like one drops the entry.
    store.set('vyro.pcs', JSON.stringify([{ ...a, wake: 'not a mac' }]));
    expect(loadPcs({ getItem: (k) => store.get(k) ?? null })).toEqual([]);
  });
});
