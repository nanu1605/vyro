// The window's glass in the page (2.5): whether Windows' Acrylic is behind a see-through window,
// and whether Windows' own Transparency effects are on. Until the app says, opaque with
// Transparency effects on, as 2.4 was.

import { getWindowLook, onWindowLook, type WindowLook } from './ipc';

export const windowLook = $state<WindowLook>({ seeThrough: false, transparency: true });

let started = false;
export async function loadWindowLook(): Promise<void> {
  if (started) return;
  started = true;
  const set = (l: WindowLook) => Object.assign(windowLook, l);
  await onWindowLook(set);
  set(await getWindowLook());
}
