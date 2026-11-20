# Clokrix Free and Pro (2.5)

Clokrix comes in two packages:

- **Free:** the Dashboard (with its layouts and editing), and Settings' own sections:
  Appearance, General, Units, Hidden sensors, Sensor service, Updates and Clokrix Pro.
- **Pro:** everything. That includes Health and alerts, Insights, Gaming, the Benchmark,
  Cooling (fan control), GPU tuning, History, My PC, every part of this PC (Hardware, Storage,
  Network, Displays, Devices, System, Processes, Sensors), desktop widgets, the game overlay,
  profiles and the phone app.

Pro costs **$100, once, for life, on one PC**, and **₹10,000 in India** (including all taxes;
the owner, 2026-11-16, from 2.5.2's checkout). Jainav Infotech Private Limited sells it, and
Razorpay takes the payment.

- **Which price:** the checkout shows rupees to a buyer whose connection Cloudflare places in
  India, and dollars to the rest, with a link to pay the other instead (for travellers, VPNs and
  cards that pay only one). The app names the price for the country Windows is set to (2.5.2;
  2.5.0 and 2.5.1 say $100). The website names both.
- **Taxes:** ₹10,000 includes all taxes; any GST comes out of it. $100 is charged as it is.

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
2. **Pay ₹10,000 or $100 there.** Razorpay's own checkout takes cards and the rest (in rupees,
   also UPI and netbanking, as the Razorpay account allows).
3. **Pro turns on by itself.** While the page is open, the app asks the checkout every 5 seconds
   (for 30 minutes) whether the purchase is paid. Once it is, the app collects the key and turns
   Pro on. Closed the page before paying? Buy Pro opens the same purchase again (2.5.2; before
   it, the button stayed greyed out for those 30 minutes, across restarts too).
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

**To Razorpay:** what the buyer types into its checkout. Razorpay keeps it under Jainav
Infotech Private Limited's account.

Nothing else is sent.

## The website

The checkout also serves Clokrix's website, all naming Jainav Infotech Private Limited:

- `/`: what Clokrix is, Free and Pro, the price, the trial, Download and Buy Pro;
- `/terms`, `/privacy`, `/refunds` (a full refund within 14 days, on request; the owner,
  2026-11-16), `/delivery` and `/contact` (the contact variables above);
- `/buy`: the payment page, with the same links at its foot.

With `CLOKRIX_SITE_HOST` set, these pages are served only there: asked for on the workers.dev
address (which 2.5.0's Buy Pro opens), they send the browser to the website, the app's code
kept. The app's own requests and Razorpay's webhook work on either address. From 2.5.1,
`CLOKRIX_LICENCE_URL` is the website, so Buy Pro opens it directly and the app's requests go
there; 2.5.0 keeps using the workers.dev address, which must stay.

The policy pages are a plain starting point, written from what Clokrix does; the owner checks
them before giving the website to Razorpay.

## The code

The code was public under MIT or Apache-2.0 until 2026-11-20, when the owner made the repository
private; copies published before then stay under those licences. A build without the checkout's
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
2. **The website:** Razorpay gives Live keys only after checking it, and takes live payments
   only from its domain. The checkout serves it (see The website below), so it needs only an
   address and the contact details:
   - **The domain:** the company's domain in the same Cloudflare account as the relay (Cloudflare
     → Add a domain). The website goes on a host of it, such as `clokrix.<company domain>`.
   - **The variables** (GitHub → Settings → Secrets and variables → Actions → Variables):
     `CLOKRIX_SITE_HOST` (the host alone, such as `clokrix.example.com`),
     `CLOKRIX_CONTACT_EMAIL`, `CLOKRIX_CONTACT_PHONE` and `CLOKRIX_CONTACT_ADDRESS` (the
     registered address, in lines). They go on a public website, as Razorpay asks.
   - **Deploy** (step 5). The run's summary lists the website's pages. If the deploy is refused on
     the domain, give the Cloudflare token (`CLOUDFLARE_API_TOKEN`) the Zone permissions Workers
     Routes: Edit and DNS: Edit for that domain, and run it again.
   - **In Razorpay:** give `https://<host>` as the business website, with its pages' addresses
     when asked: `/terms`, `/privacy`, `/refunds`, `/delivery` and `/contact`.
3. **The signing keys:** `node scripts/licence-keygen.mjs` prints a pair.
   - The private half goes into the GitHub secret `LICENCE_SIGNING_KEY`, and nowhere else.
   - The public half goes into the variable `CLOKRIX_LICENCE_PUBKEY`.
   - Make it once: a new pair turns off every activation signed with the old one.
4. **GitHub** (Settings → Secrets and variables → Actions):
   - secrets `RAZORPAY_KEY_ID`, `RAZORPAY_KEY_SECRET`, `RAZORPAY_WEBHOOK_SECRET` and
     `LICENCE_SIGNING_KEY` (the Cloudflare secrets are the relay's).
5. **Deploy:** Actions → Licence → Run workflow, on your go. It tests the checkout, checks that
   the signing key matches `CLOKRIX_LICENCE_PUBKEY`, deploys it, sets its secrets and prints its
   address. Set the variable `CLOKRIX_LICENCE_URL` to that address, or, once the website is up,
   to the website, `https://<host>` (as from 2.5.1). It must start with `https://`: a build
   stops on an address without it. From 2.5.2 the deploy also makes the R2 bucket
   `clokrix-downloads`, from which the website gives Clokrix's installers and updates
   (CONTRIBUTING.md → Releasing): turn R2 on in the Cloudflare dashboard (R2 Object Storage)
   before that deploy.
6. **Try it in Test mode:** a CI test build carries both variables once they are set, so its
   trial runs as a release's will. Buy with one of Razorpay's test cards and check:
   - Pro turns on by itself;
   - the key is refused on a second PC;
   - Move Pro here works there, and the first PC turns Free at its next check.

   Then, once Razorpay has checked the website and given them, go live: in Razorpay's Live
   mode, add the webhook again (webhooks are kept per mode; same address and events, a new
   secret), switch the three Razorpay secrets to the Live key id, key secret and that webhook
   secret, and deploy again. From then on, keys bought under Test keys (with Razorpay's
   public test cards, the owner's own test included) no longer turn Pro on: a PC on one turns
   Free at its next check, and 2.5.0 says the key's payment was refunded.
7. **Release:** on "release 2.5.0", as before.

The checkout's code and tests are in `licence/`. CI runs its tests with Razorpay played by a
local stand-in, and `licence.yml` deploys it.
