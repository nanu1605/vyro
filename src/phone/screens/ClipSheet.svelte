<script lang="ts">
  // A voice clip for the PC (1.7): recorded here, sent to the PC, and put on its clipboard as a
  // file, ready to paste into a chat (WhatsApp Web, say). The phone's microphone; up to 5 minutes
  // on a direct connection, 1 through the relay.
  import Mic from '@lucide/svelte/icons/mic';
  import Sheet from '../ui/Sheet.svelte';
  import { capture, join, level, micError, RATE, wav, type Capture } from './voice';

  interface Props {
    open: boolean;
    pc: string;
    /** The longest clip, seconds. */
    limit: number;
    /** Sends the WAV, until `stop` cancels it; gives the file's name on the PC. */
    send: (bytes: Uint8Array, progress: (done: number) => void, stop: AbortSignal) => Promise<string>;
    onpaste: () => void;
    onclose: () => void;
  }
  let { open, pc, limit, send, onpaste, onclose }: Props = $props();

  type Stage = 'ready' | 'recording' | 'sending' | 'sent' | 'failed';
  let stage = $state<Stage>('ready');
  let secs = $state(0);
  let loud = $state(0);
  let sent = $state(0);
  let why = $state<string | null>(null);
  let name = $state<string | null>(null);

  let mic: Capture | null = null;
  let blocks: Int16Array[] = [];
  let samples = 0;
  // The microphone takes a moment to start: a second tap meanwhile starts nothing more, and one
  // that starts after Cancel or the sheet closed stops at once (`take` moved on).
  let starting = false;
  let take = 0;
  /** The clip being sent: Cancel, or closing the sheet, stops it, so it does not land on the PC
   * later (or, through the relay, mix its pieces with the next clip's). */
  let sending: AbortController | null = null;

  async function record(): Promise<void> {
    if (starting || mic) return;
    starting = true;
    const mine = ++take;
    why = null;
    blocks = [];
    samples = 0;
    secs = 0;
    try {
      const started = await capture((pcm) => {
        blocks.push(pcm);
        samples += pcm.length;
        secs = samples / RATE;
        loud = level(pcm);
        if (secs >= limit) void finish();
      });
      if (mine !== take) {
        started.stop();
        return;
      }
      mic = started;
      stage = 'recording';
    } catch (e) {
      if (mine !== take) return;
      why = micError(e);
      stage = 'failed';
    } finally {
      starting = false;
    }
  }

  function halt(): void {
    take++;
    sending?.abort();
    sending = null;
    mic?.stop();
    mic = null;
    loud = 0;
  }

  async function finish(): Promise<void> {
    if (stage !== 'recording') return;
    halt();
    const pcm = join(blocks);
    blocks = [];
    if (pcm.length < RATE / 2) {
      why = 'That was too short to send.';
      stage = 'failed';
      return;
    }
    stage = 'sending';
    sent = 0;
    const going = new AbortController();
    sending = going;
    try {
      const kept = await send(wav(pcm), (d) => (sent = d), going.signal);
      if (going.signal.aborted) return;
      name = kept;
      stage = 'sent';
    } catch (e) {
      if (going.signal.aborted) return;
      why = e instanceof Error ? e.message : String(e);
      stage = 'failed';
    } finally {
      if (sending === going) sending = null;
    }
  }

  function close(): void {
    halt();
    stage = 'ready';
    onclose();
  }

  $effect(() => () => halt());

  const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;
</script>

<Sheet {open} title="A voice clip for {pc}" onclose={close}>
  {#if stage === 'ready' || stage === 'failed'}
    <p class="muted">
      Record here; {pc} puts it on its clipboard, so Paste in a chat (WhatsApp Web, say) attaches it. It goes as an audio
      message. Up to {Math.round(limit / 60)} min.
    </p>
    {#if why}<p class="error" role="alert">{why}</p>{/if}
    <div class="actions">
      <button type="button" class="big" onclick={() => void record()}><Mic size={20} aria-hidden="true" /> Record</button>
    </div>
  {:else if stage === 'recording'}
    <div class="live" role="status" aria-live="off">
      <span class="dot" aria-hidden="true"></span>
      <span class="clock">{clock(secs)}</span>
      <span class="meter" aria-hidden="true"><i style:width="{Math.round(loud * 100)}%"></i></span>
    </div>
    <p class="muted small">Recording. It stops by itself at {clock(limit)}.</p>
    <div class="actions">
      <button type="button" class="big" onclick={() => void finish()}>Send to {pc}</button>
      <button type="button" class="ghost" onclick={() => ((stage = 'ready'), halt())}>Cancel</button>
    </div>
  {:else if stage === 'sending'}
    <p role="status">Sending… {Math.round(sent * 100)} %</p>
    <div class="bar" aria-hidden="true"><i style:width="{sent * 100}%"></i></div>
    <div class="actions">
      <button type="button" class="ghost" onclick={() => ((stage = 'ready'), halt())}>Cancel</button>
    </div>
  {:else}
    <p role="status"><b>On {pc}'s clipboard</b>{name ? ` (${name})` : ''}. Paste it into the chat.</p>
    <div class="actions">
      <button type="button" class="big" onclick={() => (onpaste(), close())}>Paste on {pc}</button>
      <button type="button" class="ghost" onclick={() => (stage = 'ready')}>Record another</button>
    </div>
  {/if}
</Sheet>

<style>
  .big {
    display: inline-flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    min-height: 3rem;
  }
  .live {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin: 0.5rem 0;
  }
  .dot {
    width: 0.8rem;
    height: 0.8rem;
    border-radius: 50%;
    background: var(--hot);
    animation: pulse 1s ease-in-out infinite;
  }
  .clock {
    font-family: var(--font-num);
    font-size: 1.6rem;
    font-variant-numeric: tabular-nums;
  }
  .meter,
  .bar {
    display: block;
    flex: 1;
    height: 8px;
    overflow: hidden;
    border-radius: 4px;
    background: var(--gauge-track);
  }
  .meter i,
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 80ms linear;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
