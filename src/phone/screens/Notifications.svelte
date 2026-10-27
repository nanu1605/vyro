<script lang="ts">
  // Notifications from this PC with the app closed (2.0, in More): on or off, what about, and a
  // test. Each PC is turned on separately; the subscription is this browser's, the choice the
  // PC keeps. Any paired phone may, as they only concern this phone.
  import { BLOCKED, current, DEFAULT_KINDS, subscribe, support, type PushKinds, type PushState } from '../push';
  import { toast } from '../ui/toast.svelte';
  import { errText, untilAnswered, type ScreenProps } from './ask';

  interface Props extends ScreenProps {
    /** Whether this phone is an iPhone, and runs as the app added to the Home Screen. */
    ios: boolean;
    standalone: boolean;
    /** The phone's wish for this PC changed: kept, so the PC is given the subscription again
     * should it lose it. */
    onwish: (on: boolean) => void;
  }
  let { ask, pc, ios, standalone, onwish }: Props = $props();

  let st = $state.raw<PushState | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const can = $derived(
    support(
      {
        push: 'PushManager' in window,
        worker: 'serviceWorker' in navigator,
        notification: typeof Notification !== 'undefined',
      },
      ios,
      standalone,
    ),
  );
  /** Asked when it shows, and again after each change (the person may have just said no). */
  let blocked = $state(typeof Notification !== 'undefined' && Notification.permission === 'denied');

  $effect(() =>
    untilAnswered(5000, () =>
      ask<PushState>('push.state').then((v) => {
        st = v;
        error = null;
      }),
    ),
  );

  const KINDS: Array<{ id: keyof PushKinds; label: string; hint: string }> = [
    { id: 'alerts', label: 'Alerts', hint: 'A rule fired: a hot CPU or GPU, a hardware problem, your own rules.' },
    { id: 'batteries', label: 'Low batteries', hint: 'A mouse, headset or controller needs charging.' },
    { id: 'games', label: 'Game summaries', hint: 'When a game ends: how long, frame rates, temperatures.' },
    { id: 'offline', label: 'Stopped answering', hint: 'Its connection ended without it shutting down or sleeping, and when it is back.' },
  ];

  async function change(work: () => Promise<PushState>, wish: boolean, said?: string): Promise<void> {
    busy = true;
    error = null;
    try {
      st = await work();
      onwish(wish);
      if (said) toast(said);
    } catch (e) {
      error = errText(e);
    } finally {
      busy = false;
      blocked = typeof Notification !== 'undefined' && Notification.permission === 'denied';
    }
  }

  const turnOn = () =>
    change(async () => ask<PushState>('push.set', { ...(await subscribe()), kinds: st?.on ? st.kinds : DEFAULT_KINDS }), true, `Notifications from ${pc} are on.`);
  const turnOff = () => change(() => ask<PushState>('push.off'), false, `Notifications from ${pc} are off.`);

  function setKind(id: keyof PushKinds, on: boolean): Promise<void> {
    const kinds = { ...(st?.kinds ?? DEFAULT_KINDS), [id]: on };
    return change(async () => ask<PushState>('push.set', { ...((await current()) ?? (await subscribe())), kinds }), true);
  }

  async function test(): Promise<void> {
    busy = true;
    try {
      await ask('push.test');
      toast('Sent: it should arrive in a few seconds.');
    } catch (e) {
      toast(errText(e), 'hot');
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" aria-labelledby="push-title">
  <h1 id="push-title">Notifications</h1>
  {#if can === 'homeScreen'}
    <p>On iPhone, notifications come only to Vyro on the Home Screen:</p>
    <ol class="list">
      <li>Tap <b>Share</b> at the bottom of Safari, then <b>Add to Home Screen</b>.</li>
      <li>Open <b>Vyro</b> from your Home Screen, and pair it with {pc} there.</li>
    </ol>
  {:else if can === 'no'}
    <p class="note warn" role="status">This browser can’t show notifications from a closed app. Chrome, Edge, Firefox and Samsung Internet can, and Safari on iPhone from the Home Screen.</p>
  {:else}
    <p class="muted">From {pc}, even with Vyro closed. Sealed for this phone on the PC: the relay passes them on and can’t read them.</p>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if blocked}
      <p class="note warn" role="status">{BLOCKED}</p>
    {/if}
    {#if !st}
      {#if !error}<div class="skeleton" role="status" aria-label="Reading…"><span></span><span></span></div>{/if}
    {:else if !st.relay}
      <p class="note warn" role="status">This relay can’t send notifications yet. Update the relay, or use Vyro’s own.</p>
    {:else}
      <div class="actions">
        {#if st.on}
          <button type="button" class="ghost" disabled={busy} onclick={() => void turnOff()}>Turn off</button>
          <button type="button" disabled={busy} onclick={() => void test()}>Send a test</button>
        {:else}
          <button type="button" disabled={busy || blocked} onclick={() => void turnOn()}>Turn on</button>
        {/if}
      </div>
      {#if st.on}
        <h2 id="push-kinds">About</h2>
        <ul class="kinds" aria-labelledby="push-kinds">
          {#each KINDS as k (k.id)}
            <li>
              <label class="check">
                <input type="checkbox" checked={st.kinds[k.id]} disabled={busy} onchange={(e) => void setKind(k.id, e.currentTarget.checked)} />
                <span><b>{k.label}</b><span class="muted small">{k.hint}</span></span>
              </label>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</section>

<style>
  h2 {
    margin: 1rem 0 0.25rem;
  }
  .kinds {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    min-height: 2.75rem;
    padding: 0.4rem 0;
  }
  .check input {
    width: 1.25rem;
    min-height: 0;
    height: 1.25rem;
    flex: none;
    margin-top: 0.15rem;
  }
  .check > span {
    display: grid;
    gap: 0.1rem;
  }
</style>
