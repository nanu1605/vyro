// The app's page keys saved before the rename to Clokrix (2.2) begin `vyro.`; they move to
// `clokrix.` once, before anything reads them: main.ts imports this module first. A key already
// saved under the new name wins. The phone app has a new address after the rename (nothing saved
// there to move), and the companion page keeps only the look, which the PC sends again.
const OLD = 'vyro.';
const NEW = 'clokrix.';

/** Moves the old keys in `store`; the number moved. A key that cannot move stays as it was. */
export function renameKeys(store: Storage): number {
  const old: string[] = [];
  for (let i = 0; i < store.length; i++) {
    const key = store.key(i);
    if (key?.startsWith(OLD)) old.push(key);
  }
  let moved = 0;
  for (const key of old) {
    try {
      const value = store.getItem(key);
      const to = NEW + key.slice(OLD.length);
      if (value !== null && store.getItem(to) === null) {
        store.setItem(to, value);
        moved++;
      }
      store.removeItem(key);
    } catch {
      // Full or blocked: left for the next start.
    }
  }
  return moved;
}

try {
  renameKeys(localStorage);
} catch {
  // No storage here (blocked, or a test): nothing to move.
}
