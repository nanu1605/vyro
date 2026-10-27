# Remote: the phone companion and reach from anywhere

Vyro shows a PC's readings on other devices in two ways. Both are off until turned on in
Settings → Remote, and both run in the app, as the signed-in user, never in the LocalSystem
service. The page on this network is read-only. A phone paired from anywhere only looks too,
unless the owner lets it change the PC (1.5, [below](#control-from-the-phone-15)).

| | On this network (1.0) | From anywhere (1.4) |
|---|---|---|
| Where the phone is | the same local network | any network |
| How it connects | to the PC: plain HTTP on port 8976 | to the Vyro relay, which the PC also connects to |
| What listens on the PC | a port, private addresses only | nothing: the PC connects out |
| Encryption | none (a network you trust) | end to end, PC ↔ phone |
| Pairing | QR code, or an 8-character code typed in | QR code (or its link), once |
| The app | a page the PC serves | an installable web app the relay serves |

## From anywhere

### How it works

- **The relay** is a Cloudflare Worker (`relay/` in this repository) with one Durable Object per
  PC, in the Vyro owner's Cloudflare account, deployed only by `.github/workflows/relay.yml`. It
  also serves the phone app (`phone.html`, `src/phone/`).
- **The PC** (Settings → Remote → From anywhere) keeps one WebSocket to its room on the relay,
  `wss://<relay>/pc/<room>`. The room is 128 random bits. The PC shows the relay a 256-bit key;
  the relay keeps its SHA-256 hash, and the first PC to connect to a room claims it.
- **A phone** connects to `wss://<relay>/phone/<room>/<phone>`. It shows a key derived from its
  own secret; the relay lets it in only if the PC registered that key's hash for it. Keys go in
  the `Sec-WebSocket-Protocol` header, never in the address.
- **Readings** are sent only while a phone has the app open, about once a second, sealed for
  that phone. The relay passes the sealed frames along without opening them.

### Pairing

1. In Settings, **Pair a phone** makes a secret for one phone and shows it in a QR code:
   `https://<relay>/#p=1.<room>.<phone>.<secret>`. Browsers never send the part after `#` to a
   server, so the relay never sees the secret.
2. The phone opens it, stores it, and connects. Its first message, sealed with the link's
   secret, carries a new secret the phone made itself. The PC switches to it and tells the relay.
   From then on the link's secret is worthless: a photo of the QR code pairs nothing.
3. The link lasts 10 minutes. The PC shows it only once the relay has the phone's key, so a
   phone that scans at once is not refused.
4. On iPhone, an app on the Home Screen keeps its own storage, apart from Safari's. The link,
   opened in Safari, asks to add the app to the Home Screen first and scan again from there; it
   can also be used in Safari.

A phone stays paired while it is used at least once in the chosen number of days (30 by
default; 1, 7 or 90). Settings lists the phones and signs any of them out; the relay closes its
connection at once. A phone can also sign itself out.

### Encryption

- Each phone's 32-byte secret goes through HKDF-SHA256 (salt `vyro-relay-1`):
  - info `auth` gives the key the relay checks;
  - info `seal` gives an AES-256-GCM key, which the relay never has.
- A sealed frame is `1 | nonce (12 random bytes) | ciphertext | tag (16)`. Its associated data
  is `"vyro1" | room | phone | direction`, so a frame cannot be moved to another phone or room,
  or reflected back to its sender.
- The PC is `vyro_core::remote::relay::seal` (RustCrypto `aes-gcm`, `hkdf`); the phone is
  `src/phone/seal.ts` (WebCrypto). Both are checked against the same vectors
  (`crates/vyro-core/src/remote/relay/vectors.json`), which were also checked against OpenSSL.
- On the PC, the relay key and the phones' secrets are sealed with DPAPI to the Windows account,
  so a copy of `remote.json` opens on no other account or PC.

### What the relay sees, and what it does not

| The relay sees | The relay never sees |
|---|---|
| that a PC (a random room) and its phones connect, and when | the readings, the PC's name, the theme, the phones' names |
| the addresses they connect from (as any server does) | the secrets, or any key that opens the frames |
| how many frames go each way, and how big | what the frames say (the slow view's pictures too) |
| hashes of the keys that let the PC and the phones in | a screen session's picture and input (they go directly) |

Not stored: no readings and no logs (the Worker's request logs are off). A room keeps the hash
of its PC's key, its phones' ids and key hashes, and when the PC was last connected; it is
deleted 90 days after the PC was last seen.

### Control from the phone (1.5)

- **Off for each phone until the owner allows it, on the PC.** When pairing, Settings asks whether
  the phone may only look or also change the PC; the paired list changes it later. The phone
  cannot give itself permission: the PC checks every request against it.
- **What a phone may do** is a fixed list (`vyro_core::remote::relay::api`). Any paired phone may
  read: processes, devices, Health, Insights, the Performance Score, alerts, fans, profiles,
  benchmark and diagnostics results, PC changes, updates and power state. A phone allowed to
  control the PC may also: switch or keep a profile, give the fans back to the BIOS, turn alert
  rules on or off, start or stop a benchmark, run diagnostics, connect or disconnect a Bluetooth
  device, end a process, check for or install an update, shut down, restart, sleep, hibernate,
  lock, sign out, cancel a shutdown, and have the PC wake another PC. Anything else is refused.
- **Only at the PC:** turning fan control on, confirming a pump, and (1.6) editing GPU settings.
  These need someone who can see and hear the PC.
- **No replays.** On protocol 2 the PC gives each connection of a phone a random session in its
  sealed `info`; every later message from the phone carries it with a count that only goes up.
  The PC drops a message from another connection or with a count it has seen, so the relay
  cannot replay a recorded request. Answers carry the request's id; a long one comes in parts.
- **A record.** Settings → Remote lists what phones changed, with the phone's name and time (the
  last 50, in `phone-actions.json` in the app's data folder). Ending a process and shutting down
  or restarting also show a notification on the PC, and a waiting shutdown shows a banner with
  Cancel on every view.
- **Ending a process** never touches Windows' own processes, Explorer or Vyro, and checks the
  process is still the one named; a process that runs as administrator cannot be ended (Vyro is
  not elevated).
- **Power.** Shut down and Restart wait 30 seconds unless asked for "now", can close apps with
  unsaved work only if asked, and are full shutdowns (not Fast Startup's), which a wake signal can
  wake from. Remote access runs in the app, so after a restart or sign-out the phone shows
  "asleep or off" until someone signs in.
- **Waking a PC.** A phone's browser cannot send the wake signal (web pages cannot send UDP).
  With "Wake from phone" on, a PC tells its phones, sealed, its wired adapter's address; another
  Vyro PC on the same network, paired with the same phone and allowing it, sends the signal
  when asked. Settings shows what Windows says about the adapter's wake setting and Fast
  Startup; the BIOS setting cannot be read. A router's own Wake-on-LAN page, or a smart plug with
  the BIOS set to power on after power loss, are other ways.

### The screen on the phone (1.7)

- **Off for each phone until the owner allows it, on the PC**, apart from control: Settings →
  Remote's paired list has "Sees the screen" next to "May change this PC". A phone that may see
  the screen can also use its mouse and keyboard. Taking it back ends the phone's session at
  once; so do signing the phone out and turning reach from anywhere off.
- **While it lasts** the PC shows a banner with "Stop sharing" on every view, "Stop sharing the
  screen with …" in the tray, and a notification at the start; Settings → Remote's record keeps
  each session's start and end. One phone at a time. A session ends after 10 minutes with no
  input and the phone not showing it, and (if chosen) the PC locks when a phone's session ends.
  Nothing is captured unless a session is on.
- **Direct, end to end.** The phone's WebRTC offer and the PC's answer travel sealed through the
  relay. They carry the certificate fingerprints DTLS checks, so the relay cannot put itself in
  the middle: it never sees the picture or the input. The picture then goes straight from the PC
  to the phone over UDP: on the same network, or through the router at the public address a
  STUN server (Cloudflare's, Google's) reports. The STUN server sees that address, nothing else.
  No TURN server is used.
- **The slow view.** Where no direct path exists (some mobile networks and firewalls), the phone
  asks for one picture at a time through the relay, sealed like everything else: JPEG, at most
  2 a second, 20 minutes a day per PC.
- **What it cannot do.** Vyro runs as the signed-in user: Windows' sign-in, lock and
  administrator prompts are a secure screen it can neither see nor use (the phone says so), a
  window running as administrator can be seen but not used, and Ctrl+Alt+Del cannot be sent.
- **Voice (1.7, decision 39).** Under the same permission and only during a session: a clip
  recorded on the phone is kept on the PC in `Phone clips` (named by Vyro, never the phone; 5
  minutes at most; checked as 16 kHz mono PCM before it is kept; deleted after 7 days) and put on
  the clipboard as a file, only when the phone sends one; the PC records each. The phone's live
  microphone (direct connection only) plays into a virtual cable the owner installed; while it
  plays, the cable is Windows' default microphone if Settings allows it, and the earlier defaults
  are put back after, or at the next start after a crash. A phone with the microphone on is
  heard by any program on the PC that records, and the banner says so. The phone app may use
  the microphone (the relay's page policy allows it for the app itself), and the browser asks
  first.
- **Trust.** A phone that may see the screen sees everything on it, including what is typed
  where it shows. The phone app's code comes from the relay (below): whoever controls the relay
  could serve an app that shows the screen to someone else, for a phone allowed to see it. Keep
  "Sees the screen" for phones you would hand the PC to.

### Notifications (2.0)

- **On the phone:** More → Notifications turns them on for that PC, chooses what they are about
  (alerts, low batteries, game summaries, the PC going quiet) and sends a test. The browser's
  push subscription (its push service's address for this phone, a P-256 key and a 16-byte
  secret) goes to the PC as a `push.set` request, sealed like any other. Any paired phone may
  ask: it concerns only its own notifications. The PC keeps it with the phone in `remote.json`,
  sealed with DPAPI; signing the phone out removes it.
- **On the PC:** each notification is a small JSON object (the PC's room, a title, a text, a tag
  and a time) sealed for the phone's browser with RFC 8291 (`aes128gcm`: a one-time ECDH key,
  HKDF-SHA256 with the browser's secret, AES-128-GCM), then handed to the relay as
  `{"t":"push",…}`. `vyro_core::remote::relay::push` has it, checked against the RFC's own example.
- **On the relay:** it makes a P-256 key the first time one is needed and keeps it in a Durable
  Object of its own (`Keys`); phones subscribe with its public half (`/push/key`). It posts each
  notification to the phone's push service with a VAPID token (RFC 8292) signed with that key,
  only to Google's, Apple's, Mozilla's and Microsoft's push services, only for a phone the PC
  has, at most 24 at once, one every 5 s after that and 300 a day per PC. It tells the PC when a
  push service no longer knows a phone, and the PC forgets that subscription; the phone gives it
  again the next time it opens that PC.
- **"Stopped answering":** the PC seals that notification ahead of time for each phone that
  wants it and leaves the list (its will) with the relay. The relay sends it when the PC's
  connection ends without the PC's `{"t":"leaving"}` (sent as Vyro quits and as Windows goes to
  sleep; shutting down, restarting and signing out end the app, which says it too), or when the
  PC's 30-second `ping`s stop (a room with a will looks every two minutes; the runtime answers
  the pings without waking it and notes when), and the PC is not back within two minutes. So
  the phones hear two minutes after a connection that ended (Vyro crashed), and four to six
  minutes after the last ping of a PC that lost its power or its network. When it is back, the
  relay tells the PC since when it was quiet, and the PC tells those phones.
- **What the relay learns:** when a PC sends a notification, to which phone and which push
  service, and how big it is; never what it says.

### Trust points, plainly

- **The phone app's code comes from the relay.** Whoever controls the Cloudflare account could
  serve an app that leaks the phone's secrets, and, for a phone allowed to change the PC, sends
  it requests (1.5). It is deployed only by `relay.yml` from this repository, by hand; the
  account should have two-factor sign-in. Running your own relay (below) removes that trust
  point; letting phones only look limits what it could do.
- **The relay can deny service:** it can drop frames or close connections. It cannot read or
  forge readings: a forged frame does not open, and the phone ignores readings older than the
  last it showed.
- **A phone that is lost** is signed out from Settings → Remote; the relay cuts it off at once.
  A phone that may change the PC is as strong as its lock screen.

### Limits

- **Free plan.** The relay runs on Cloudflare's free plan: 100,000 requests a day for all users
  together (WebSocket messages count 20 to 1), and Durable Object time only while a message is
  handled. One phone watching costs about 180 requests an hour. Past a daily limit, the relay
  stops answering until 00:00 UTC; nothing is billed. Anyone can open rooms on a public relay,
  so a flood of them could use up a day's limit for everyone: that pauses remote viewing, it
  does not expose anything. A relay of your own (below) is not shared.
- **Networks** that block `*.workers.dev`, or that allow the web only through a proxy, cannot
  reach the relay in 1.4.
- The relay caps frames at 64 KiB, 8 phones per PC with 2 connections each, and rates: the PC
  100 messages a second (bursts of 200), a phone 5 (bursts of 20). Breaking a cap closes the
  connection.
- **A day's allowance** (1.7): the relay passes a PC's frames to its phones up to 1 GB a day
  (UTC), far more than a day of readings or the screen's slow view, and drops the rest, telling
  each phone once, the first time something for it is held back (a phone that connects later
  hears it too); it stops one room from using the shared plan up.

### Running your own relay

1. A free Cloudflare account, with its workers.dev subdomain chosen.
2. In a clone of this repository: `pnpm install`, `pnpm build:phone`, `pnpm --dir relay install`,
   then `pnpm --dir relay exec wrangler login` and `pnpm --dir relay exec wrangler deploy`.
3. In Vyro: Settings → Remote → Options → **Relay address**: the address `wrangler deploy`
   printed. Phones paired through another relay are signed out when it changes.

A relay set up before 2.0 sends no notifications until it is deployed again from this
repository: `wrangler.jsonc` adds the `Keys` Durable Object (migration `v2`). Until then the PC
and the phone leave notifications off, and everything else works as before.

To try it on one computer: `pnpm --dir relay dev` runs the relay at `http://127.0.0.1:8787`, and
`cargo run -p vyro-core --example companion -- --relay http://127.0.0.1:8787` plays a PC with
simulated readings and prints a pairing link. `scripts/relay-e2e.mjs` does both and drives the
phone app in Chromium (CI runs it).

## On this network

See the README's [Phone companion](../README.md#phone-companion-10) section: a page served by the
app on port 8976, to private addresses only, paired with a code, not encrypted.
