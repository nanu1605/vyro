// What the phone's screens (1.5) share: asking the PC, and whether this phone may.

/** A request to the PC through the relay (`PcLink.request`). */
export type Ask = <T>(method: string, params?: object) => Promise<T>;

export interface ScreenProps {
  ask: Ask;
  /** The PC lets this phone make the request (control is the owner's choice, on the PC). */
  can: (method: string) => boolean;
  /** The PC's name. */
  pc: string;
}

export function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Calls `load` now and every `ms` while the screen shows; gives a function that stops it. A
 * load still under way (a slow link) is waited out, not asked again on top of. */
export function every(ms: number, load: () => Promise<unknown>): () => void {
  let under = false;
  const go = () => {
    if (under) return;
    under = true;
    const done = () => (under = false);
    load().then(done, done);
  };
  go();
  const t = setInterval(() => {
    if (!document.hidden) go();
  }, ms);
  return () => clearInterval(t);
}

/** Calls `load` now, and again `ms` after each time it fails, until it succeeds; gives a function
 * that stops it. For what is asked once: the link may be down as the screen opens. */
export function untilAnswered(ms: number, load: () => Promise<unknown>): () => void {
  let t: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const go = (): void => {
    load().catch(() => {
      if (!stopped) t = setTimeout(go, ms);
    });
  };
  go();
  return () => {
    stopped = true;
    clearTimeout(t);
  };
}
