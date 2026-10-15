<script lang="ts">
  // Settings → Remote (P26): turns the remote companion on and off, pairs devices with a QR code
  // or a typed code, and lists the paired devices with a way to sign each out at once.
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import { fmtDate, fmtDateTime } from '../format';
  import { cancelRemotePairing, getRemoteStatus, onRemote, setRemoteEnabled, setRemoteOptions, signOutRemote, startRemotePairing } from '../ipc';
  import type { RemoteStatus } from '../types';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  const MAX_DEVICES = 8;
  const DAYS = [1, 7, 30, 90];

  let status = $state.raw<RemoteStatus | null>(null);
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let now = $state(Date.now() / 1000);
  // Typed in Options, until saved.
  let portDraft = $state<number | null>(null);
  let daysDraft = $state<number | null>(null);

  async function run(f: () => Promise<RemoteStatus>): Promise<boolean> {
    busy = true;
    try {
      status = await f();
      error = null;
      return true;
    } catch (e) {
      error = String(e).replace(/^Error: /, '');
      return false;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (!active) return;
    untrack(() => void run(getRemoteStatus));
    let stop: (() => void) | null = null;
    let gone = false;
    // A device paired, signed out or opened its view: show it.
    void onRemote(() => {
      void getRemoteStatus()
        .then((s) => (status = s))
        .catch(() => {});
    }).then((f) => {
      if (gone) f();
      else stop = f;
    });
    return () => {
      gone = true;
      stop?.();
    };
  });

  // The code's countdown; an expired code goes away.
  $effect(() => {
    const expires = status?.pairing?.expires;
    if (expires == null) return;
    now = Date.now() / 1000;
    const t = setInterval(() => {
      now = Date.now() / 1000;
      if (now >= expires) void run(getRemoteStatus);
    }, 1000);
    return () => clearInterval(t);
  });

  const left = $derived.by(() => {
    const s = Math.max(0, Math.round((status?.pairing?.expires ?? 0) - now));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  });
  const subtitle = $derived(
    !status
      ? null
      : !status.settings.enabled
        ? 'Off'
        : !status.running
          ? 'Not running'
          : status.watching
            ? `On · ${status.watching} watching`
            : 'On',
  );
  const FIREWALL: Record<string, string> = {
    ask: 'Windows will ask the first time a device connects: allow Private networks only.',
    allowed: 'Windows Firewall lets devices on this network reach Vyro.',
    blocked:
      'Windows Firewall blocks Vyro on this network. Allow it in Windows Security → Firewall & network protection → Allow an app through firewall (Private only).',
    off: 'Windows Firewall is off for this network.',
  };

  async function turnOn(): Promise<void> {
    if (await run(() => setRemoteEnabled(true))) confirming = false;
  }

  async function saveOptions(): Promise<void> {
    if (!status) return;
    const port = portDraft ?? status.settings.port;
    const days = daysDraft ?? status.settings.sessionDays;
    if (await run(() => setRemoteOptions(port, days))) {
      portDraft = null;
      daysDraft = null;
    }
  }
</script>

<Card title="Remote" {subtitle} id="remote">
  {#if !status}
    <p class="note">Loading…</p>
  {:else if !status.settings.enabled}
    <p>See this PC’s readings on a phone or another computer on the same network.</p>
    {#if !confirming}
      <div class="actions">
        <button type="button" onclick={() => (confirming = true)}>Turn on…</button>
      </div>
      <p class="note">Off: nothing listens on the network.</p>
    {:else}
      <div class="confirm" role="group" aria-labelledby="remote-before">
        <p id="remote-before"><b>Before you turn it on</b></p>
        <ul>
          <li>Only devices on this local network can connect, and only after you pair them with a code shown here.</li>
          <li>The page is read-only: nothing on it can change this PC.</li>
          <li>
            The connection is not encrypted: someone on the same network could watch the readings go by. Use it at home,
            not on public Wi-Fi.
          </li>
          <li>Windows will ask whether Vyro may accept connections. Allow <b>Private networks</b> only.</li>
          <li>It answers while Vyro runs, in the tray too.</li>
        </ul>
        <div class="actions">
          <button type="button" class="primary" disabled={busy} onclick={() => void turnOn()}>Turn on</button>
          <button type="button" onclick={() => (confirming = false)}>Cancel</button>
        </div>
      </div>
    {/if}
  {:else}
    {#if status.error}
      <p class="err" role="alert">Not running: {status.error}. Pick another port in Options.</p>
    {:else if status.running}
      {#if status.urls.length}
        <p>
          Open on the other device:
          {#each status.urls as u, i (u)}<code>{u}</code>{i < status.urls.length - 1 ? ' or ' : ''}{/each}
        </p>
      {:else}
        <p>This PC has no local network address right now.</p>
      {/if}
      <p class="note">Local network only · not encrypted · read-only.</p>
      {#if FIREWALL[status.firewall]}
        <p class="note" class:warn={status.firewall === 'blocked'}>{FIREWALL[status.firewall]}</p>
      {/if}
    {/if}

    {#if status.pairing}
      <div class="pairing" role="group" aria-labelledby="remote-code">
        {#if status.pairing.qr}
          <svg class="qr" viewBox="0 0 {status.pairing.qrSize} {status.pairing.qrSize}" role="img" aria-label="QR code that opens the companion with this code">
            <rect width={status.pairing.qrSize} height={status.pairing.qrSize} fill="#fff" />
            <path d={status.pairing.qr} fill="#000" />
          </svg>
        {/if}
        <div>
          <p class="code" id="remote-code"><span class="sr-only">Pairing code </span>{status.pairing.code}</p>
          <p class="note">
            Scan it with the phone’s camera, or open the address above and type the code. It works for one device and
            ends in <span role="timer">{left}</span>{status.pairing.triesLeft < 5 ? ` · ${status.pairing.triesLeft} tries left` : ''}.
          </p>
          <div class="actions">
            <button type="button" disabled={busy} onclick={() => void run(cancelRemotePairing)}>Cancel</button>
          </div>
        </div>
      </div>
    {:else if status.running}
      <div class="actions">
        <button type="button" class="primary" disabled={busy || status.devices.length >= MAX_DEVICES} onclick={() => void run(startRemotePairing)}>
          Pair a device
        </button>
      </div>
      {#if status.devices.length >= MAX_DEVICES}
        <p class="note">{MAX_DEVICES} devices are paired; sign one out to pair another.</p>
      {/if}
    {/if}

    <h3>Paired devices</h3>
    {#if status.devices.length}
      <ul class="devices">
        {#each status.devices as d (d.id)}
          <li>
            <div>
              <span class="name">{d.name}</span>
              {#if d.watching}<span class="watching">watching now</span>{/if}
              <small>
                Paired {fmtDate(d.paired)} · until {fmtDate(d.expires)}{d.lastSeen ? ` · last seen ${fmtDateTime(d.lastSeen)}` : ''}{d.lastAddress
                  ? ` from ${d.lastAddress}`
                  : ''}
              </small>
            </div>
            <button type="button" disabled={busy} aria-label="Sign out {d.name}" onclick={() => void run(() => signOutRemote(d.id))}>
              Sign out
            </button>
          </li>
        {/each}
      </ul>
      {#if status.devices.length > 1}
        <div class="actions">
          <button type="button" disabled={busy} onclick={() => void run(() => signOutRemote(null))}>Sign out all devices</button>
        </div>
      {/if}
    {:else}
      <p class="note">None yet.</p>
    {/if}

    <details>
      <summary>Options</summary>
      <div class="options">
        <label>
          <span>Port</span>
          <input
            type="number"
            min="1024"
            max="65535"
            value={portDraft ?? status.settings.port}
            oninput={(e) => (portDraft = Number(e.currentTarget.value))}
          />
        </label>
        <label>
          <span>A pairing lasts</span>
          <select value={daysDraft ?? status.settings.sessionDays} onchange={(e) => (daysDraft = Number(e.currentTarget.value))}>
            {#each DAYS as d (d)}<option value={d}>{d === 1 ? '1 day' : `${d} days`}</option>{/each}
          </select>
        </label>
        <button type="button" disabled={busy || (portDraft == null && daysDraft == null)} onclick={() => void saveOptions()}>Save</button>
      </div>
      <p class="note">A new length applies to devices paired from now on. A new port signs nobody out; open the new address.</p>
    </details>

    <div class="actions">
      <button type="button" disabled={busy} onclick={() => void run(() => setRemoteEnabled(false))}>Turn off</button>
    </div>
    <p class="note">
      Turning it off closes the port and every connection at once. Devices stay paired until their pairing ends or you sign
      them out.
    </p>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</Card>

<style>
  p {
    margin: 0 0 0.5rem;
    font-size: 0.85rem;
  }
  .note {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .note.warn {
    color: var(--text);
    padding-left: 0.5rem;
    border-left: 3px solid var(--warn);
  }
  .err {
    color: var(--hot);
    font-size: 0.82rem;
  }
  code {
    font-size: 0.8rem;
    overflow-wrap: anywhere;
    user-select: all;
  }
  h3 {
    margin: 0.9rem 0 0.4rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .confirm {
    padding: 0.6rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--panel);
  }
  .confirm ul {
    margin: 0 0 0.6rem;
    padding-left: 1.1rem;
    font-size: 0.82rem;
    line-height: 1.45;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }
  button {
    padding: 0.35rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  button.primary {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--bg));
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .pairing {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem 1rem;
    margin: 0.5rem 0;
  }
  .pairing > div {
    flex: 1 1 12rem;
    min-width: 0;
  }
  .qr {
    width: 11rem;
    height: 11rem;
    border-radius: 6px;
    shape-rendering: crispEdges;
  }
  .code {
    margin: 0 0 0.3rem;
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 1.6rem;
    font-weight: 700;
    letter-spacing: 0.12em;
  }
  .devices {
    margin: 0 0 0.5rem;
    padding: 0;
    list-style: none;
  }
  .devices li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.4rem 0;
    border-top: 1px solid var(--line);
    font-size: 0.82rem;
  }
  .devices li:first-child {
    border-top: 0;
  }
  .devices li > div {
    min-width: 0;
  }
  .devices button {
    flex: none;
    white-space: nowrap;
  }
  .devices small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
    overflow-wrap: anywhere;
  }
  .name {
    font-weight: 600;
  }
  .watching {
    margin-left: 0.4rem;
    padding: 0 0.4rem;
    border: 1px solid color-mix(in srgb, var(--ok) 55%, var(--line));
    border-radius: 999px;
    font-size: 0.7rem;
  }
  details {
    margin: 0.6rem 0;
    font-size: 0.82rem;
  }
  summary {
    cursor: pointer;
    color: var(--muted);
  }
  .options {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 0.5rem 0.75rem;
    margin: 0.5rem 0;
  }
  .options label {
    display: grid;
    gap: 0.2rem;
  }
  .options input,
  .options select {
    padding: 0.3rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .options input {
    width: 6.5rem;
  }
</style>
