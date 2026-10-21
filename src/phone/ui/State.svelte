<script lang="ts">
  // A screen that has one thing to say (1.7): asleep, can't reach the relay, signed out, nothing
  // yet. An icon, a title, a sentence or two, and what can be done.
  import type { Snippet } from 'svelte';
  import type Glyph from '@lucide/svelte/icons/moon';

  interface Props {
    icon: typeof Glyph;
    title: string;
    tone?: 'ok' | 'warn' | 'hot' | 'muted';
    children?: Snippet;
    actions?: Snippet;
  }
  let { icon: Icon, title, tone = 'muted', children, actions }: Props = $props();
  const id = $props.id();
</script>

<section class="panel state rise" aria-labelledby="{id}-title">
  <span class="glyph {tone}" aria-hidden="true"><Icon size={28} /></span>
  <h1 id="{id}-title">{title}</h1>
  {@render children?.()}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</section>

<style>
  .state {
    display: grid;
    justify-items: center;
    padding: 1.6rem 1.2rem 1.4rem;
    text-align: center;
  }
  .state :global(p) {
    max-width: 30rem;
    margin: 0 0 0.4rem;
    color: var(--muted);
  }
  .state :global(.actions) {
    justify-content: center;
  }
  .glyph {
    display: grid;
    width: 3.6rem;
    height: 3.6rem;
    margin-bottom: 0.8rem;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--muted) 14%, transparent);
    color: var(--muted);
  }
  .glyph.ok {
    background: color-mix(in srgb, var(--ok) 16%, transparent);
    color: var(--ok);
  }
  .glyph.warn {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--warn);
  }
  .glyph.hot {
    background: color-mix(in srgb, var(--hot) 16%, transparent);
    color: var(--hot);
  }
  h1 {
    margin: 0 0 0.5rem;
    font-size: 1.2rem;
  }
</style>
