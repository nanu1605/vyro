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

## Keys given, not sold (2.5.5)

The seller can give a key without a sale, for the owner's own PCs or for testers (the owner,
2026-11-26: "build the key page"). Only the owner's own device gives one, once it has checked
them (the owner: "Use some safety and privacy so only i can generate the key in whole jainav
infotech"):

1. **Set up, once:** open `/give` on the website on your PC, type the setup passphrase (the
   GitHub secret `KEY_PAGE_PASSPHRASE`, at least 20 characters) and choose **Set up this device**.
   Windows asks for your fingerprint, face or PIN (Windows Hello) and keeps a passkey for the
   page on this PC. From then on the passphrase gives nothing.
2. **Each key:** open `/give` on that PC and choose **Sign in**. Windows checks you again, and
   the page shows a new key, once.
3. **A backup device** (the owner: "A backup device for the key page"): on a device set up,
   choose **Add another device**. That device checks you first, then shows a code. Within 5
   minutes, either:
   - **on the other device itself** (your phone, or another PC): open `/give` there, choose
     **Set up this device with a code**, type the code, and let that device check you. It keeps
     its own passkey: no Bluetooth, nothing between the two;
   - **or from the PC:** choose **Set up the other device**, and in the browser's window pick a
     password manager that syncs (Google Password Manager, 1Password, Bitwarden), a security
     key, or your phone by its QR code (with Bluetooth on in both); not the PC's own Windows
     Hello, which is set up already.

   Either device can then sign in, so a PC reset or a lost PC keeps nobody out. Up to 5
   devices; an added one can add more. A code is good once; a step that does not finish says
   which it was, with the browser's reason, and can be tried again.
4. **Removing a device** (the owner, 2026-11-27: "build the remove device option"): on a device
   set up, choose **Devices set up here**. That device checks you, then lists each device with
   when it was set up and last signed in, named by the system and browser it was set up from
   ("Windows, Edge", "Android, Chrome"; one made from the browser's window: "A phone, by its QR
   code" or "A security key"). **Remove** takes one off: it can no longer sign in here, and the
   browser, where it can, stops offering its passkey. Within 5 minutes of that sign-in. Any
   device can remove another, or itself, but not the last one that signs in here. A device set
   up before names were kept is named at its next sign-in on itself.

Anyone else who opens the page sees only Sign in, which gives them nothing: their device has no
passkey for it. The passphrase in GitHub sets up a device only while none is set up; more are
added only from one set up. A device lost or in the wrong hands: remove it from another (step
4). To start again (every device lost, one in the wrong hands that removed the others first, or
a passphrase someone may have seen), change `KEY_PAGE_PASSPHRASE` and deploy: the page then
asks to be set up again, and every old device gives nothing more.

Nothing about the owner is kept or sent: the passkey's name is "Clokrix Pro keys" and its user
id is random. The checkout keeps its public key, when it was set up and last signed in, and its
device's name in the fixed words above (nothing else of the browser's User-Agent). A given key
works as a bought one: one PC at a time, Move Pro here, and on Live keys too. It has no order,
payment or receipt: the checkout records it as given. After 20 wrong setup passphrases in an
hour, setup takes none, the right one too, until the oldest is an hour old.

What the page cannot stop: someone who controls the company's GitHub or Cloudflare account can
change what is deployed. Keep both to yourself, with two-factor sign-in.

## The owner's dashboard (2.6)

The owner, 2026-11-27: "Make an admin panel dashboard where i can see keys which are currently in
use and also including the system the key is in use and names of the person who owns that key. I
need a full dashboard."

At `/dashboard` on the website, signed in with a device set up on the key page (above). Before
the sign-in it shows only Sign in. After it:

- **In use now**: keys on a PC that checked in within 8 days (the app checks weekly); then keys
  bought, the money taken in each currency (refunds left out), given, quiet (on a PC not seen in
  8 days), not turned on yet, cancelled and refunded.
- **Keys sold each month**, the last 12 months.
- **Every key**, searchable and filtered by status and source: its owner (the buyer's name from
  checkout, email and phone, read from Razorpay as the dashboard opens and never kept by the
  website; or who a given key was given to), the key (hidden until Show, or copied), the price
  and date, the PC (its name, processor, graphics card, Windows and Clokrix versions, which the
  app sends since 2.6; a short code of the PC's hash before that), when it was turned on or
  moved, and when its PC last checked in.
- **Cancel** turns a key off: its PC turns Free at its next weekly check-in, and the app says
  the key was cancelled. Cancelling refunds nothing; refunds are made in Razorpay, which then
  turns the key off by itself, for good. **Restore** turns on again a key the owner cancelled,
  never one refunded. **Free from its PC** takes a key off its PC, so it can be entered on any
  other without waiting for a move. **Name** sets who a key was given to.
- **Give a key**, with who it is for; **Export CSV** of the keys shown.

A sign-in keeps the dashboard open for 30 minutes at most, each action within 5 minutes of the
one before (it refreshes itself every 4 minutes while it is looked at). A device removed on the
key page signs the dashboard out at its next action.

## What is sent, and to whom

**To the checkout** (Clokrix's own Cloudflare Worker, `licence/`):

- the purchase's one-time code;
- the key;
- the PC's id: a SHA-256 hash of Windows' installation id (`MachineGuid`) with a Clokrix salt,
  never the id itself;
- since 2.6, what the owner's dashboard shows of that PC: its name as Windows shows it, its
  processor and graphics card, and its Windows and Clokrix versions.

The checkout keeps orders, keys, the price paid, the PC each key is on and what the app said of
it, and when that PC last checked in. It keeps nothing else about the buyer.

**To Razorpay:** the buyer's name, typed on the checkout page before Pay (2.6), kept with the
order; and what the buyer types into Razorpay's own window. Razorpay keeps both under Jainav
Infotech Private Limited's account. The dashboard reads them from Razorpay when it opens; the
checkout keeps none of them.

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
private, and public again from 2026-11-26 for a while (the owner: "for sometime i have made vyro
repo public until the quota is reset"); copies published while it was public stay under those
licences. A build without the checkout's
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
     `LICENCE_SIGNING_KEY` (the Cloudflare secrets are the relay's);
   - optional (2.5.5): the secret `KEY_PAGE_PASSPHRASE`, the key page's setup passphrase (Keys
     given, not sold).
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
