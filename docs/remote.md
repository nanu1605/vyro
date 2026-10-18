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
| how many frames go each way, and how big | what the frames say |
| hashes of the keys that let the PC and the phones in | |

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
- **Notifications** for alerts are not sent to phones yet.
- The relay caps frames at 64 KiB, 8 phones per PC with 2 connections each, and rates: the PC
  100 messages a second (bursts of 200), a phone 5 (bursts of 20). Breaking a cap closes the
  connection.

### Running your own relay

1. A free Cloudflare account, with its workers.dev subdomain chosen.
2. In a clone of this repository: `pnpm install`, `pnpm build:phone`, `pnpm --dir relay install`,
   then `pnpm --dir relay exec wrangler login` and `pnpm --dir relay exec wrangler deploy`.
3. In Vyro: Settings → Remote → Options → **Relay address**: the address `wrangler deploy`
   printed. Phones paired through another relay are signed out when it changes.

To try it on one computer: `pnpm --dir relay dev` runs the relay at `http://127.0.0.1:8787`, and
`cargo run -p vyro-core --example companion -- --relay http://127.0.0.1:8787` plays a PC with
simulated readings and prints a pairing link. `scripts/relay-e2e.mjs` does both and drives the
phone app in Chromium (CI runs it).

## On this network

See the README's [Phone companion](../README.md#phone-companion-10) section: a page served by the
app on port 8976, to private addresses only, paired with a code, not encrypted.
