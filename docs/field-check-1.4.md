# Field check: 1.4 (the phone app, from anywhere)

Status: **not run yet.** The relay is live at `https://vyro-relay.tanishqjain010.workers.dev` (Relay run 39099314796, 2026-10-18,
from `claude/vyro-1.4` at e94322c). CI-green test build: `vyro-windows-089ce4b` (CI run
39077275048), with no relay built in: set the address in Settings → Remote → Options, or set
`VYRO_RELAY_URL` first and use the next run's build.

1.4 answers the owner's request for a phone app on Android and iPhone that shows the PC from
any network (ROADMAP → 1.4.0, decision 34, [`remote.md`](remote.md)).

## Before you start (once)

The relay has to be live first:

1. **Cloudflare.** Create a free account at dash.cloudflare.com. Open **Workers & Pages** once
   and pick the account's workers.dev subdomain when it asks.
2. **A token.** My Profile → API Tokens → Create Token → the **Edit Cloudflare Workers**
   template → Continue → Create. Copy the token.
3. **The repository's secrets.** GitHub → the repository → Settings → Secrets and variables →
   Actions → New repository secret:
   - `CLOUDFLARE_API_TOKEN`: the token.
   - `CLOUDFLARE_ACCOUNT_ID`: the Account ID shown on Workers & Pages → Overview (right side).
4. **The first deploy** (on your go): Actions → **Relay** → Run workflow, `ref`:
   `claude/vyro-1.4` (`relay.yml` is on `main` so that GitHub offers Run workflow). Its summary
   shows the relay's address, `https://vyro-relay.<sub>.workers.dev`. Done on 2026-10-18:
   `https://vyro-relay.tanishqjain010.workers.dev`.
5. **The address in builds.** Settings → Secrets and variables → Actions → Variables → New
   repository variable `VYRO_RELAY_URL` = `https://vyro-relay.tanishqjain010.workers.dev`. Then the next CI run's
   `vyro-windows-<commit>` artifact has it built in. (Without it, enter the address in Vyro:
   Settings → Remote → Options → Relay address.)

**Build.** The `vyro-windows-<commit>` artifact of the latest CI run on `claude/vyro-1.4`, version
1.4.0. Install the MSI over what you have. About 30 minutes.

## 1. Turn it on and pair

- Settings → Remote → From anywhere → **Turn on reach from anywhere…**: the confirmation says
  what the relay sees and does not. Turn it on: within seconds it says "Connected to the relay".
- **Pair a phone**: a QR code with a 10-minute countdown.
- **Android** (Chrome): scan it with the camera. Vyro opens and shows the readings within a few
  seconds. Install it (the Install button, or the menu → Install app). Open it from the home
  screen: the readings come back.
- **iPhone** (Safari): scan it with the camera. The page asks to add Vyro to the Home Screen
  first. Do so (Share → Add to Home Screen), open it, tap **Scan the code**, allow the camera, and
  scan the same code (still within its 10 minutes). The readings show.
- In Settings, the phone is listed as "Anywhere", "watching now" while the app is open.
- The same QR code scanned by another phone, or again after it worked: "That pairing code has
  expired".

## 2. From anywhere

- Turn the phone's Wi-Fi off (mobile data only): the readings keep coming, about once a second,
  matching the PC's Dashboard.
- Put the app in the background for a minute and open it again: readings within a few seconds.
- Lock the phone for ten minutes, unlock, open the app: readings within a few seconds.
- Put the PC to sleep: the app says "<PC> is asleep or off" within about a minute. Wake it:
  the readings come back by themselves.
- Restart the PC's network (or unplug it for a minute): Settings shows "Connecting" and then
  "Connected" again; the app comes back by itself.

## 3. Signing out

- Settings → Remote → the phone → **Sign out**: the app says "Signed out of <PC>" at once.
  Pair it again: a new code works.
- In the app: **Sign out of this PC**: Settings no longer lists it.
- Turn reach from anywhere off: the app says the PC is asleep or off; turn it on: it comes back
  (the phone stays paired).

## 4. Several PCs (if you have a second PC with 1.4)

- Pair the same phone with both: the app's **PCs** list shows both, each with its state; tap one
  to read it.

## 5. Footprint

- With reach from anywhere on and no phone watching, Task Manager shows Vyro's CPU and memory as
  with it off (one idle connection).
- With the app open on the phone, the PC reads at the window's rate (1 s), as with the local
  companion.

## Recording results

- **Passes:** ROADMAP → 1.4.0 gets "Field check 1.4 (`<PC>`, `<phones>`, `<date>`, test build
  `vyro-windows-<commit>`)".
- **A check fails:** note what you saw, with the phone's model and system version and whether it
  was on Wi-Fi or mobile data. The fix goes on the branch, and the checks it affects are run
  again.
- **Release:** after 1.3.0 is released and merged into this branch, and the owner's go: the
  branch goes into `main`, Relay is run on `main`, and the Release workflow builds 1.4.0.
