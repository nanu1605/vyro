<script lang="ts">
  // A Settings group in its own window (2.5): it grows out of the group's tile (or the middle of
  // the window, opened from elsewhere) over the dimmed page, and shrinks back into it on Escape,
  // × or a click beside it. Under reduced motion it only fades; with the profile's animation off,
  // or Windows' reduced motion, it just shows and goes.
  //
  // Smooth (the owner, 2026-11-15: "The settings pop out animation is very laggy"): only its
  // transform and opacity move, from where the tile is, read once at the click (a modal window is
  // centred, so nothing of the window is measured); what it shows is ready in its first frame; and
  // it frosts nothing behind it (app.css), which would be redone at every frame of the growing.
  import X from '@lucide/svelte/icons/x';
  import { onMount, type Snippet } from 'svelte';

  interface Props {
    title: string;
    /** Its state in a line, as its tile says it. */
    line?: string | null;
    /** Its width, in rem. */
    width?: number;
    /** Where it grows from: a point in the window, or null for its middle. */
    from: { x: number; y: number } | null;
    /** A row inside to show and ring: a setting found by search, or a part named by a link. */
    at?: string | null;
    /** Once it has gone, however it was closed. */
    onclosed: () => void;
    children: Snippet;
  }
  let { title, line = null, width = 36, from, at = null, onclosed, children }: Props = $props();

  let dialog = $state<HTMLDialogElement | undefined>();
  let body = $state<HTMLElement | undefined>();
  let closing = false;
  /** What had the focus when it opened (the group's tile), to give it back. */
  let before: HTMLElement | null = null;
  /** Taken down by the page (another group opened in its place): its closing says nothing. */
  let gone = false;
  /** From the tile's middle to the window's: where the growing starts and the shrinking ends. */
  const dx = $derived(from ? Math.round(from.x - innerWidth / 2) : 0);
  const dy = $derived(from ? Math.round(from.y - innerHeight / 2) : 0);

  onMount(() => {
    const d = dialog;
    if (!d) return;
    let ring: ReturnType<typeof setTimeout> | undefined;
    let raf = 0;
    // After the update that opened it: Ctrl K's palette, closing in that same update, would
    // otherwise hand the focus back behind this window.
    queueMicrotask(() => {
      if (gone || d.open) return;
      const had = document.activeElement;
      before = had instanceof HTMLElement && had !== document.body ? had : null;
      d.showModal();
      if (at) show(at, 0);
    });
    /** The row `id`: unfolded, in the middle of the window, ringed a moment. Rows that load a
     * little later (Remote's tabs) are waited for, a second at most. */
    function show(id: string, tries: number): void {
      const el = body?.querySelector<HTMLElement>(`#${CSS.escape(id)}`);
      if (!el) {
        if (tries < 60) raf = requestAnimationFrame(() => show(id, tries + 1));
        return;
      }
      for (let p = el.parentElement?.closest('details'); p; p = p.parentElement?.closest('details') ?? null) p.open = true;
      el.scrollIntoView({ block: 'center' });
      el.classList.add('ringed');
      ring = setTimeout(() => el.classList.remove('ringed'), 1800);
    }
    return () => {
      gone = true;
      clearTimeout(ring);
      cancelAnimationFrame(raf);
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
  aria-labelledby="settings-window-title"
  style:--w="{width}rem"
  style:--dx="{dx}px"
  style:--dy="{dy}px"
  oncancel={(e) => {
    // Escape: shrink first. Windows may close it outright on a second Escape; `close` covers that.
    e.preventDefault();
    shut();
  }}
  onclose={() => {
    closing = false;
    dialog?.classList.remove('closing');
    if (gone) return;
    onclosed();
    // The browser gives the focus back as it closes; if the tile was redrawn meanwhile, again.
    const back = before;
    if (back?.isConnected && document.activeElement === document.body) back.focus({ preventScroll: true });
  }}
  onclick={(e) => {
    // The window has no padding of its own: only a click on the dimmed page beside it lands here.
    if (e.target === dialog) shut();
  }}
>
  <header class="head">
    <div class="titles">
      <h2 id="settings-window-title">{title}</h2>
      {#if line}<p>{line}</p>{/if}
    </div>
    <button type="button" class="x" aria-label="Close {title}" title="Close (Esc)" onclick={shut}>
      <X size={16} aria-hidden="true" />
    </button>
  </header>
  <div class="body" bind:this={body}>
    {@render children()}
  </div>
</dialog>

<style>
  .section {
    width: min(var(--w), calc(100vw - 2rem));
    max-height: min(85vh, calc(100vh - 2rem));
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: calc(var(--radius) + 4px);
    /* Solid: a see-through theme's card would show the page under it. */
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 28px 64px -24px rgb(0 0 0 / 55%);
  }
  /* The header stays; what is under it scrolls. */
  .section[open] {
    display: flex;
    flex-direction: column;
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
  .head {
    display: flex;
    flex: none;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 1rem 1rem 0.75rem 1.25rem;
    border-bottom: 1px solid var(--line);
  }
  :global(:root[data-glass='on']) .head {
    border-bottom-color: var(--glass-line);
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 600;
  }
  .titles p {
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  /* A container: what is inside follows the window's width (the overlay's preview goes under its
     controls in a narrow one). */
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    container-type: inline-size;
  }
  /* Each part's card is a stretch of the window, parted by a line. */
  .body > :global(.card) {
    margin: 0;
    border: 0;
    border-radius: 0;
    background: none;
    box-shadow: none;
    padding: 1rem 1.25rem;
  }
  .body > :global(.card + .card) {
    border-top: 1px solid var(--line);
  }
  :global(:root[data-glass='on']) .body > :global(.card + .card) {
    border-top-color: var(--glass-line);
  }
  /* One part: the window's own header names it. */
  .body > :global(.card:only-child > header) {
    display: none;
  }
  .x {
    display: grid;
    flex: none;
    place-items: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .x:hover {
    color: var(--text);
  }
  @keyframes grow {
    from {
      opacity: 0;
      transform: translate(var(--dx), var(--dy)) scale(0.5);
    }
    55% {
      opacity: 1;
    }
  }
  @keyframes shrink {
    to {
      opacity: 0;
      transform: translate(var(--dx), var(--dy)) scale(0.5);
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
