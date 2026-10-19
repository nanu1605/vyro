// End to end through the relay (1.4): the relay Worker run locally (wrangler dev, workerd), a
// PC played by the companion example with simulated readings, and the phone app in Chromium.
//
//   pnpm build:phone && pnpm --dir relay install && node scripts/relay-e2e.mjs
//
// It pairs a phone from the PC's link and reads live readings; opens the other tabs (1.5) and
// finds a phone that may only look offered no changes; switches tabs quickly, which asks for more
// at once than the relay takes, without the relay cutting the phone off; lets it control the PC
// and has it lock the PC and end a process (the example PC only prints what it was asked); finds
// the same link refused the second time; checks that nothing on the wire between the phone and
// the relay is readable; signs the phone out from the PC; shows a PC that went away as asleep;
// and runs axe on each screen at phone and desktop width. Exits 1 on any failure. With
// SHOTS=<folder>, it also saves a screenshot of each screen there.

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(import.meta.url);
const AXE = readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const PORT = 8787;
const RELAY = `http://127.0.0.1:${PORT}`;
const problems = [];
let checks = 0;

function problem(where, what) {
  problems.push(`${where}: ${what}`);
  console.error(`✗ ${where}: ${what}`);
  // On CI, also an annotation on the run.
  if (process.env.GITHUB_ACTIONS) console.log(`::error title=${where.replace(/[:,]/g, ' ')}::${String(what).replace(/%/g, '%25').replace(/\r?\n/g, '%0A')}`);
}

function ok(where) {
  checks++;
  console.log(`✓ ${where}`);
}

/** Something worth seeing on the run's page (on CI, a notice there too). */
function note(text) {
  console.log(`· ${text}`);
  if (process.env.GITHUB_ACTIONS) console.log(`::notice title=End to end::${text}`);
}

const children = [];
function run(cmd, args, opts = {}) {
  const child = spawn(cmd, args, { cwd: root, stdio: ['pipe', 'pipe', 'pipe'], ...opts });
  children.push(child);
  return child;
}

function cleanup() {
  for (const c of children) if (c.exitCode === null) c.kill('SIGTERM');
}
process.on('exit', cleanup);

/** Lines a child prints, and a way to wait for one. */
function lines(child, label) {
  const seen = [];
  const waiters = [];
  let buf = '';
  child.stdout.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      seen.push(line);
      if (process.env.VERBOSE) console.log(`  [${label}] ${line}`);
      for (const w of [...waiters]) {
        const m = w.re.exec(line);
        if (m) {
          waiters.splice(waiters.indexOf(w), 1);
          w.resolve(m);
        }
      }
    }
  });
  const errors = [];
  child.stderr.on('data', (d) => {
    errors.push(...String(d).split('\n').map((l) => l.trim()).filter(Boolean));
    if (process.env.VERBOSE) process.stderr.write(`  [${label}] ${d}`);
  });
  return {
    seen,
    errors,
    wait(re, ms = 30_000) {
      const old = seen.map((l) => re.exec(l)).find(Boolean);
      if (old) return Promise.resolve(old);
      return new Promise((resolve, reject) => {
        const w = { re, resolve };
        waiters.push(w);
        setTimeout(() => {
          if (waiters.includes(w)) reject(new Error(`${label}: no line matching ${re} in ${ms} ms`));
        }, ms);
      });
    },
    /** The next line matching `re` from now on. */
    next(re, ms = 30_000) {
      return new Promise((resolve, reject) => {
        const w = { re, resolve };
        waiters.push(w);
        setTimeout(() => {
          if (waiters.includes(w)) reject(new Error(`${label}: no new line matching ${re} in ${ms} ms`));
        }, ms);
      });
    },
  };
}

async function relayUp() {
  for (let i = 0; i < 120; i++) {
    try {
      const r = await fetch(`${RELAY}/`);
      if (r.ok) return;
    } catch {
      /* not yet */
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error('the relay did not start');
}

/** A PC (the companion example) on the relay; its pairing links and states come on stdout. */
function startPc() {
  const child = run('cargo', ['run', '-q', '-p', 'vyro-core', '--example', 'companion', '--', '--relay', RELAY]);
  return { child, out: lines(child, 'pc'), say: (s) => child.stdin.write(`${s}\n`) };
}

async function axe(page, where) {
  if (!(await page.evaluate(() => 'axe' in window))) await page.evaluate(AXE);
  const found = await page.evaluate(async () =>
    (await window.axe.run(document, { resultTypes: ['violations'] })).violations.map(
      (v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`,
    ),
  );
  for (const f of found) problem(where, f);
  const extra = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  if (extra > 1) problem(where, `scrolls sideways by ${extra} px`);
  if (!found.length && extra <= 1) ok(`axe: ${where}`);
}

const SHOTS = process.env.SHOTS;

/** axe at phone width, then at desktop width, then back. */
async function axeBoth(page, where) {
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${where.replace(/[^a-z0-9]+/gi, '-')}.png`, fullPage: true });
  await axe(page, `${where} (390 px)`);
  await page.setViewportSize({ width: 1440, height: 900 });
  await axe(page, `${where} (1440 px)`);
  await page.setViewportSize({ width: 390, height: 844 });
}

/** A phone: its own browser storage, with every WebSocket frame kept. */
async function phone(browser, name) {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  // How each of the page's sockets closed.
  await ctx.addInitScript(() => {
    const Native = window.WebSocket;
    window.__closes = [];
    window.WebSocket = class extends Native {
      constructor(...args) {
        super(...args);
        this.addEventListener('close', (e) => window.__closes.push({ at: Date.now(), code: e.code, reason: e.reason }));
      }
    };
    // Its direct connections, to read what the video brought.
    const Peer = window.RTCPeerConnection;
    window.__peers = [];
    if (Peer) {
      window.RTCPeerConnection = class extends Peer {
        constructor(...args) {
          super(...args);
          window.__peers.push(this);
        }
      };
    }
  });
  const page = await ctx.newPage();
  const frames = [];
  page.on('websocket', (ws) => {
    ws.on('framesent', (f) => frames.push({ dir: 'sent', payload: f.payload, at: Date.now() }));
    ws.on('framereceived', (f) => frames.push({ dir: 'received', payload: f.payload, at: Date.now() }));
  });
  page.on('pageerror', (e) => problem(name, `page error: ${e.message}`));
  page.on('console', (m) => m.type() === 'error' && !/WebSocket|ERR_|status of 4/.test(m.text()) && problem(name, `console: ${m.text()}`));
  return { ctx, page, frames };
}

async function seeText(page, text, where, ms = 20_000) {
  try {
    await page.getByText(text).first().waitFor({ timeout: ms });
    ok(where);
  } catch {
    problem(where, `"${text}" never showed; the page says: ${(await page.locator('main').innerText().catch(() => '')).slice(0, 300)}`);
  }
}

/** For a failure's message: what the phone shows and says now, how its sockets closed (with what
 * it sent in the 5 s before), what the PC printed last (without its pairing links, which carry a
 * phone's secret), and what the relay complained of. */
async function scene(phone, pc, relay) {
  const { page, frames } = phone;
  const flat = (s) => s.trim().replace(/\s+/g, ' ');
  const says = (await page.locator('[role=status], [role=alert]').allInnerTexts().catch(() => [])).map(flat).filter(Boolean);
  const shows = flat((await page.locator('main').innerText().catch(() => '')).slice(0, 300));
  const closes = (await page.evaluate(() => window.__closes ?? []).catch(() => [])).map((c) => {
    const sent = frames.filter((f) => f.dir === 'sent' && f.at <= c.at && f.at > c.at - 5000).length;
    return `${c.code}${c.reason ? ` "${c.reason}"` : ''} after sending ${sent} in 5 s`;
  });
  const said = pc.out.seen.filter((l) => !l.startsWith('link:')).slice(-12);
  const relayed = relay.errors.slice(-6).map(flat);
  return [
    `the phone says: ${says.join(' | ') || 'nothing'}`,
    `shows: ${shows || 'nothing'}`,
    `its sockets closed: ${closes.join(', ') || 'none'}`,
    `the PC last printed: ${said.join(' | ')}`,
    `the relay: ${relayed.join(' | ') || 'nothing'}`,
  ].join('; ');
}

async function main() {
  if (!existsSync(`${root}/relay/public/index.html`)) throw new Error('relay/public is empty: run `pnpm build:phone` first');
  const wrangler = run(`${root}/relay/node_modules/.bin/wrangler`, ['dev', '--port', String(PORT), '--ip', '127.0.0.1', '--log-level', 'warn'], {
    cwd: `${root}/relay`,
    env: { ...process.env, WRANGLER_SEND_METRICS: 'false', CI: 'true' },
  });
  const relay = lines(wrangler, 'relay');
  await relayUp();
  ok('the relay serves the phone app');

  const pc = startPc();
  const link1 = (await pc.out.wait(/^link: (.+)$/, 600_000))[1];
  await pc.out.wait(/^state: "online"$/);
  ok('the PC is on the relay');

  const browser = await chromium.launch();
  try {
    // 1. Pair from the link and read.
    const a = await phone(browser, 'phone A');
    await a.page.goto(link1);
    await seeText(a.page, 'Live', 'phone A pairs and reads live readings');
    await a.page.getByRole('heading', { name: /^CPU/ }).first().waitFor({ timeout: 10_000 });
    if (a.page.url().includes('#p=')) problem('phone A', 'the pairing link stayed in the address bar');
    await pc.out.wait(/^watching: 1$/);
    ok('the PC sees one phone watching');
    await axeBoth(a.page, 'phone app, live readings');

    // 1b. The other tabs (1.5). Paired as the PC's default: it may only look.
    const tab = (name) => a.page.getByRole('navigation').getByRole('button', { name, exact: true }).click();
    await tab('Processes');
    await seeText(a.page, 'What runs on', 'the Processes tab lists what runs on the PC');
    await a.page.locator('.rows li').first().waitFor({ timeout: 10_000 }).then(
      () => ok('…with processes'),
      () => problem('processes', 'no process listed'),
    );
    if (await a.page.getByRole('button', { name: /^End task|^End .*…$/ }).count()) problem('view only', 'End is offered');
    else ok('a phone that may only look is offered no End');
    await axeBoth(a.page, 'phone app, processes');
    await tab('Devices');
    await seeText(a.page, 'Devices on', 'the Devices tab');
    await axeBoth(a.page, 'phone app, devices');
    await tab('Health');
    await seeText(a.page, 'Health of', 'the Health tab');
    await axeBoth(a.page, 'phone app, health');
    await tab('More');
    await seeText(a.page, 'Diagnostics', 'the More tab');
    if (await a.page.getByRole('heading', { name: 'Power' }).count()) problem('view only', 'Power is offered');
    else ok('…without Power for a phone that may only look');
    await axeBoth(a.page, 'phone app, more (view only)');
    // Tabs opened in quick succession ask for more at once than the relay takes (checked below).
    for (let i = 0; i < 3; i++) {
      await tab('Health');
      await tab('More');
    }

    // The owner lets it control the PC: the phone hears at once.
    pc.say('control');
    await seeText(a.page, 'can change this PC', 'a phone allowed on the PC is told at once');
    await a.page.getByRole('heading', { name: 'Power' }).waitFor({ timeout: 10_000 });
    await axeBoth(a.page, 'phone app, more (control)');
    await a.page.getByRole('button', { name: 'Lock…' }).click();
    await axeBoth(a.page, 'phone app, power confirmation');
    await a.page.getByRole('button', { name: 'Lock', exact: true }).click();
    await pc.out.wait(/^asked: power .*"action":"lock"/).then(
      () => ok('Lock reaches the PC'),
      (e) => problem('lock', e.message),
    );
    await tab('Processes');
    await a.page.getByRole('button', { name: /^End .*…$/ }).first().click();
    await a.page.getByRole('button', { name: /^End .* now$/ }).first().click();
    await pc.out.wait(/^asked: process\.end /).then(
      () => ok('End reaches the PC'),
      (e) => problem('end', e.message),
    );

    // 1c. The screen (1.7), its own permission. Playwright's Chromium plays no H.264, so the
    // phone goes to the slow view: the PC's picture through the relay, a picture at a time.
    await tab('Screen');
    await seeText(a.page, 'may not see', 'the Screen tab says the screen is not allowed yet');
    await axeBoth(a.page, 'phone app, screen (not allowed)');
    pc.say('screen');
    const show = a.page.getByRole('button', { name: /^Show .*screen$/ });
    await show.waitFor({ timeout: 10_000 }).then(
      () => ok('a phone allowed the screen is told at once'),
      (e) => problem('screen', e.message),
    );
    await a.page.getByRole('button', { name: 'Screen 2, 1920×1080' }).click();
    await axeBoth(a.page, 'phone app, screen');
    await show.click();
    await a.page.locator('.surface img').waitFor({ timeout: 15_000 }).then(
      () => ok("the slow view shows the PC's picture"),
      (e) => problem('slow view', e.message),
    );
    await seeText(a.page, 'the slow view, a picture at a time', '…and says it is the slow view');
    await pc.out.wait(/^asked: screen\.stop /);
    const surface = await a.page.locator('.surface').boundingBox();
    // Waits are set before the action: on this machine the answer can beat them.
    const clicked = pc.out.next(/^asked: screen\.input .*"button"/);
    if (surface) await a.page.touchscreen.tap(surface.x + surface.width / 2, surface.y + surface.height / 2);
    await clicked.then(
      () => ok('a tap reaches the PC as a click'),
      async (e) => problem('slow view', `${e.message}; ${await scene(a, pc, relay)}`),
    );
    await axeBoth(a.page, 'phone app, screen (slow view)');
    const stop = a.page.getByRole('button', { name: 'Stop', exact: true });
    if (await stop.waitFor({ timeout: 10_000 }).then(() => true, () => false)) {
      await stop.click();
      await seeText(a.page, 'The session ended: you stopped it', '…and Stop ends it');
    } else problem('slow view', `no Stop: the session is no longer shown; ${await scene(a, pc, relay)}`);
    await tab('Live');
    const cut = (await a.page.evaluate(() => window.__closes)).filter((c) => c.code === 1008);
    if (cut.length) problem('pace', `the relay cut phone A off: ${cut.map((c) => c.reason).join(', ')}`);
    else ok('the relay never cut the phone off for asking too fast');

    // 2. The link works once.
    const b = await phone(browser, 'phone B');
    await b.page.goto(link1);
    await seeText(b.page, 'That pairing code has expired', 'the same link on another phone is refused');
    await axeBoth(b.page, 'phone app, expired code');

    // 3. Nothing readable on the wire.
    const plain = /"cpu"|"usage"|"temp"|"gpus"|DESKTOP|"pc":"(?!online)/;
    for (const f of a.frames) {
      const text = typeof f.payload === 'string' ? f.payload : Buffer.from(f.payload).toString('latin1');
      if (typeof f.payload === 'string' && !/^(ping|pong|\{"t":"pc","online":(true|false)\})$/.test(f.payload)) {
        problem('wire', `a text frame (${f.dir}) the relay could read: ${f.payload.slice(0, 80)}`);
      } else if (typeof f.payload !== 'string' && plain.test(text)) {
        problem('wire', `a binary frame (${f.dir}) with readable readings`);
      }
    }
    const sealed = a.frames.filter((f) => typeof f.payload !== 'string').length;
    if (sealed < 3) problem('wire', `only ${sealed} sealed frames seen`);
    else ok(`${a.frames.length} frames between phone and relay, ${sealed} sealed, none readable`);

    // 3b. A direct session (1.7), where the browser plays H.264 (Google Chrome, as CI's runner
    // has, or the one CHROME names; not Playwright's Chromium): offer and answer through the
    // relay, then straight to the PC on this machine, which sends a moving test pattern through
    // the app's own producer (OpenH264 standing in for Windows' encoder).
    let chrome = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : { channel: 'chrome' }).catch(() => null);
    if (chrome) {
      const probe = await chrome.newPage();
      const h264 = await probe.evaluate(() =>
        (RTCRtpReceiver.getCapabilities('video')?.codecs ?? []).some((c) => c.mimeType.toLowerCase() === 'video/h264'),
      );
      await probe.close();
      if (!h264) {
        await chrome.close();
        chrome = null;
        note('this Google Chrome plays no H.264: the direct session is not tried');
      }
    } else note('no Google Chrome here: the direct session is not tried');
    if (chrome) {
      try {
        pc.say('');
        const link = (await pc.out.next(/^link: (.+)$/))[1];
        const d = await phone(chrome, 'phone D');
        await d.page.goto(link);
        await seeText(d.page, 'Live', 'phone D (Chrome) pairs');
        pc.say('screen');
        await d.page.getByRole('navigation').getByRole('button', { name: 'Screen', exact: true }).click();
        const start = d.page.getByRole('button', { name: /^Show .*screen$/ });
        await start.waitFor({ timeout: 10_000 });
        const connected = pc.out.next(/^screen: connected$/, 30_000);
        await start.click();
        await connected.then(
          () => {
            ok('the phone connects to the PC directly');
            note('a direct session (Google Chrome) connected to the example PC');
          },
          (e) => problem('direct', e.message),
        );
        await seeText(d.page, 'Tap to click', '…and the phone knows it is direct');
        // The picture: decoded, not blank, and still there when the screen stops changing.
        await seeText(d.page, /Receiving \d+×\d+/, '…and plays the PC\'s video');
        const picture = () =>
          d.page.evaluate(async () => {
            const v = document.querySelector('.surface video');
            const stats = { decoded: 0 };
            (await window.__peers?.at(-1)?.getStats())?.forEach((r) => {
              if (r.type === 'inbound-rtp' && r.kind === 'video') stats.decoded = r.framesDecoded ?? 0;
            });
            if (!v || !v.videoWidth) return { spread: 0, ...stats };
            const c = document.createElement('canvas');
            c.width = 64;
            c.height = 36;
            const g = c.getContext('2d', { willReadFrequently: true });
            g.drawImage(v, 0, 0, 64, 36);
            const px = g.getImageData(0, 0, 64, 36).data;
            const luma = [];
            for (let i = 0; i < px.length; i += 4) luma.push(0.299 * px[i] + 0.587 * px[i + 1] + 0.114 * px[i + 2]);
            const mean = luma.reduce((a, b) => a + b, 0) / luma.length;
            const spread = Math.sqrt(luma.reduce((a, b) => a + (b - mean) ** 2, 0) / luma.length);
            return { spread: Math.round(spread), ...stats };
          });
        const moving = await picture();
        if (moving.spread < 20) problem('direct video', `the picture is blank (spread ${moving.spread})`);
        else ok(`the phone shows the PC's picture (spread ${moving.spread}, ${moving.decoded} decoded)`);
        // What arrives, both ends: the phone's own reading of the picture and the PC's report.
        await d.page.locator('details.arrives summary').click();
        await seeText(d.page, /Here: brightness \d+ %/, '…and says how bright the picture is here');
        await seeText(d.page, /its screen \d+×\d+, sent as \d+×\d+/, '…and what the PC sent');
        pc.say('still');
        await d.page.waitForTimeout(4000);
        const still = await picture();
        if (still.spread < 20) problem('still screen', `the picture went blank on a still screen (spread ${still.spread})`);
        else if (still.decoded <= moving.decoded) problem('still screen', `no picture arrived in 4 s of a still screen (${still.decoded} decoded)`);
        else ok(`a still screen stays on the phone and is sent again (${still.decoded - moving.decoded} pictures in 4 s)`);
        pc.say('moving');
        const box = await d.page.locator('.surface').boundingBox();
        const pressed = pc.out.next(/^screen input: Button/);
        if (box) await d.page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
        await pressed.then(
          () => ok('a tap reaches the PC over the direct connection'),
          (e) => problem('direct input', e.message),
        );
        const stopped = pc.out.next(/^asked: screen\.stop /);
        await d.page.getByRole('button', { name: 'Stop', exact: true }).click();
        await stopped.then(
          () => ok('…and Stop ends it'),
          (e) => problem('direct stop', e.message),
        );
      } finally {
        await chrome.close();
      }
    }

    // 4. The list of PCs.
    await a.page.getByRole('button', { name: 'PCs' }).click();
    await seeText(a.page, 'Your PCs', 'the home screen lists the PC');
    await a.page.locator('.pc-state.online').first().waitFor({ timeout: 10_000 }).then(() => ok('…as online'), () => problem('home', 'the PC is not shown online'));
    await axeBoth(a.page, 'phone app, home');
    await a.page.locator('.pc-row').first().click();
    await seeText(a.page, 'Live', 'opening it again reads live');

    // 5. Signed out from the PC.
    pc.say('out');
    await seeText(a.page, 'Signed out of', 'a phone signed out on the PC is told at once');
    await axeBoth(a.page, 'phone app, signed out');

    // 6. The PC goes away.
    pc.say('');
    const link2 = (await pc.out.next(/^link: (.+)$/))[1];
    const c = await phone(browser, 'phone C');
    await c.page.goto(link2);
    await seeText(c.page, 'Live', 'a new link pairs another phone');
    pc.child.kill('SIGTERM');
    await seeText(c.page, 'is asleep or off', 'a PC that went away shows as asleep', 30_000);
    await axeBoth(c.page, 'phone app, PC asleep');
  } finally {
    await browser.close();
  }
}

main()
  .catch((e) => problem('run', e.message))
  .finally(() => {
    cleanup();
    if (problems.length) {
      console.error(`\nRelay end to end: ${problems.length} problem(s) in ${checks + problems.length} checks.`);
      process.exit(1);
    }
    console.log(`\nRelay end to end: ${checks} checks passed.`);
    process.exit(0);
  });
