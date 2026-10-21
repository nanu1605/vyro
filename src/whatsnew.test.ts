import { describe, expect, it } from 'vitest';
import { compareVersions, NOTES, notesSince, shortVersion, type Release } from './whatsnew';

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

  it('shows this version’s notes after an update from before 1.5', () => {
    expect(notesSince(null, '1.5.0', SAMPLE).map((n) => n.version)).toEqual(['1.5.0']);
    expect(notesSince(null, '1.6.0', SAMPLE).map((n) => n.version)).toEqual(['1.6.0']);
  });

  it('shortens a .0 version', () => {
    expect(shortVersion('1.5.0')).toBe('1.5');
    expect(shortVersion('1.5.1')).toBe('1.5.1');
    expect(shortVersion('1.10.0')).toBe('1.10');
  });
});
