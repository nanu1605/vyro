// Remote's state, kept while Settings shows (2.5's tiles): the Phone and remote tile says it, and
// its window opens on it at once, at full height, instead of loading it again as it grows.

import { getRemoteStatus, onRemote } from './ipc';
import type { RemoteStatus } from './types';

export const remoteStatus = $state<{ s: RemoteStatus | null }>({ s: null });

/** Reads it once, then again whenever a device pairs, signs out or watches, or the relay
 * connects; until the returned function is called. */
export function followRemote(): () => void {
  let stop: (() => void) | null = null;
  let gone = false;
  const load = () =>
    void getRemoteStatus()
      .then((s) => {
        if (!gone) remoteStatus.s = s;
      })
      .catch(() => {});
  load();
  void onRemote(load).then((f) => {
    if (gone) f();
    else stop = f;
  });
  return () => {
    gone = true;
    stop?.();
  };
}
