<script lang="ts">
  // A Settings section in its own window (2.5): it grows out of the section's name in the list
  // (or the middle of the window, opened from elsewhere) over the dimmed page, and shrinks back
  // into it on Escape, × or a click beside it. The section's own card is inside, so what it holds
  // and does is the same as in the page. Under reduced motion it only fades; with the profile's
  // animation off, or Windows' reduced motion, it just shows and goes.
  import X from '@lucide/svelte/icons/x';
  import { onMount, type Snippet } from 'svelte';

  interface Props {
    title: string;
    wide?: boolean;
    /** Where it grows from: a point in the window, or null for its middle. */
    from: { x: number; y: number } | null;
    /** Once it has gone, however it was closed. */
    onclosed: () => void;
    children: Snippet;
  }
  let { title, wide = false, from, onclosed, children }: Props = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  let closing = false;
  /** What had the focus when it opened (the section's name in the list), to give it back. */
  let before: HTMLElement | null = null;

  onMount(() => {
    const d = dialog;
    if (!d) return;
    let gone = false;
    // After the update that opened it: Ctrl K's palette, closing in that same update, would
    // otherwise hand the focus back behind this window.
    queueMicrotask(() => {
      if (gone || d.open) return;
      const had = document.activeElement;
      before = had instanceof HTMLElement && had !== document.body ? had : null;
      d.showModal();
      // From untransformed sizes: the growing has already started, and would skew a measured box.
      // A modal dialog sits in the middle of the window.
      const left = (innerWidth - d.offsetWidth) / 2;
      const top = (innerHeight - d.offsetHeight) / 2;
      d.style.transformOrigin = from ? `${Math.round(from.x - left)}px ${Math.round(from.y - top)}px` : 'center';
    });
    return () => {
      gone = true;
      if (d.open) d.close();
    };
  });

  /** Plays the shrinking, then closes; at once where nothing animates. */
  function shut(): void {
    const d = dialog;
    if (!d?.open || closing) return;
    closing = true;
    d.classList.add('closing');
    const finite = d.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity);
    const done = () => {
      if (d.open) d.close();
    };
    if (!finite.length) return done();
    // Also after a while: an animation that never finishes (the window hidden) must not keep it.
    const late = setTimeout(done, 600);
    void Promise.all(finite.map((a) => a.finished)).then(
      () => {
        clearTimeout(late);
        done();
      },
      () => {},
    );
  }
</script>

<dialog
  bind:this={dialog}
  class="section"
  class:wide
  aria-label={title}
  oncancel={(e) => {
    // Escape: shrink first. Windows may close it outright on a second Escape; `close` covers that.
    e.preventDefault();
    shut();
  }}
  onclose={() => {
    closing = false;
    onclosed();
    // The browser gives the focus back as it closes, but the card going back into the page can
    // move the list's names around while it settles (Remote's loads a moment later), and a name
    // moved loses the focus: it is given back again for a while, until the person does something.
    const back = before;
    if (!back) return;
    const until = performance.now() + 1500;
    let done = false;
    const stop = () => (done = true);
    addEventListener('pointerdown', stop, { once: true, capture: true });
    addEventListener('keydown', stop, { once: true, capture: true });
    const keep = () => {
      if (done || performance.now() > until || !back.isConnected) {
        removeEventListener('pointerdown', stop, { capture: true });
        removeEventListener('keydown', stop, { capture: true });
        return;
      }
      if (document.activeElement === document.body) back.focus({ preventScroll: true });
      requestAnimationFrame(keep);
    };
    requestAnimationFrame(keep);
  }}
  onclick={(e) => {
    // The dialog has no padding of its own: only a click on the dimmed page beside it lands here.
    if (e.target === dialog) shut();
  }}
>
  <div class="inner">
    <button type="button" class="x" aria-label="Close {title}" title="Close (Esc)" onclick={shut}>
      <X size={16} aria-hidden="true" />
    </button>
    {@render children()}
  </div>
</dialog>

<style>
  .section {
    width: min(34rem, calc(100vw - 2rem));
    max-height: min(85vh, calc(100vh - 2rem));
    padding: 0;
    overflow-y: auto;
    border: 0;
    border-radius: var(--radius);
    /* Solid: a see-through theme's card would show the page under it. */
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-lift);
    overscroll-behavior: contain;
  }
  .section.wide {
    width: min(62rem, calc(100vw - 2rem));
  }
  .section[open] {
    animation: grow var(--dur-3) var(--ease-out);
  }
  .section[open]::backdrop {
    background: rgb(0 0 0 / 55%);
    animation: dim var(--dur-3) ease-out;
  }
  .section:global(.closing) {
    animation: shrink var(--dur-2) cubic-bezier(0.4, 0, 1, 1) forwards;
  }
  .section:global(.closing)::backdrop {
    animation: undim var(--dur-2) ease-in forwards;
  }
  /* Calmer: a fade, no growing. */
  :global(:root[data-motion='reduced']) .section[open] {
    animation-name: dim;
  }
  :global(:root[data-motion='reduced']) .section:global(.closing) {
    animation-name: undim;
  }
  .inner {
    position: relative;
  }
  /* The card's own frame is the window's. */
  .inner > :global(.card) {
    margin: 0;
    border: 0;
    box-shadow: none;
  }
  .inner > :global(.card > header) {
    padding-right: 2.2rem;
  }
  .x {
    position: absolute;
    z-index: 1;
    top: 0.7rem;
    right: 0.7rem;
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    color: var(--text);
  }
  @keyframes grow {
    from {
      opacity: 0;
      transform: scale(0.6);
    }
  }
  @keyframes shrink {
    to {
      opacity: 0;
      transform: scale(0.6);
    }
  }
  @keyframes dim {
    from {
      opacity: 0;
    }
  }
  @keyframes undim {
    to {
      opacity: 0;
    }
  }
</style>
