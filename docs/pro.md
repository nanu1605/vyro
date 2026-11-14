# Clokrix Free and Pro (2.5)

Clokrix comes in two packages:

- **Free:** the Dashboard (with its layouts and editing), and Settings' own sections:
  Appearance, General, Units, Hidden sensors, Sensor service, Updates and Clokrix Pro.
- **Pro:** everything. That includes Health and alerts, Insights, Gaming, the Benchmark,
  Cooling (fan control), GPU tuning, History, My PC, every part of this PC (Hardware, Storage,
  Network, Displays, Devices, System, Processes, Sensors), desktop widgets, the game overlay,
  profiles and the phone app.

Pro costs **$100, once, for life, on one PC**. Jainan Infotech sells it, and Razorpay takes
the payment.

## The trial

Every PC is Pro for 7 days from the first start of 2.5 or later. That counts both a new install
and an update from 2.4 or earlier. The top bar shows the days left; the chip opens Settings →
Clokrix Pro.

When the trial ends, Clokrix says so once, and the PC turns Free:

- **Turned off, with their settings kept:**
  - fans go back to the BIOS, and fan control is turned off;
  - the GPU goes back to stock, with tuning and "Apply at start" turned off;
  - desktop widgets and the game overlay are hidden;
  - game watching stops (automatic profiles and frame rates);
  - the phone link closes;
  - alerts stop notifying.
- **In the tray:** Profile, Game overlay and Desktop widgets are greyed out. "Return fans to
  BIOS" and "Return GPU to stock" stay.
- **History** keeps recording, so it is all there once Pro is on.

A Pro view in Free shows what Pro adds, with Buy Pro and Enter a key. The sidebar marks Pro
views with a lock.

## Buying

1. **Buy Pro** (Settings → Clokrix Pro, or any Pro view) opens the checkout in the browser. The
   page link carries a one-time code the app made for this purchase.
2. **Pay $100 there.** Razorpay's own checkout takes cards and the rest.
3. **Pro turns on by itself.** While the page is open, the app asks the checkout every 5 seconds
   (for 30 minutes) whether the purchase is paid. Once it is, the app collects the key and turns
   Pro on.
4. **The page shows the key.** Keep it: it sets Pro up again after reinstalling Windows, and it
   moves Pro to another PC. Razorpay emails a receipt.

Bought from the website rather than the app, the page shows the key, which goes in at Settings →
Clokrix Pro → Already have a key?

## One PC

A key is Pro on one PC at a time:

- **Activation:** the checkout binds the key to the PC and signs an *activation* saying so. The
  app checks it at every start with the public key built into it. Pro works offline from then on.
- **Weekly check:** the app asks again at most once a week, when it is online. It turns Free only
  on a clear answer: the key was moved, freed or refunded. A checkout it can't reach changes
  nothing.
- **Another PC:** entering the key there is refused while it is Pro here.
  - **Free this PC** (Settings → Clokrix Pro) releases the key; then enter it on the other PC.
  - **Move Pro here** is for a PC that is gone, or after Windows was reinstalled (the PC's id
    changes then). It takes the key from the other PC, which turns Free at its next check. It
    works once every 30 days.
- **Refund:** a refunded payment's key stops working (Razorpay's webhook tells the checkout).

## What is sent, and to whom

**To the checkout** (Clokrix's own Cloudflare Worker, `licence/`):

- the purchase's one-time code;
- the key;
- the PC's id: a SHA-256 hash of Windows' installation id (`MachineGuid`) with a Clokrix salt,
  never the id itself.

The checkout keeps orders, keys and the PC each key is on, and nothing about the buyer.

**To Razorpay:** what the buyer types into its checkout. Razorpay keeps it under Jainan
Infotech's account.

Nothing else is sent.

## It is open source

The lock is in Clokrix's public code (MIT or Apache-2.0), and a build without the checkout's
settings is Pro: forks, local builds and CI builds before the settings are set. The official
builds carry them. A copy of the code takes another name and logo (README → License).

## Setting it up (the owner)

Nothing here works until these are done; the Release workflow refuses to publish without the
last two variables.

1. **Razorpay** (Dashboard → Account & Settings):
   - **International payments:** turn them on, to charge US dollars.
   - **API keys:** start in Test mode, then make the Live keys. Each is a key id and a key
     secret.
   - **Webhooks:** add one to `<checkout>/razorpay/webhook`, with a secret of your choosing, for
     the events `payment.captured`, `order.paid` and `refund.processed`.
2. **The website:** Razorpay asks for terms, refund and contact pages. Its Buy button links to
   `<checkout>/buy`. Optionally, put the checkout on a domain of yours (Cloudflare → the Worker
   → Domains & Routes).
3. **The signing keys:** `node scripts/licence-keygen.mjs` prints a pair.
   - The private half goes into the GitHub secret `LICENCE_SIGNING_KEY`, and nowhere else.
   - The public half goes into the variable `CLOKRIX_LICENCE_PUBKEY`.
   - Make it once: a new pair turns off every activation signed with the old one.
4. **GitHub** (Settings → Secrets and variables → Actions):
   - secrets `RAZORPAY_KEY_ID`, `RAZORPAY_KEY_SECRET`, `RAZORPAY_WEBHOOK_SECRET` and
     `LICENCE_SIGNING_KEY` (the Cloudflare secrets are the relay's).
5. **Deploy:** Actions → Licence → Run workflow, on your go. It tests the checkout, checks that
   the signing key matches `CLOKRIX_LICENCE_PUBKEY`, deploys it, sets its secrets and prints its
   address. Set the variable `CLOKRIX_LICENCE_URL` to that address.
6. **Try it in Test mode:** a CI test build carries both variables once they are set, so its
   trial runs as a release's will. Buy with one of Razorpay's test cards and check:
   - Pro turns on by itself;
   - the key is refused on a second PC;
   - Move Pro here works there, and the first PC turns Free at its next check.

   Then switch the Razorpay secrets to the Live keys and deploy again.
7. **Release:** on "release 2.5.0", as before.

The checkout's code and tests are in `licence/`. CI runs its tests with Razorpay played by a
local stand-in, and `licence.yml` deploys it.
