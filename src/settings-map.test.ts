import { describe, expect, it } from 'vitest';
import type { Edition, RemoteStatus } from './types';
import type { UpdateStatus } from './ipc';
import { PRO_SECTIONS } from './nav';
import { GROUPS, PART_TITLES, SETTINGS, placeOf, proLine, remoteLine, searchSettings, startupLine, tabOf, updatesLine, whereOf } from './settings-map';

/** The sections Settings had before its tiles: links, the palette and saved places use them. */
const OLD = ['appearance', 'profile', 'general', 'gaming', 'overlay', 'remote', 'units', 'hidden-sensors', 'service', 'updates', 'pro'];

describe("Settings' groups", () => {
  it('are six, holding every section once, each with a heading', () => {
    expect(GROUPS.map((g) => g.id)).toEqual(['appearance', 'gaming', 'remote', 'general', 'updates', 'pro']);
    const parts = GROUPS.flatMap((g) => g.parts);
    expect([...parts].sort()).toEqual([...OLD].sort());
    for (const p of parts) expect(PART_TITLES[p]).toBeTruthy();
  });

  it('take every old section to its group, landing on it unless it is first there', () => {
    expect(placeOf('appearance')).toEqual({ group: 'appearance', at: null });
    expect(placeOf('profile')).toEqual({ group: 'gaming', at: null });
    expect(placeOf('gaming')).toEqual({ group: 'gaming', at: 'gaming' });
    expect(placeOf('overlay')).toEqual({ group: 'gaming', at: 'overlay' });
    expect(placeOf('units')).toEqual({ group: 'general', at: 'units' });
    expect(placeOf('updates')).toEqual({ group: 'updates', at: null });
    expect(placeOf('pro')).toEqual({ group: 'pro', at: null });
    for (const id of OLD) expect(placeOf(id)).not.toBeNull();
    expect(placeOf('nothing')).toBeNull();
  });

  it('keep the Pro sections in the two Pro groups', () => {
    for (const id of PRO_SECTIONS) expect(['gaming', 'remote']).toContain(placeOf(id)?.group);
  });
});

describe('the settings index', () => {
  it('has one row per id, each in a known section, with a place', () => {
    const ids = SETTINGS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of SETTINGS) {
      expect(OLD).toContain(s.part);
      expect(placeOf(s.id)).toEqual({ group: placeOf(s.part)!.group, at: s.id });
      // Remote's rows are on one of its tabs; no other section has tabs.
      expect(!!s.tab).toBe(s.part === 'remote');
    }
    expect(tabOf('set-remote-wake')).toBe('wake');
    expect(tabOf('set-units')).toBeUndefined();
    expect(tabOf(null)).toBeUndefined();
  });

  it('says where a setting is', () => {
    expect(whereOf(SETTINGS.find((s) => s.id === 'set-overlay-corner')!)).toBe('Gaming and overlay › Overlay');
    expect(whereOf(SETTINGS.find((s) => s.id === 'set-theme')!)).toBe('Appearance');
  });
});

describe('searching settings', () => {
  const first = (q: string) => searchSettings(q)[0]?.id;

  it('finds a setting by its name or the words it goes by', () => {
    expect(first('fahrenheit')).toBe('set-units');
    expect(first('°F')).toBe('set-units');
    expect(first('start with windows')).toBe('set-autostart');
    expect(first('startup')).toBe('set-autostart');
    expect(first('overlay corner')).toBe('set-overlay-corner');
    expect(first('glass')).toBe('set-glass');
    expect(first('wake')).toBe('set-remote-wake');
    expect(first('licence')).toBe('set-pro');
  });

  it('needs every word typed to start a word', () => {
    expect(searchSettings('overlay zzz')).toEqual([]);
    expect(searchSettings('verlay')).toEqual([]);
    expect(searchSettings('  ')).toEqual([]);
  });

  it('puts names before other words, then keeps the windows’ order', () => {
    const ids = searchSettings('overlay').map((s) => s.id);
    expect(ids[0]).toBe('set-overlay');
    expect(ids.indexOf('set-overlay-mode')).toBeLessThan(ids.indexOf('set-overlay-corner'));
    expect(searchSettings('overlay', 3)).toHaveLength(3);
  });
});

describe("the tiles' lines", () => {
  const remote = (over: Partial<RemoteStatus> = {}, relay: Partial<RemoteStatus['relay']> = {}): RemoteStatus =>
    ({
      settings: { enabled: false, port: 8976, sessionDays: 30 },
      running: false,
      error: null,
      urls: [],
      devices: [],
      watching: 0,
      pairing: null,
      firewall: 'ask',
      relay: { enabled: false, state: 'off', ...relay },
      ...over,
    }) as unknown as RemoteStatus;
  const dev = (anywhere: boolean) => ({ id: String(Math.random()), name: 'x', anywhere }) as unknown as RemoteStatus['devices'][number];

  it('Phone and remote: off, or the ways in and what is paired', () => {
    expect(remoteLine(null)).toBeNull();
    expect(remoteLine(remote())).toBe('Off');
    expect(remoteLine(remote({ devices: [dev(true)] }, { enabled: true, state: 'online' }))).toBe('From anywhere · 1 phone');
    expect(remoteLine(remote({}, { enabled: true, state: 'connecting' }))).toBe('From anywhere (connecting) · nothing paired');
    expect(remoteLine(remote({ settings: { enabled: true } as RemoteStatus['settings'], devices: [dev(true), dev(true), dev(false)] }, { enabled: true, state: 'online' }))).toBe(
      'From anywhere and On this network · 2 phones, 1 device',
    );
  });

  it('Updates: the version and what is new', () => {
    const u = (over: Partial<UpdateStatus>): UpdateStatus => ({ configured: true, current: '2.5.0', checking: false, available: null, lastCheck: null, error: null, installing: false, ...over });
    expect(updatesLine(null)).toBeNull();
    expect(updatesLine(u({}))).toBe('Version 2.5.0');
    expect(updatesLine(u({ lastCheck: 1 }))).toBe('Version 2.5.0 · up to date');
    expect(updatesLine(u({ checking: true }))).toBe('Version 2.5.0 · checking…');
    expect(updatesLine(u({ available: { version: '2.6.0' } as UpdateStatus['available'] }))).toBe('2.6.0 is available');
    expect(updatesLine(u({ configured: false }))).toBe('Version 2.5.0 · updated by hand');
  });

  it('Clokrix Pro: Pro, the trial’s days, or Free', () => {
    const e = (tier: Edition['tier']) => ({ tier, trialEnds: null, key: null, note: null }) as Edition;
    expect(proLine(null, null)).toBeNull();
    expect(proLine(e('pro'), null)).toBe('Pro');
    expect(proLine(e('trial'), 1)).toBe('Trial · 1 day left');
    expect(proLine(e('trial'), 5)).toBe('Trial · 5 days left');
    expect(proLine(e('free'), null)).toBe('Free · Pro unlocks every view');
  });

  it('Startup and units: start, unit and hidden sensors', () => {
    expect(startupLine(true, 'c', 0)).toBe('Starts with Windows · °C');
    expect(startupLine(false, 'f', 2)).toBe('Starts when you open it · °F · 2 hidden');
    expect(startupLine(null, 'c', 0)).toBe('°C');
  });
});
