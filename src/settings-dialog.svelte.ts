// The Settings section shown in its own window (2.5): a section opens over the page when its name
// is clicked in Settings' list, and the same way from Ctrl K and the links that name a section
// (the update and Remote chips, the Dashboard's "Change profile", Gaming's "Turn it on").

export const settingsDialog = $state<{ id: string | null }>({ id: null });

/** Opens the Settings section `id` in its window; Settings must be the view shown. */
export function openSettingsSection(id: string): void {
  settingsDialog.id = id;
}

export function closeSettingsSection(): void {
  settingsDialog.id = null;
}
