<script lang="ts">
  // The game overlay window: a click-through, transparent window over the game. The window is
  // sized generously (src-tauri/src/overlay.rs); the plate sits in the window's corner that
  // matches the screen corner, so the spare room never shows.
  import { onMount } from 'svelte';
  import OverlayPlate from './components/OverlayPlate.svelte';
  import { onOverlay, type OverlayData } from './ipc';
  import { loadPrefs } from './prefs.svelte';

  let data = $state.raw<OverlayData | null>(null);

  onMount(() => {
    void loadPrefs().catch(() => {});
    let stop: (() => void) | undefined;
    let disposed = false;
    void onOverlay((d) => (data = d)).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });
</script>

{#if data}
  <div class="corner {data.corner}">
    <OverlayPlate {data} />
  </div>
{/if}

<style>
  .corner {
    position: fixed;
    inset: 0;
    display: flex;
  }
  .topLeft {
    align-items: flex-start;
    justify-content: flex-start;
  }
  .topRight {
    align-items: flex-start;
    justify-content: flex-end;
  }
  .bottomLeft {
    align-items: flex-end;
    justify-content: flex-start;
  }
  .bottomRight {
    align-items: flex-end;
    justify-content: flex-end;
  }
</style>
