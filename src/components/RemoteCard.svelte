<script lang="ts">
  // Settings → Remote (P26, 1.4): reach this PC from anywhere through the Vyro relay, or from
  // this network through the local companion; pairs phones and devices with a QR code (or a
  // typed code on this network), and lists them with a way to sign each out at once.
  import { untrack } from 'svelte';
  import Card from './Card.svelte';
  import { fmtDate, fmtDateTime } from '../format';
  import {
    cancelRelayPairing,
    cancelRemotePairing,
    getPhoneActions,
    getRemoteStatus,
    getWakeReadiness,
    onPhoneActions,
    onRemote,
    resetRelay,
    setPhoneControl,
    setRelayEnabled,
    setRelayUrl,
    setRemoteEnabled,
    setRemoteOptions,
    setWakeFromPhone,
    signOutRemote,
    startRelayPairing,
    startRemotePairing,
  } from '../ipc';
  import type { PhoneAction, RemoteStatus, WakeReadiness } from '../types';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  const MAX_DEVICES = 8;
  const DAYS = [1, 7, 30, 90];

  let status = $state.raw<RemoteStatus | null>(null);
  let confirming = $state(false);
  let confirmingRelay = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let copied = $state(false);
  let now = $state(Date.now() / 1000);
  // Typed in Options, until saved.
  let portDraft = $state<number | null>(null);
  let daysDraft = $state<number | null>(null);
  let relayDraft = $state<string | null>(null);
  /** The next phone paired may change the PC (1.5); off unless the owner says so. */
  let pairControl = $state(false);
  let actions = $state.raw<PhoneAction[]>([]);
  let readiness = $state.raw<WakeReadiness | null>(null);

  function loadActions(): void {
    getPhoneActions()
      .then((a) => (actions = a))
      .catch(() => {});
  }
  $effect(() => {
    if (!active) return;
    untrack(loadActions);
    let stop: (() => void) | undefined;
    let disposed = false;
    void onPhoneActions(loadActions).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });
  // Read when "Wake from phone" is turned on or Settings shows, not on every status reload (it
  // reads the adapters and Wi-Fi).
  const wakeOn = $derived(!!status?.relay.wake);
  $effect(() => {
    if (!active || !wakeOn) return;
    untrack(() =>
      getWakeReadiness()
        .then((r) => (readiness = r))
        .catch(() => {}),
    );
  });

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
    // A device paired, signed out or opened its view; the relay connected: show it.
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

  // The codes' countdowns; an expired code goes away.
  $effect(() => {
    const ends = [status?.pairing?.expires, status?.relay.pairing?.expires].filter((e): e is number => e != null);
    if (!ends.length) return;
    now = Date.now() / 1000;
    const t = setInterval(() => {
      now = Date.now() / 1000;
      if (ends.some((e) => now >= e)) void run(getRemoteStatus);
    }, 1000);
    return () => clearInterval(t);
  });

  const countdown = (expires: number | undefined) => {
    const s = Math.max(0, Math.round((expires ?? 0) - now));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  };
  const left = $derived(countdown(status?.pairing?.expires));
  const relayLeft = $derived(countdown(status?.relay.pairing?.expires));
  const lanOn = $derived(!!status?.settings.enabled);
  const relayOn = $derived(!!status?.relay.enabled);
  const phones = $derived(status?.devices.filter((d) => d.anywhere).length ?? 0);
  const subtitle = $derived.by(() => {
    if (!status) return null;
    const parts = [
      relayOn ? (status.relay.state === 'online' ? 'From anywhere' : 'From anywhere (connecting)') : null,
      lanOn ? (status.running ? 'On this network' : 'On this network (not running)') : null,
    ].filter(Boolean);
    const watching = status.watching + status.relay.watching;
    return parts.length ? `${parts.join(' · ')}${watching ? ` · ${watching} watching` : ''}` : 'Off';
  });
  const RELAY_STATE: Record<string, string> = {
    online: 'Connected to the relay.',
    connecting: 'Connecting to the relay…',
    unreachable: 'Can’t reach the relay right now; Vyro keeps trying.',
    off: 'Not connected.',
  };
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

  async function turnOnRelay(): Promise<void> {
    if (await run(() => setRelayEnabled(true))) confirmingRelay = false;
  }

  async function copyLink(url: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(url);
      copied = true;
      setTimeout(() => (copied = false), 2500);
    } catch {
      error = 'Could not copy the link.';
    }
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

  async function saveRelay(url: string | null): Promise<void> {
    if (await run(() => setRelayUrl(url))) relayDraft = null;
  }
</script>

<Card title="Remote" {subtitle} id="remote">
  {#if !status}
    <p class="note">Loading…</p>
  {:else}
    <section class="part" aria-labelledby="remote-anywhere">
      <h3 id="remote-anywhere">From anywhere</h3>
      {#if !relayOn}
        <p>See this PC’s readings on your phone from any network, in the Vyro app for Android and iPhone.</p>
        {#if !confirmingRelay}
          <div class="actions">
            <button type="button" onclick={() => (confirmingRelay = true)}>Turn on reach from anywhere…</button>
          </div>
          <p class="note">Off: this PC connects to nothing for it.</p>
        {:else}
          <div class="confirm" role="group" aria-labelledby="relay-before">
            <p id="relay-before"><b>Before you turn it on</b></p>
            <ul>
              <li>
                This PC keeps one connection to the Vyro relay, a small service on Cloudflare, and your phones reach it
                there. Nothing on this PC listens to the network for it.
              </li>
              <li>
                The readings are encrypted on this PC and decrypted only on your phones: the relay cannot read them. It
                sees when this PC and a phone connect, from which addresses, and how much they send.
              </li>
              <li>Only phones you pair here can connect, with a code that works once. Sign one out and it is cut off at once.</li>
              <li>The phone app is read-only: nothing in it can change this PC.</li>
            </ul>
            {#if status.relay.url}
              <p class="note">Relay: <code>{status.relay.url}</code></p>
            {:else}
              <p class="note warn">This build of Vyro has no relay built in. Enter your relay’s address under Options first.</p>
            {/if}
            <div class="actions">
              <button type="button" class="primary" disabled={busy || !status.relay.url} onclick={() => void turnOnRelay()}>
                Turn on reach from anywhere
              </button>
              <button type="button" onclick={() => (confirmingRelay = false)}>Cancel</button>
            </div>
          </div>
        {/if}
      {:else}
        <p role="status" class:warn-text={status.relay.state === 'unreachable'}>
          {RELAY_STATE[status.relay.state] ?? ''}{status.relay.watching
            ? ` ${status.relay.watching} phone${status.relay.watching === 1 ? '' : 's'} watching.`
            : ''}
        </p>
        {#if status.relay.error}<p class="note warn">{status.relay.error}</p>{/if}
        <p class="note">Encrypted end to end. A phone only looks unless you let it change this PC.</p>

        {#if status.relay.pairing}
          <div class="pairing" role="group" aria-labelledby="relay-code">
            {#if status.relay.pairing.qr}
              <svg
                class="qr"
                viewBox="0 0 {status.relay.pairing.qrSize} {status.relay.pairing.qrSize}"
                role="img"
                aria-label="QR code that pairs a phone with this PC"
              >
                <rect width={status.relay.pairing.qrSize} height={status.relay.pairing.qrSize} fill="#fff" />
                <path d={status.relay.pairing.qr} fill="#000" />
              </svg>
            {/if}
            <div>
              <p id="relay-code"><b>Scan it with your phone’s camera</b></p>
              <p class="note">
                It opens the Vyro app in the phone’s browser and pairs it. On iPhone, add the app to the Home Screen when
                it asks, then scan again from there. The code works once and ends in <span role="timer">{relayLeft}</span>.
              </p>
              <div class="actions">
                <button type="button" disabled={busy} onclick={() => status?.relay.pairing && void copyLink(status.relay.pairing.url)}>
                  {copied ? 'Copied' : 'Copy link'}
                </button>
                <button type="button" disabled={busy} onclick={() => void run(cancelRelayPairing)}>Cancel</button>
              </div>
              <p class="note">Send the link only to yourself: anyone who opens it first gets the pairing.</p>
            </div>
          </div>
        {:else}
          <fieldset class="may">
            <legend>The next phone paired may</legend>
            <label><input type="radio" name="pair-control" value={false} bind:group={pairControl} /> Only look</label>
            <label>
              <input type="radio" name="pair-control" value={true} bind:group={pairControl} />
              Look and change this PC: end programs, shut it down or restart it, start a benchmark, switch profiles…
            </label>
          </fieldset>
          <div class="actions">
            <button
              type="button"
              class="primary"
              disabled={busy || phones >= MAX_DEVICES || status.relay.state !== 'online'}
              onclick={() => void run(() => startRelayPairing(pairControl))}
            >
              Pair a phone
            </button>
            <button type="button" disabled={busy} onclick={() => void run(() => setRelayEnabled(false))}>Turn off</button>
          </div>
          {#if phones >= MAX_DEVICES}
            <p class="note">{MAX_DEVICES} phones are paired; sign one out to pair another.</p>
          {:else if status.relay.state !== 'online'}
            <p class="note">Pairing needs the relay connection.</p>
          {/if}
        {/if}
      {/if}
    </section>

    <section class="part" aria-labelledby="remote-lan">
      <h3 id="remote-lan">On this network</h3>
      {#if !lanOn}
        <p>See this PC’s readings in the browser of a phone or another computer on the same network.</p>
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
                The connection is not encrypted: someone on the same network could watch the readings go by. Use it at
                home, not on public Wi-Fi.
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
            <button
              type="button"
              class="primary"
              disabled={busy || status.devices.length - phones >= MAX_DEVICES}
              onclick={() => void run(startRemotePairing)}
            >
              Pair a device
            </button>
            <button type="button" disabled={busy} onclick={() => void run(() => setRemoteEnabled(false))}>Turn off</button>
          </div>
          {#if status.devices.length - phones >= MAX_DEVICES}
            <p class="note">{MAX_DEVICES} devices are paired; sign one out to pair another.</p>
          {/if}
        {:else}
          <div class="actions">
            <button type="button" disabled={busy} onclick={() => void run(() => setRemoteEnabled(false))}>Turn off</button>
          </div>
        {/if}
      {/if}
    </section>

    {#if status.devices.length || lanOn || relayOn}
      <h3>Paired devices</h3>
      {#if status.devices.length}
        <ul class="devices">
          {#each status.devices as d (d.id)}
            <li>
              <div>
                <span class="name">{d.name}</span>
                <span class="via">{d.anywhere ? 'Anywhere' : 'This network'}</span>
                {#if d.watching}<span class="watching">watching now</span>{/if}
                <small>
                  Paired {fmtDate(d.paired)} · until {fmtDate(d.expires)}{d.anywhere ? ', renewed each time it is used' : ''}{d.lastSeen
                    ? ` · last seen ${fmtDateTime(d.lastSeen)}`
                    : ''}{d.lastAddress ? ` from ${d.lastAddress}` : ''}
                </small>
              </div>
              <span class="row-actions">
                {#if d.anywhere}
                  <button
                    type="button"
                    disabled={busy}
                    aria-pressed={d.control}
                    aria-label="{d.name} may change this PC"
                    onclick={() => void run(() => setPhoneControl(d.id, !d.control))}
                  >
                    {d.control ? 'May change this PC' : 'Only looks'}
                  </button>
                {/if}
                <button type="button" disabled={busy} aria-label="Sign out {d.name}" onclick={() => void run(() => signOutRemote(d.id))}>
                  Sign out
                </button>
              </span>
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
    {/if}

    {#if relayOn}
      <h3 id="remote-wake">Wake from phone</h3>
      <label class="check">
        <input
          type="checkbox"
          checked={status.relay.wake}
          disabled={busy}
          onchange={(e) => void run(() => setWakeFromPhone(e.currentTarget.checked))}
        />
        Let a phone have another Vyro PC on this network wake this one
      </label>
      <p class="note">
        A phone cannot send the wake signal itself; another PC that is on, on the same network and paired with the phone
        sends it. Phones are told this PC's wired network address, sealed.
      </p>
      {#if status.relay.wake && readiness}
        <ul class="ready">
          <li class:bad={!readiness.adapter}>
            {readiness.adapter ? `Wired adapter: ${readiness.adapter}` : 'No wired adapter: wake needs a network cable.'}
          </li>
          {#if readiness.adapter}
            <li class:bad={readiness.magicPacket === false}>
              {readiness.magicPacket === true
                ? '“Wake on Magic Packet” is on.'
                : readiness.magicPacket === false
                  ? '“Wake on Magic Packet” is off: Device Manager → the adapter → Advanced.'
                  : 'Check Device Manager → the adapter → Advanced: “Wake on Magic Packet” on, and Power Management: “Allow this device to wake the computer”.'}
            </li>
          {/if}
          {#if readiness.fastStartup}
            <li class="bad">Fast Startup is on: some PCs then wake only from sleep. Turn it off in Control Panel → Power Options.</li>
          {/if}
          <li>Turn on “Wake on LAN” (or “Power on by PCI-E”) in the BIOS: Vyro cannot see it.</li>
          {#if readiness.laptop}<li>Laptops mostly wake only from sleep, on a cable and plugged in.</li>{/if}
        </ul>
      {/if}
    {/if}

    {#if actions.length}
      <h3 id="remote-recent">Recent from your phones</h3>
      <ul class="recent" aria-labelledby="remote-recent">
        {#each actions.slice(0, 10) as a, i (i)}
          <li>
            <span>{a.what}</span>
            <small>{a.phone} · {fmtDateTime(a.at)}{a.error ? ` · not done: ${a.error}` : ''}</small>
          </li>
        {/each}
      </ul>
    {/if}

    <details>
      <summary>Options</summary>
      <div class="options">
        <label>
          <span>Port on this network</span>
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
      <p class="note">
        A new length applies to devices paired from now on; phones paired from anywhere stay paired while they are used at
        least once in it. A new port signs nobody out; open the new address.
      </p>
      <div class="options">
        <label class="grow">
          <span>Relay address</span>
          <input
            type="url"
            placeholder="https://…"
            value={relayDraft ?? (status.relay.custom ? status.relay.url : '')}
            oninput={(e) => (relayDraft = e.currentTarget.value)}
          />
        </label>
        <button type="button" disabled={busy || relayDraft == null} onclick={() => void saveRelay(relayDraft)}>Save</button>
        {#if status.relay.custom}
          <button type="button" disabled={busy} onclick={() => void saveRelay(null)}>Use Vyro’s relay</button>
        {/if}
      </div>
      <p class="note">
        Empty: Vyro’s own relay{status.relay.url && !status.relay.custom ? ` (${status.relay.url})` : ''}. To run your own,
        see docs/remote.md. Phones paired through another relay are signed out when it changes.
      </p>
      {#if relayOn || phones}
        <div class="actions">
          <button type="button" disabled={busy} onclick={() => void run(resetRelay)}>Start over on the relay</button>
        </div>
        <p class="note">New keys for this PC on the relay; every phone paired from anywhere is signed out.</p>
      {/if}
    </details>
  {/if}
  {#if error}<p class="err" role="alert">{error}</p>{/if}
</Card>

<style>
  .may {
    display: grid;
    gap: 0.35rem;
    margin: 0.5rem 0 0;
    padding: 0;
    border: 0;
    font-size: 0.85rem;
  }
  .may legend {
    margin-bottom: 0.25rem;
    color: var(--muted);
  }
  .may label {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    justify-content: flex-end;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .ready,
  .recent {
    display: grid;
    gap: 0.3rem;
    margin: 0.5rem 0 0;
    padding-left: 1.1rem;
    font-size: 0.85rem;
  }
  .ready .bad {
    font-weight: 600;
  }
  .recent {
    padding-left: 0;
    list-style: none;
  }
  .recent li {
    display: grid;
  }
  .recent small {
    color: var(--muted);
  }
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
  .part + .part {
    margin-top: 0.4rem;
    padding-top: 0.2rem;
    border-top: 1px solid var(--line);
  }
  .warn-text {
    color: var(--text);
    padding-left: 0.5rem;
    border-left: 3px solid var(--warn);
  }
  .via {
    margin-left: 0.4rem;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .options .grow {
    flex: 1 1 16rem;
  }
  .options .grow input {
    width: 100%;
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
