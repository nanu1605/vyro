<script lang="ts">
  // What's new for the phone (1.7): once, the first time the app opens after it changed, up to
  // the open PC's version. The notes are the PC's (src/whatsnew.ts), those marked for the phone.
  // Loaded only when it shows.
  import type { Component } from 'svelte';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import Cpu from '@lucide/svelte/icons/cpu';
  import Fan from '@lucide/svelte/icons/fan';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Mic from '@lucide/svelte/icons/mic';
  import MonitorSmartphone from '@lucide/svelte/icons/monitor-smartphone';
  import Power from '@lucide/svelte/icons/power';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Sunrise from '@lucide/svelte/icons/sunrise';
  import { notesSince, shortVersion, type NoteIcon } from '../whatsnew';
  import Sheet from './ui/Sheet.svelte';

  interface Props {
    /** The newest notes this phone showed; null: never (the app from before 1.7). */
    from: string | null;
    /** The newest notes to show: the open PC's version, or this app's newest. */
    upTo: string;
    onclose: (shown: string) => void;
  }
  let { from, upTo, onclose }: Props = $props();

  const ICONS: Record<NoteIcon, Component> = {
    phone: Smartphone,
    end: CircleX,
    power: Power,
    wake: Sunrise,
    fan: Fan,
    gauge: Gauge,
    gpu: Cpu,
    screen: MonitorSmartphone,
    mic: Mic,
    sparkles: Sparkles,
  };

  const items = $derived(notesSince(from, upTo).flatMap((r) => r.items.filter((i) => i.phone)));
  let open = $state(true);
</script>

<Sheet {open} title="What’s new in Vyro {shortVersion(upTo)}" onclose={() => ((open = false), onclose(upTo))}>
  <ul class="items">
    {#each items as item, i (item.title)}
      {@const Icon = ICONS[item.icon]}
      <li class="rise" style:--i={i}>
        <span class="icon" aria-hidden="true"><Icon size={18} /></span>
        <span class="words"><b>{item.title}</b><span>{item.text}</span></span>
      </li>
    {/each}
  </ul>
  <div class="actions">
    <button type="button" class="got" onclick={() => ((open = false), onclose(upTo))}>Got it</button>
  </div>
</Sheet>

<style>
  .items {
    display: grid;
    gap: 0.85rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .items li {
    display: flex;
    gap: 0.75rem;
  }
  .icon {
    display: grid;
    flex: none;
    width: 2.1rem;
    height: 2.1rem;
    place-items: center;
    border-radius: 11px;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }
  .words {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
    font-size: 0.9rem;
    line-height: 1.4;
  }
  .words span {
    color: var(--muted);
  }
  .got {
    width: 100%;
  }
</style>
