// Automatic game profiles (P13) on the window's side: the saved games shown in the Gaming tab
// and, for the browser preview, the same decisions as `vyro_core::gaming::auto::Switcher`.

import type { Effective, GameAutomation, GameRule, GameSession, Profile } from './types';

/** Update rates a game can ask for, ms (`vyro_core::gaming::auto::POLL_CHOICES`). */
export const POLL_CHOICES = [500, 1000, 2000, 5000] as const;

export const DEFAULT_GAMES: GameAutomation = { enabled: true, defaultProfile: 'gaming', loadTrigger: false, rules: [] };

export function newRule(exe: string, name = exe): GameRule {
  return { exe: exe.toLowerCase(), name, profile: null, keep: false, fans: null, overlay: null, overlayMode: null, pollMs: null, ignore: false };
}

export function ruleFor(auto: GameAutomation, exe: string): GameRule | undefined {
  const key = exe.toLowerCase();
  return auto.rules.find((r) => r.exe === key);
}

/** Nothing of its own: the game behaves as an unsaved one. */
export function isPlain(r: GameRule): boolean {
  return r.profile == null && !r.keep && r.fans == null && r.overlay == null && r.overlayMode == null && r.pollMs == null && !r.ignore;
}

/** `auto` with `rule` added or replaced; a rule left with nothing of its own is dropped. */
export function withRule(auto: GameAutomation, rule: GameRule): GameAutomation {
  const rest = auto.rules.filter((r) => r.exe !== rule.exe.toLowerCase());
  return { ...auto, rules: isPlain(rule) ? rest : [...rest, { ...rule, exe: rule.exe.toLowerCase() }] };
}

export interface GameRow {
  exe: string;
  name: string;
  rule: GameRule;
  /** Has settings of its own. */
  saved: boolean;
}

/** Games to list: the one being played, saved ones, then recently played ones (each once). */
export function gameRows(auto: GameAutomation, sessions: GameSession[], playing: string | null, max = 20): GameRow[] {
  const rows: GameRow[] = [];
  const add = (exe: string, name: string) => {
    const key = exe.toLowerCase();
    if (rows.some((r) => r.exe === key)) return;
    const saved = ruleFor(auto, key);
    rows.push({ exe: key, name: saved?.name ?? name, rule: saved ?? newRule(key, name), saved: saved != null });
  };
  if (playing) add(playing, playing);
  for (const r of auto.rules) add(r.exe, r.name);
  for (const s of sessions) {
    if (rows.length >= max) break;
    add(s.app, s.app);
  }
  return rows;
}

/** What applies: mirrors `Switcher::effective` (browser preview). */
export function effectiveFor(
  base: Profile,
  auto: GameAutomation,
  game: string | null,
  suppressed: boolean,
  pollOf: (p: Profile) => number,
): Effective {
  const plain: Effective = {
    base, profile: base, fans: base, pollMs: pollOf(base), overlay: null, overlayMode: null, game: null, auto: false, suppressed: false,
    alert: null,
  };
  if (!game) return plain;
  const rule = ruleFor(auto, game);
  if (rule?.ignore) return plain;
  const playing: Effective = { ...plain, game, overlay: rule?.overlay ?? null, overlayMode: rule?.overlayMode ?? null };
  if (suppressed) return { ...playing, suppressed: true };
  if (!auto.enabled) return playing;
  const profile = rule?.keep ? base : (rule?.profile ?? auto.defaultProfile ?? base);
  const fans = rule?.fans ?? profile;
  const pollMs = rule?.pollMs ?? pollOf(profile);
  return { ...playing, profile, fans, pollMs, auto: profile !== base || fans !== base || pollMs !== pollOf(base) };
}

/** "0.5 s", "2 s". */
export const pollLabel = (ms: number): string => `${ms / 1000} s`;
