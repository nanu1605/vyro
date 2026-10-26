import { describe, expect, it } from 'vitest';
import { compareVersions, NOTES, notesSince, phoneNotes, shortVersion, type Release } from './whatsnew';

const r = (version: string): Release => ({ version, items: [{ icon: 'sparkles', title: version, text: '' }] });
const SAMPLE = [r('1.7.0'), r('1.6.0'), r('1.5.1'), r('1.5.0')];

describe('what’s new', () => {
  it('has notes for the version being built', () => {
    const [version] = Object.values(import.meta.glob<string>('../package.json', { eager: true, import: 'version' }));
    expect(version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(NOTES.map((n) => n.version)).toContain(version);
  });

  it('lists the notes newest first, each with something to say', () => {
    for (let i = 1; i < NOTES.length; i++) expect(compareVersions(NOTES[i - 1]!.version, NOTES[i]!.version)).toBe(1);
    for (const n of NOTES) {
      expect(n.items.length).toBeGreaterThan(0);
      for (const item of n.items) expect(item.title && item.text).toBeTruthy();
    }
  });

  it('compares versions', () => {
    expect(compareVersions('1.10.0', '1.9.9')).toBe(1);
    expect(compareVersions('1.5.0', '1.5.0')).toBe(0);
    expect(compareVersions('1.5', '1.5.0')).toBe(0);
    expect(compareVersions('1.4.1', '1.5.0')).toBe(-1);
    expect(compareVersions('1.5.0-beta', '1.5.0')).toBe(0);
  });

  it('shows every version after the one seen, up to this one', () => {
    expect(notesSince('1.5.0', '1.7.0', SAMPLE).map((n) => n.version)).toEqual(['1.7.0', '1.6.0', '1.5.1']);
    expect(notesSince('1.6.0', '1.6.0', SAMPLE)).toEqual([]);
    // A build older than the newest notes shows none from its future.
    expect(notesSince('1.5.0', '1.6.0', SAMPLE).map((n) => n.version)).toEqual(['1.6.0', '1.5.1']);
  });

  it('shows every version’s notes after an update from before 1.5, which kept no version', () => {
    expect(notesSince(null, '1.5.0', SAMPLE).map((n) => n.version)).toEqual(['1.5.0']);
    expect(notesSince(null, '1.6.0', SAMPLE).map((n) => n.version)).toEqual(['1.6.0', '1.5.1', '1.5.0']);
    // 1.7.0 over 1.4.1, as released: 1.5 and 1.6 came only within it.
    expect(notesSince(null, '1.7.0').map((n) => n.version)).toEqual(['1.7.0', '1.6.0', '1.5.0']);
  });

  it('tells the phone only of what the open PC has', () => {
    const notes: Release[] = [
      { version: '1.7.0', items: [{ icon: 'screen', title: 'Screen', text: '', phone: true }, { icon: 'sparkles', title: 'PC only', text: '' }] },
      { version: '1.6.0', items: [{ icon: 'phone', title: 'Presets', text: '', phone: true }] },
      { version: '1.5.0', items: [{ icon: 'phone', title: 'Control', text: '', phone: true }] },
    ];
    const titles = (seen: string | null, pc: string) => {
      const n = phoneNotes(seen, pc, notes);
      return [n.upTo, n.items.map((i) => i.title)];
    };
    // A PC on the newest Vyro: everything after what the phone showed, phone items only.
    expect(titles('1.5.0', '1.7.0')).toEqual(['1.7.0', ['Screen', 'Presets']]);
    // The app from before 1.7 showed none: every phone item up to the PC's Vyro.
    expect(titles(null, '1.7.0')).toEqual(['1.7.0', ['Screen', 'Presets', 'Control']]);
    // A PC not yet updated: nothing from its future, and the rest is kept for when it is.
    expect(titles(null, '1.4.1')).toEqual(['1.4.1', []]);
    expect(titles('1.5.0', '1.6.0')).toEqual(['1.6.0', ['Presets']]);
    expect(titles('1.6.0', '1.6.0')).toEqual(['1.6.0', []]);
    // Shown already for a newer PC: nothing for an older one.
    expect(titles('1.7.0', '1.5.0')).toEqual(['1.5.0', []]);
    // A PC newer than this phone app: up to the app's newest notes.
    expect(titles('1.6.0', '1.8.0')).toEqual(['1.7.0', ['Screen']]);
  });

  it('shortens a .0 version', () => {
    expect(shortVersion('1.5.0')).toBe('1.5');
    expect(shortVersion('1.5.1')).toBe('1.5.1');
    expect(shortVersion('1.10.0')).toBe('1.10');
  });
});
