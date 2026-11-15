// The Settings group shown in its own window (2.5): a group opens over the page when its tile is
// clicked, and the same way from Settings' search, Ctrl K and the links that name a section or
// setting (the update and Remote chips, the Dashboard's "Change profile", Gaming's "Turn it on").

import { placeOf, type GroupId } from './settings-map';

export const settingsDialog = $state<{
  id: GroupId | null;
  /** The row to land on, if any. */
  at: string | null;
  /** Where its window grows from (a tile, a search result); none: the middle. */
  from: { x: number; y: number } | null;
  /** Counts the openings: opening again, even the same group, starts afresh. */
  n: number;
}>({ id: null, at: null, from: null, n: 0 });

/** Opens the group, section (the ids from before the tiles) or setting `id` in its window;
 * Settings must be the view shown. */
export function openSettingsSection(id: string, from: { x: number; y: number } | null = null): void {
  const p = placeOf(id);
  if (!p) return;
  Object.assign(settingsDialog, { id: p.group, at: p.at, from, n: settingsDialog.n + 1 });
}

/** Opens a group at its top (its tile). */
export function openSettingsGroup(id: GroupId, from: { x: number; y: number } | null = null): void {
  Object.assign(settingsDialog, { id, at: null, from, n: settingsDialog.n + 1 });
}

export function closeSettingsSection(): void {
  settingsDialog.id = null;
  settingsDialog.at = null;
}
