<script lang="ts">
  // The rest of what a setting does (2.5, Settings' shorter text): its one line shows, and this
  // small "More" opens the longer word under it. It sits beside the setting's label, never in it,
  // so opening it never flips the switch.
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import type { Snippet } from 'svelte';

  interface Props {
    /** The setting it tells more about, for screen readers. */
    about: string;
    children: Snippet;
  }
  let { about, children }: Props = $props();
  const id = $props.id();
  let open = $state(false);
</script>

<button
  type="button"
  class="more-toggle"
  aria-expanded={open}
  aria-controls={id}
  aria-label="{open ? 'Less' : 'More'} about {about}"
  onclick={() => (open = !open)}
>
  {open ? 'Less' : 'More'}<ChevronDown size={13} aria-hidden="true" />
</button>
<p {id} class="more-text" hidden={!open}>{@render children()}</p>

<style>
  .more-toggle {
    display: inline-flex;
    align-items: center;
    align-self: start;
    gap: 0.15rem;
    margin-top: 0.1rem;
    padding: 0.1rem 0.45rem;
    border: 1px solid transparent;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: 0.74rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .more-toggle:hover {
    border-color: var(--line);
    color: var(--text);
  }
  .more-toggle[aria-expanded='true'] :global(svg) {
    transform: rotate(180deg);
  }
  .more-text {
    grid-column: 1 / -1;
    margin: -0.3rem 0 0.7rem var(--more-indent, 2.6rem);
    color: var(--muted);
    font-size: 0.76rem;
    line-height: 1.45;
  }
  .more-text[hidden] {
    display: none;
  }
</style>
