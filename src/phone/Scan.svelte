<script lang="ts">
  // Scans the pairing code shown in Vyro on the PC (1.4). Loaded only when opened. Uses the
  // browser's own QR reader where there is one (Android), else jsQR on camera frames (iPhone).
  import { onMount } from 'svelte';

  interface Props {
    /** The text of a QR code (the caller checks it is a pairing link). */
    onlink: (text: string) => void;
    oncancel: () => void;
  }
  let { onlink, oncancel }: Props = $props();

  interface Detector {
    detect(source: HTMLVideoElement): Promise<Array<{ rawValue: string }>>;
  }
  type DetectorClass = { new (o: { formats: string[] }): Detector; getSupportedFormats(): Promise<string[]> };

  let video: HTMLVideoElement | undefined = $state();
  let error = $state<string | null>(null);
  let ready = $state(false);

  async function reader(): Promise<(v: HTMLVideoElement) => Promise<string | null>> {
    const Native = (window as Window & { BarcodeDetector?: DetectorClass }).BarcodeDetector;
    if (Native) {
      try {
        if ((await Native.getSupportedFormats()).includes('qr_code')) {
          const d = new Native({ formats: ['qr_code'] });
          return async (v) => (await d.detect(v))[0]?.rawValue ?? null;
        }
      } catch {
        /* fall through to jsQR */
      }
    }
    const { default: jsQR } = await import('jsqr');
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    return async (v) => {
      const w = v.videoWidth;
      const h = v.videoHeight;
      if (!ctx || !w || !h) return null;
      const scale = Math.min(1, 720 / Math.max(w, h));
      canvas.width = Math.round(w * scale);
      canvas.height = Math.round(h * scale);
      ctx.drawImage(v, 0, 0, canvas.width, canvas.height);
      const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
      return jsQR(img.data, img.width, img.height, { inversionAttempts: 'dontInvert' })?.data ?? null;
    };
  }

  onMount(() => {
    let stream: MediaStream | null = null;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    void (async () => {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment' }, audio: false });
      } catch {
        error = 'The camera is not available here. Allow it in the browser’s settings, or paste the pairing link instead.';
        return;
      }
      if (stopped || !video) {
        stream.getTracks().forEach((t) => t.stop());
        return;
      }
      video.srcObject = stream;
      await video.play().catch(() => {});
      ready = true;
      const read = await reader();
      const look = async () => {
        if (stopped || !video) return;
        const text = await read(video).catch(() => null);
        if (stopped) return;
        // A code the app takes closes this view; one it does not (another QR code) is said
        // above, and looking goes on, after a pause so that code is not read over and over.
        if (text) onlink(text);
        timer = setTimeout(() => void look(), text ? 2000 : 200);
      };
      void look();
    })();
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      stream?.getTracks().forEach((t) => t.stop());
    };
  });
</script>

<section class="panel scan" aria-labelledby="scan-title">
  <h1 id="scan-title">Scan the code in Vyro</h1>
  <p>On the PC: Settings → Remote → From anywhere → <b>Pair a phone</b>.</p>
  <div class="frame">
    <!-- svelte-ignore a11y_media_has_caption -->
    <video bind:this={video} playsinline muted aria-label="Camera view"></video>
    {#if !ready && !error}<p class="muted overlay" role="status">Starting the camera…</p>{/if}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="actions"><button type="button" class="ghost" onclick={oncancel}>Cancel</button></div>
</section>

<style>
  .frame {
    position: relative;
    overflow: hidden;
    max-width: 26rem;
    aspect-ratio: 1;
    margin: 0.75rem 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: #000;
  }
  video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .overlay {
    position: absolute;
    inset: auto 0 0;
    margin: 0;
    padding: 0.6rem;
    background: rgb(0 0 0 / 55%);
    color: #fff;
    text-align: center;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
  }
</style>
