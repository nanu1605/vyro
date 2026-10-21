<script lang="ts" module>
  export type ChipTone = 'ok' | 'wait' | 'off';
</script>

<script lang="ts">
  // The phone app's bar (1.7): Vyro, the PC open now (a tap shows every PC), how the link to it
  // is, and what this phone may do. It stays at the top, over the content scrolling beneath.
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';

  interface Props {
    /** The PC open now; null on the home screens. */
    pc: string | null;
    chip: { text: string; tone: ChipTone } | null;
    /** What travels and what this phone may do. */
    sub: string;
    /** Shows every PC (the PC's name is then a button). */
    onpcs?: () => void;
    /** A screen with a way back (scanning, a section of More). */
    back?: { label: string; go: () => void } | null;
  }
  let { pc, chip, sub, onpcs, back = null }: Props = $props();
</script>

<header class="bar">
  <div class="row">
    {#if back}
      <button type="button" class="back" onclick={back.go}>
        <ChevronLeft size={20} aria-hidden="true" />
        <span>{back.label}</span>
      </button>
    {:else}
      <span class="brand">
        <img src="/icons/icon.svg" alt="" width="26" height="26" />
        <span class="word">VYRO</span>
      </span>
    {/if}
    {#if chip}
      <p class="chip {chip.tone}" role="status">{chip.text}</p>
    {/if}
  </div>
  {#if pc}
    {#if onpcs}
      <button type="button" class="pc" onclick={onpcs} aria-label="{pc}: your PCs">
        <span class="name">{pc}</span>
        <ChevronDown size={18} aria-hidden="true" />
      </button>
    {:else}
      <p class="pc"><span class="name">{pc}</span></p>
    {/if}
  {/if}
  <p class="sub"><ShieldCheck size={13} aria-hidden="true" /> <span>{sub}</span></p>
</header>

<style>
  .bar {
    position: sticky;
    top: 0;
    z-index: 20;
    padding: calc(env(safe-area-inset-top) + 0.6rem) max(1rem, env(safe-area-inset-right)) 0.6rem
      max(1rem, env(safe-area-inset-left));
    border-bottom: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    -webkit-backdrop-filter: blur(16px) saturate(150%);
    backdrop-filter: blur(16px) saturate(150%);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    min-height: 2.25rem;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .brand img {
    border-radius: 7px;
  }
  .word {
    color: var(--accent);
    font-weight: 800;
    letter-spacing: 0.16em;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.15rem;
    min-height: 2.25rem;
    margin-left: -0.5rem;
    padding: 0.25rem 0.6rem 0.25rem 0.35rem;
    border: 0;
    background: transparent;
    color: var(--accent);
    font-weight: 600;
  }
  .chip {
    display: inline-flex;
    flex: none;
    align-items: center;
    margin: 0;
    padding: 0.2rem 0.65rem;
    border: 1px solid color-mix(in srgb, var(--ok) 45%, var(--line));
    border-radius: 999px;
    background: color-mix(in srgb, var(--ok) 10%, transparent);
    color: var(--text);
    font-size: 0.78rem;
    font-weight: 600;
  }
  .chip::before {
    content: '';
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.4rem;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 25%, transparent);
  }
  .chip.wait {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--line));
    background: color-mix(in srgb, var(--warn) 10%, transparent);
  }
  .chip.wait::before {
    background: var(--warn);
    box-shadow: none;
    animation: pulse 1.2s ease-in-out infinite;
  }
  .chip.off {
    border-color: var(--line);
    background: transparent;
  }
  .chip.off::before {
    background: var(--muted);
    box-shadow: none;
  }
  .pc {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    max-width: 100%;
    min-height: 0;
    margin: 0.35rem 0 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font-size: 1.35rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    text-align: left;
  }
  button.pc:active {
    transform: none;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pc :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    margin: 0.2rem 0 0;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .sub :global(svg) {
    flex: none;
    color: var(--ok);
  }
</style>
