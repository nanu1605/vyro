// Update state (P11), shared by the dashboard notice and Settings → Updates.

import { checkForUpdates, getUpdateStatus, installUpdate, onUpdateStatus, type UpdateStatus } from './ipc';

export const update = $state<{ status: UpdateStatus | null }>({ status: null });

let listening = false;

/** Loads the status and follows later changes (automatic checks). */
export async function loadUpdates(): Promise<void> {
  if (!listening) {
    listening = true;
    void onUpdateStatus((s) => (update.status = s));
  }
  update.status = await getUpdateStatus();
}

export async function checkNow(): Promise<void> {
  update.status = await checkForUpdates();
}

/** On success Vyro exits and the installer takes over; errors arrive in the status too. */
export async function install(): Promise<void> {
  await installUpdate();
}
