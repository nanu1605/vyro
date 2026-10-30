// Scrolling to a card that may still move (2.0.3). A view's card grid places its cards again as
// their data arrives (card-layout.ts: Remote's, Updates'), so on a view's first showing a card
// scrolled to once, 150 ms after the view opened, could be elsewhere a moment later: Ctrl K's
// Units ended 555 px down the window rather than under the top bar, and Updates below it.

import { animator } from './anim';

/** What the person does that ends the following: their own scrolling, a click, a key. */
const OWN = ['wheel', 'touchstart', 'pointerdown', 'keydown'] as const;

/** Ends the card followed now, if any. */
let following: (() => void) | null = null;

/** The element's top in the page as laid out: without the transform of a card gliding there. */
function pageTop(el: HTMLElement): number {
  let y = 0;
  for (let n: Element | null = el; n instanceof HTMLElement; n = n.offsetParent) y += n.offsetTop;
  return y;
}

/**
 * Brings the element `id` to the top of the window (under the top bar, by its scroll margin) as
 * soon as it shows, and again whenever it moves or the page's length changes, for `ms`. The person
 * scrolling, clicking or typing ends it, as does the next one.
 */
export function reveal(id: string, ms = 3000): void {
  following?.();
  const until = performance.now() + ms;
  let raf = 0;
  let last = '';
  const stop = (): void => {
    cancelAnimationFrame(raf);
    for (const e of OWN) window.removeEventListener(e, stop, true);
    if (following === stop) following = null;
  };
  const step = (): void => {
    const el = document.getElementById(id);
    // offsetParent: it shows (its view is not hidden).
    if (el?.offsetParent) {
      const top = pageTop(el) - (parseFloat(getComputedStyle(el).scrollMarginTop) || 0);
      const at = `${Math.round(top)} ${document.documentElement.scrollHeight}`;
      if (at !== last) {
        last = at;
        window.scrollTo({ top, behavior: animator.snap ? 'instant' : 'smooth' });
      }
    }
    if (performance.now() < until) raf = requestAnimationFrame(step);
    else stop();
  };
  for (const e of OWN) window.addEventListener(e, stop, { capture: true, passive: true });
  following = stop;
  raf = requestAnimationFrame(step);
}
