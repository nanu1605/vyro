import { describe, expect, it } from 'vitest';
import { DEFAULT_GAMES, effectiveFor, gameRows, newRule, withRule } from './games';
import type { GameSession, Profile } from './types';

const POLL: Record<Profile, number> = { desktop: 1000, gaming: 2000, work: 2000, benchmark: 500, silent: 5000 };
const poll = (p: Profile) => POLL[p];

describe('effectiveFor (mirrors the Rust switcher)', () => {
  it('switches an unsaved game to Gaming and goes back without one', () => {
    const e = effectiveFor('desktop', DEFAULT_GAMES, 'game.exe', false, poll);
    expect([e.profile, e.fans, e.pollMs, e.auto]).toEqual(['gaming', 'gaming', 2000, true]);
    expect(effectiveFor('desktop', DEFAULT_GAMES, null, false, poll).profile).toBe('desktop');
  });

  it('follows a saved game, "Not a game" and a kept profile', () => {
    const auto = withRule(DEFAULT_GAMES, { ...newRule('Quiet.exe'), profile: 'silent', fans: 'benchmark', pollMs: 1000 });
    const e = effectiveFor('desktop', auto, 'quiet.exe', false, poll);
    expect([e.profile, e.fans, e.pollMs]).toEqual(['silent', 'benchmark', 1000]);
    const ignored = withRule(DEFAULT_GAMES, { ...newRule('editor.exe'), ignore: true });
    expect(effectiveFor('desktop', ignored, 'editor.exe', false, poll).game).toBeNull();
    const kept = effectiveFor('work', DEFAULT_GAMES, 'game.exe', true, poll);
    expect([kept.profile, kept.suppressed, kept.auto]).toEqual(['work', true, false]);
  });

  it('leaves the profile alone when asked to', () => {
    const auto = { ...DEFAULT_GAMES, defaultProfile: null };
    expect(effectiveFor('work', auto, 'game.exe', false, poll).profile).toBe('work');
  });
});

describe('saved games', () => {
  it('drops a rule left with nothing of its own', () => {
    const auto = withRule(DEFAULT_GAMES, { ...newRule('A.exe'), profile: 'work' });
    expect(auto.rules.map((r) => r.exe)).toEqual(['a.exe']);
    expect(withRule(auto, newRule('a.exe')).rules).toEqual([]);
  });

  it('lists the game in front, saved games, then recent ones, each once', () => {
    const auto = withRule(DEFAULT_GAMES, { ...newRule('Saved.exe'), keep: true });
    const sessions = [{ app: 'Recent.exe' }, { app: 'SAVED.EXE' }, { app: 'Recent.exe' }] as GameSession[];
    const rows = gameRows(auto, sessions, 'Front.exe');
    expect(rows.map((r) => [r.exe, r.saved])).toEqual([
      ['front.exe', false],
      ['saved.exe', true],
      ['recent.exe', false],
    ]);
    expect(rows[2]!.name).toBe('Recent.exe');
  });
});
