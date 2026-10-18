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

/** Calls `load` now and every `ms` while the screen shows; gives a function that stops it. */
export function every(ms: number, load: () => void): () => void {
  load();
  const t = setInterval(() => {
    if (!document.hidden) load();
  }, ms);
  return () => clearInterval(t);
}
