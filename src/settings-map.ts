// Settings as tiles (2.5, the owner, 2026-11-15): six groups, each a tile on the page with its
// state in a line, opening in its own window; and every setting in an index that Settings'
// search and Ctrl K look through, landing on the setting itself in its group's window.

import type { Edition, RemoteStatus } from './types';
import type { UpdateStatus } from './ipc';

export type GroupId = 'appearance' | 'gaming' | 'remote' | 'general' | 'updates' | 'pro';

export interface Group {
  id: GroupId;
  title: string;
  /** What it is for, said under its name when there is no state to say. */
  blurb: string;
  /** The sections it holds, in order (the ids Settings had before 2.5's tiles). */
  parts: readonly string[];
  /** Its window's width, in rem. */
  width: number;
}

export const GROUPS: readonly Group[] = [
  { id: 'appearance', title: 'Appearance', blurb: 'Theme, glass and accent', parts: ['appearance'], width: 40 },
  { id: 'gaming', title: 'Gaming and overlay', blurb: 'Profiles, frame rates and the overlay', parts: ['profile', 'gaming', 'overlay'], width: 56 },
  { id: 'remote', title: 'Phone and remote', blurb: 'The phone app and this network', parts: ['remote'], width: 48 },
  { id: 'general', title: 'Startup and units', blurb: 'Tray, start with Windows, °C or °F', parts: ['general', 'units', 'hidden-sensors', 'service'], width: 36 },
  { id: 'updates', title: 'Updates', blurb: 'New versions and what changed', parts: ['updates'], width: 36 },
  { id: 'pro', title: 'Clokrix Pro', blurb: 'Your licence', parts: ['pro'], width: 40 },
];

/** Each part's heading in its group's window. */
export const PART_TITLES: Readonly<Record<string, string>> = {
  appearance: 'Appearance',
  profile: 'Profile',
  gaming: 'Games',
  overlay: 'Overlay',
  remote: 'Remote',
  general: 'Startup',
  units: 'Units',
  'hidden-sensors': 'Hidden sensors',
  service: 'Sensor service',
  updates: 'Updates',
  pro: 'Clokrix Pro',
};

export const group = (id: GroupId): Group => GROUPS.find((g) => g.id === id)!;
const groupOfPart = (part: string): Group | undefined => GROUPS.find((g) => g.parts.includes(part));

/** Remote's tabs (its window). */
export type RemoteTab = 'phones' | 'network' | 'screen' | 'wake' | 'activity' | 'options';

export interface Setting {
  /** The id of its row in the window. */
  id: string;
  label: string;
  /** The section it is in. */
  part: string;
  /** Other words it is found by. */
  keys: string;
  /** Remote's tab it is on. */
  tab?: RemoteTab;
}

/** Every setting, in the order the windows show them. */
export const SETTINGS: readonly Setting[] = [
  { id: 'set-theme', label: 'Theme', part: 'appearance', keys: 'midnight amoled cyber glass minimal daylight dark light colours look' },
  { id: 'set-glass', label: 'Glass effects', part: 'appearance', keys: 'transparency transparent blur see-through acrylic frosted translucent' },
  { id: 'set-accent', label: 'Accent colour', part: 'appearance', keys: 'color custom wheel violet cyan blue magenta green highlight' },
  { id: 'set-open-on', label: 'Open on', part: 'appearance', keys: 'start tab dashboard last tab launch' },
  { id: 'set-cards', label: 'Put cards back', part: 'appearance', keys: 'card order drag reset arrange layout move' },
  { id: 'set-profile', label: 'Profile', part: 'profile', keys: 'poll rate update rate notifications animation motion desktop gaming work benchmark silent' },
  { id: 'set-frame-rate', label: 'Measure the frame rate of games', part: 'gaming', keys: 'fps presentmon 1% lows frame times capture fullscreen' },
  { id: 'set-game-profiles', label: 'Switch profiles for games', part: 'gaming', keys: 'game profile automation default window gpu busy' },
  { id: 'set-overlay', label: 'Show the overlay over the game', part: 'overlay', keys: 'overlay osd in-game fps counter hud on off' },
  { id: 'set-overlay-mode', label: 'Overlay mode', part: 'overlay', keys: 'minimal performance full graph' },
  { id: 'set-overlay-corner', label: 'Overlay corner', part: 'overlay', keys: 'position top bottom left right place' },
  { id: 'set-overlay-refresh', label: 'Overlay refresh', part: 'overlay', keys: 'update every second rate' },
  { id: 'set-overlay-size', label: 'Overlay size', part: 'overlay', keys: 'scale bigger smaller zoom' },
  { id: 'set-overlay-background', label: 'Overlay background', part: 'overlay', keys: 'opacity transparent dark' },
  { id: 'set-overlay-offset', label: 'Overlay distance from the edges', part: 'overlay', keys: 'offset in from the side edge margin' },
  { id: 'set-overlay-graph', label: 'Overlay frame-time graph', part: 'overlay', keys: 'frametime chart' },
  { id: 'set-overlay-compact', label: 'Compact overlay', part: 'overlay', keys: 'small tight' },
  { id: 'set-overlay-colour', label: 'Overlay colour', part: 'overlay', keys: 'color accent' },
  { id: 'set-overlay-readings', label: 'Overlay readings', part: 'overlay', keys: 'metrics choose cpu gpu temperature power vram clock' },
  { id: 'set-remote-anywhere', label: 'Reach from anywhere', part: 'remote', tab: 'phones', keys: 'relay phone app android iphone internet outside' },
  { id: 'set-remote-devices', label: 'Paired phones and devices', part: 'remote', tab: 'phones', keys: 'pair qr code sign out control screen' },
  { id: 'set-remote-lan', label: 'On this network', part: 'remote', tab: 'network', keys: 'local network lan browser companion wifi pair device' },
  { id: 'set-remote-screen', label: 'Screen sharing', part: 'remote', tab: 'screen', keys: 'remote desktop see the screen mouse keyboard' },
  { id: 'set-remote-lock', label: 'Lock this PC when a phone’s session ends', part: 'remote', tab: 'screen', keys: 'lock screen' },
  { id: 'set-remote-mic', label: 'The phone as this PC’s microphone', part: 'remote', tab: 'screen', keys: 'mic microphone vb-cable audio voice default' },
  { id: 'set-remote-wake', label: 'Wake from phone', part: 'remote', tab: 'wake', keys: 'wake on lan power on magic packet' },
  { id: 'set-remote-activity', label: 'Recent from your phones', part: 'remote', tab: 'activity', keys: 'history log actions' },
  { id: 'set-remote-port', label: 'Port on this network', part: 'remote', tab: 'options', keys: 'port' },
  { id: 'set-remote-length', label: 'How long a pairing lasts', part: 'remote', tab: 'options', keys: 'days session expire' },
  { id: 'set-remote-relay', label: 'Relay address', part: 'remote', tab: 'options', keys: 'custom relay url server own' },
  { id: 'set-remote-reset', label: 'Start over on the relay', part: 'remote', tab: 'options', keys: 'reset keys sign out' },
  { id: 'set-tray', label: 'Keep running in the tray', part: 'general', keys: 'tray background close minimise minimize' },
  { id: 'set-autostart', label: 'Start with Windows', part: 'general', keys: 'startup boot sign in login launch' },
  { id: 'set-units', label: 'Temperature unit', part: 'units', keys: 'celsius fahrenheit °c °f degrees' },
  { id: 'set-hidden', label: 'Hidden sensors', part: 'hidden-sensors', keys: 'hide show sensors' },
  { id: 'set-service', label: 'Sensor service', part: 'service', keys: 'service administrator privileged log programdata' },
  { id: 'set-check-updates', label: 'Check for updates daily', part: 'updates', keys: 'update automatic new version' },
  { id: 'set-update-now', label: 'Check for updates now', part: 'updates', keys: 'update install upgrade new version' },
  { id: 'set-whats-new', label: 'What’s new', part: 'updates', keys: 'release notes changelog version' },
  { id: 'set-pro', label: 'Clokrix Pro', part: 'pro', keys: 'licence license key buy purchase activate trial free move' },
];

/** Where a section or a setting is: its group, and what in the window to land on (none: the top). */
export interface Place {
  group: GroupId;
  at: string | null;
}

/** A Settings section (the ids before the tiles, which links and Ctrl K still use), a group or a
 * setting: where it is. */
export function placeOf(id: string): Place | null {
  const owner = groupOfPart(id);
  if (owner) return { group: owner.id, at: owner.parts[0] === id ? null : id };
  const s = SETTINGS.find((x) => x.id === id);
  if (s) return { group: groupOfPart(s.part)!.id, at: s.id };
  return null;
}

/** Remote's tab showing `at` (a setting's row), if it is on one. */
export const tabOf = (at: string | null): RemoteTab | undefined => SETTINGS.find((s) => s.id === at)?.tab;

/** Where a setting is, said: "Gaming and overlay › Overlay". */
export function whereOf(s: Setting): string {
  const g = groupOfPart(s.part)!;
  return g.parts.length > 1 ? `${g.title} › ${PART_TITLES[s.part]}` : g.title;
}

const words = (s: string) => s.toLowerCase().split(/[^\p{L}\p{N}°%]+/u).filter(Boolean);
const index = SETTINGS.map((s) => {
  const g = groupOfPart(s.part)!;
  return { s, label: words(s.label), keys: words(s.keys), place: words(`${PART_TITLES[s.part]} ${g.title}`) };
});

/** The settings that every word typed starts a word of (its name, the words it is found by, its
 * section and group), best first: its name before the words it goes by before where it is, then
 * the windows' order. */
export function searchSettings(query: string, limit = 12): Setting[] {
  const typed = words(query);
  if (!typed.length) return [];
  const hits: Array<{ s: Setting; score: number; i: number }> = [];
  index.forEach(({ s, label, keys, place }, i) => {
    let score = 0;
    for (const t of typed) {
      const starts = (ws: string[]) => ws.some((w) => w.startsWith(t));
      if (starts(label)) score += label.includes(t) ? 4 : 3;
      else if (starts(keys)) score += 2;
      else if (starts(place)) score += 1;
      else return;
    }
    hits.push({ s, score, i });
  });
  return hits
    .sort((a, b) => b.score - a.score || a.i - b.i)
    .slice(0, limit)
    .map((h) => h.s);
}

// --- What each tile says. -----------------------------------------------------------------------

/** Phone and remote: how this PC can be reached, and the phones paired. */
export function remoteLine(r: RemoteStatus | null): string | null {
  if (!r) return null;
  const phones = r.devices.filter((d) => d.anywhere).length;
  const others = r.devices.length - phones;
  const ways = [
    r.relay.enabled ? (r.relay.state === 'online' ? 'From anywhere' : 'From anywhere (connecting)') : null,
    r.settings.enabled ? 'On this network' : null,
  ].filter(Boolean);
  if (!ways.length) return 'Off';
  const paired = [phones ? `${phones} ${phones === 1 ? 'phone' : 'phones'}` : null, others ? `${others} ${others === 1 ? 'device' : 'devices'}` : null].filter(Boolean);
  return [ways.join(' and '), paired.length ? paired.join(', ') : 'nothing paired'].join(' · ');
}

/** Updates: this version, and whether there is a newer one. */
export function updatesLine(u: UpdateStatus | null): string | null {
  if (!u) return null;
  const v = `Version ${u.current}`;
  if (u.installing) return `${v} · installing`;
  if (u.available) return `${u.available.version} is available`;
  if (u.checking) return `${v} · checking…`;
  if (!u.configured) return `${v} · updated by hand`;
  return u.lastCheck ? `${v} · up to date` : v;
}

/** Clokrix Pro: Pro, the trial's days, or Free. */
export function proLine(e: Edition | null, days: number | null): string | null {
  if (!e) return null;
  if (e.tier === 'pro') return 'Pro';
  if (e.tier === 'trial') return `Trial · ${days ?? 0} ${days === 1 ? 'day' : 'days'} left`;
  return 'Free · Pro unlocks every view';
}

/** Startup and units: whether Clokrix starts with Windows, the unit, and hidden sensors. */
export function startupLine(autostart: boolean | null, unit: 'c' | 'f', hidden: number): string {
  return [
    autostart == null ? null : autostart ? 'Starts with Windows' : 'Starts when you open it',
    unit === 'f' ? '°F' : '°C',
    hidden ? `${hidden} hidden` : null,
  ]
    .filter(Boolean)
    .join(' · ');
}
