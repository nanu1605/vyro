<script lang="ts">
  // Every PC this phone is paired with (1.7), from the PC's name in the bar: open another, go to
  // the list, pair one more, or sign this phone out of the one open. Loaded when first asked for.
  import Monitor from '@lucide/svelte/icons/monitor';
  import type { PairedPc } from './pcs';
  import Sheet from './ui/Sheet.svelte';

  interface Props {
    open: boolean;
    pcs: PairedPc[];
    current: string | null;
    onclose: () => void;
    onopen: (room: string) => void;
    onall: () => void;
    onpair: () => void;
    onleave: () => void;
  }
  let { open, pcs, current, onclose, onopen, onall, onpair, onleave }: Props = $props();
  const here = $derived(pcs.find((p) => p.room === current));
</script>

<Sheet {open} title="Your PCs" {onclose}>
  <ul class="rows list">
    {#each pcs as p (p.room)}
      <li>
        <button type="button" class="ghost pick" aria-current={p.room === current ? 'true' : undefined} onclick={() => (p.room === current ? onclose() : onopen(p.room))}>
          <Monitor size={18} aria-hidden="true" />
          <span class="name">{p.name}</span>
          {#if p.room === current}<span class="muted small">open</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
  <div class="actions">
    <button type="button" class="ghost" onclick={onall}>All PCs</button>
    <button type="button" class="ghost" onclick={onpair}>Pair another PC</button>
    {#if here}<button type="button" class="ghost" onclick={onleave}>Sign out of {here.name}</button>{/if}
  </div>
</Sheet>

<style>
  .list {
    margin-bottom: 0.25rem;
  }
  .list > li {
    padding: 0.35rem 0;
    border-top: 0;
  }
  .pick {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.6rem;
    text-align: left;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pick[aria-current='true'] {
    border-color: var(--accent);
  }
</style>
