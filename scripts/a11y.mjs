// Accessibility sweep (P27): axe-core on every tab of the app in all six themes at desktop and
// phone width, the main dialogs and the phone companion, plus keyboard focus rings and reduced
// motion. Runs against the browser preview (sample data) of a finished build:
//
//   pnpm build && pnpm a11y
//
// Exits 1 on any violation, page error, sideways scroll or missing focus ring. CI runs it as its
// own job; locally it uses the Playwright browser already installed (PLAYWRIGHT_BROWSERS_PATH).

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(import.meta.url);
const AXE = readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const PORT = 4180;
const BASE = `http://localhost:${PORT}`;

const THEMES = ['midnight', 'amoled', 'cyber', 'glass', 'minimal', 'daylight'];
const WIDTHS = [1440, 390];
const TABS = [
  'Dashboard', 'Cooling', 'History', 'Health', 'Insights', 'Gaming', 'Benchmark', 'Processes',
  'Hardware', 'My PC', 'Storage', 'Network', 'Displays', 'Devices', 'System', 'Sensors', 'Settings',
];

const problems = [];
const problem = (where, what) => problems.push(`${where}: ${what}`);
let checks = 0;

async function axe(page, where) {
  if (!(await page.evaluate(() => 'axe' in window))) await page.addScriptTag({ content: AXE });
  const found = await page.evaluate(async () =>
    (await window.axe.run(document, { resultTypes: ['violations'] })).violations.map(
      (v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`,
    ),
  );
  checks++;
  for (const f of found) problem(where, f);
}

async function noSidewaysScroll(page, where) {
  const extra = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  if (extra > 1) problem(where, `scrolls sideways by ${extra} px`);
}

async function open(browser, url, { width, theme, motion = 'no-preference', init } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height: 900 }, reducedMotion: motion });
  await ctx.addInitScript(
    ([t, extra]) => {
      try {
        localStorage.setItem('vyro.look', JSON.stringify({ theme: t, accent: 'theme', openOn: 'dashboard' }));
      } catch {
        /* the page works without it */
      }
      if (extra) Function(extra)();
    },
    [theme, init ?? ''],
  );
  const page = await ctx.newPage();
  const where = `${url.replace(BASE, '') || '/'} ${theme} ${width}px`;
  page.on('pageerror', (e) => problem(where, `page error: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error' && !/status of 40[13]/.test(m.text())) problem(where, `console: ${m.text()}`);
  });
  return { ctx, page };
}

async function tab(page, name) {
  await page.getByRole('tab', { name: new RegExp(`^${name}`) }).click();
  await page.waitForTimeout(600);
}

// --- The app: every tab, every theme, both widths. -------------------------------------------

async function sweepApp(browser, theme, width) {
  const { ctx, page } = await open(browser, BASE, { width, theme });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  for (const t of TABS) {
    await tab(page, t);
    if (t === 'Sensors') {
      // A dimmed row (hidden, or no reading) must keep its contrast: make sure one is on screen.
      await page.getByRole('button', { name: /^Hide / }).first().click();
      await page.getByRole('checkbox', { name: /^Show hidden/ }).check();
    }
    const where = `${t} · ${theme} ${width}px`;
    await axe(page, where);
    await noSidewaysScroll(page, where);
  }
  await ctx.close();
}

// --- Dialogs and editors, in a dark and the light theme. ---------------------------------------

async function sweepDialogs(browser, theme, width) {
  const { ctx, page } = await open(browser, BASE, { width, theme });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  const at = (what) => `${what} · ${theme} ${width}px`;

  // Dashboard edit mode with a widget's options open.
  await tab(page, 'Dashboard');
  await page.getByRole('button', { name: 'Edit layout' }).click();
  await page.getByRole('button', { name: 'Add widget' }).click();
  await page.waitForTimeout(300);
  await axe(page, at('Dashboard editor'));
  await noSidewaysScroll(page, at('Dashboard editor'));
  await page.keyboard.press('Escape');
  await page.reload();
  await page.waitForTimeout(1200);

  // An alert rule open for editing.
  await tab(page, 'Health');
  const alerts = page.locator('section.alerts');
  await alerts.getByRole('button', { name: 'Edit' }).first().click();
  await page.waitForTimeout(300);
  await axe(page, at('Alert rule editor'));
  await noSidewaysScroll(page, at('Alert rule editor'));

  // Fan control's confirmation.
  await tab(page, 'Cooling');
  await page.getByRole('button', { name: /Turn on fan control/ }).click();
  await page.waitForTimeout(300);
  await axe(page, at('Fan control confirmation'));
  await page.getByRole('dialog').getByRole('button', { name: /Cancel|Not now/ }).first().click();

  // The share card (from Hardware).
  await tab(page, 'Hardware');
  await page.getByRole('button', { name: 'Make a share card' }).click();
  await page.waitForTimeout(500);
  await axe(page, at('Share card'));
  await noSidewaysScroll(page, at('Share card'));
  await page.keyboard.press('Escape');

  // The remote companion's card: confirmation, then pairing with its QR code.
  await tab(page, 'Settings');
  const remote = page.locator('#remote');
  await remote.scrollIntoViewIfNeeded();
  await remote.getByRole('button', { name: 'Turn on…' }).click();
  await axe(page, at('Remote confirmation'));
  await remote.getByRole('button', { name: 'Turn on', exact: true }).click();
  await remote.getByRole('button', { name: 'Pair a device' }).click();
  await page.waitForTimeout(300);
  await axe(page, at('Remote pairing'));
  await noSidewaysScroll(page, at('Remote pairing'));
  await ctx.close();
}

// --- The app's frame (P28): the command palette and the sidebar folded to icons. ------------

async function sweepShell(browser, theme) {
  const { ctx, page } = await open(browser, BASE, { width: 1440, theme });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  const at = (what) => `${what} · ${theme}`;
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: /^Search/ }).waitFor();
  await page.waitForTimeout(300);
  await axe(page, at('Command palette'));
  await page.keyboard.type('temp');
  await page.waitForTimeout(400);
  await axe(page, at('Command palette, results'));
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Collapse the sidebar' }).click();
  await page.waitForTimeout(400);
  await axe(page, at('Sidebar folded'));
  await noSidewaysScroll(page, at('Sidebar folded'));
  await page.getByRole('button', { name: 'Expand the sidebar' }).click();
  await ctx.close();
}

// --- The phone companion, served the API's answers. --------------------------------------------

const VIEW = {
  ts: 1760000000,
  cpu: { usage: 37.5, temp: 71.2, mhz: 5210, powerW: 88.1 },
  gpus: [
    { name: 'NVIDIA GeForce RTX 5060 Ti', integrated: false, usage: 96, temp: 66, hotspot: 79, mhz: 2805, powerW: 171, vramUsedMb: 6120, vramTotalMb: 16311 },
    { name: 'AMD Radeon(TM) Graphics', integrated: true, usage: 2, temp: 48, hotspot: null, mhz: 600, powerW: null, vramUsedMb: 210, vramTotalMb: 2048 },
  ],
  memory: { usedMb: 14800, totalMb: 31900 },
  drives: [
    { model: 'Samsung SSD 990 PRO 2TB', kind: 'nvme', temp: 52, usedPct: 61, health: 'ok' },
    { model: 'WDC WD40EZAZ', kind: 'sata', temp: 58, usedPct: 88, health: 'warn' },
  ],
  fans: [
    { name: 'CPU Fan', rpm: 1240, pct: 48, pump: false },
    { name: 'Pump Fan', rpm: 2950, pct: 100, pump: true },
  ],
  temps: [{ name: 'Motherboard', value: 41 }, { name: 'DIMM 1', value: 45.5 }],
  power: { totalW: 312, cpuW: 88.1, gpuW: 171 },
  game: { app: 'Cyberpunk2077', fps: 88.4, low1Fps: 62.1, frameMs: 11.3 },
  network: { downMbps: 182.4, upMbps: 9.6, pingMs: 14, lossPct: 0 },
  health: { overall: 'warn', issues: [{ part: 'Storage', status: 'warn', message: 'WDC WD40EZAZ at 58 °C' }] },
  alerts: [{ name: 'GPU hot under load', since: 1759999900 }],
  recent: [{ ts: 1759999950, title: 'GPU hot under load', message: 'GPU hotspot 79 °C for 30 s' }],
};

async function sweepCompanion(browser, theme, width, paired) {
  const { ctx, page } = await open(browser, `${BASE}/remote.html`, { width, theme });
  const json = (body, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) });
  const info = { pc: 'DESKTOP-7E24', version: '1.0.0', theme, accent: 'theme', unit: 'c', device: 'Android phone · Chrome', expires: 1762592000 };
  await page.route('**/api/info', (r) => r.fulfill(paired ? json(info) : json({ error: 'not paired' }, 401)));
  await page.route('**/api/view', (r) => r.fulfill(json({ ...VIEW, ts: Math.floor(Date.now() / 1000) })));
  await page.route('**/api/stream', (r) =>
    r.fulfill({ status: 200, contentType: 'text/event-stream', body: `data: ${JSON.stringify({ ...VIEW, ts: Math.floor(Date.now() / 1000) })}\n\n` }),
  );
  await page.goto(`${BASE}/remote.html`);
  await page.waitForTimeout(800);
  const where = `Companion ${paired ? 'live' : 'pairing'} · ${theme} ${width}px`;
  await axe(page, where);
  await noSidewaysScroll(page, where);
  await ctx.close();
}

// --- Keyboard: every control in reach shows the focus ring. -----------------------------------

async function sweepKeyboard(browser) {
  for (const t of ['Dashboard', 'Cooling', 'Health', 'Settings']) {
    const { ctx, page } = await open(browser, BASE, { width: 1440, theme: 'midnight' });
    await page.goto(`${BASE}/`);
    await page.waitForTimeout(1200);
    await tab(page, t);
    // Start from the selected tab, then walk forward through the page.
    await page.getByRole('tab', { name: new RegExp(`^${t}`) }).focus();
    for (let i = 0; i < 60; i++) {
      await page.keyboard.press('Tab');
      const f = await page.evaluate(() => {
        const el = document.activeElement;
        if (!el || el === document.body) return null;
        const stuck = el === window.__a11yLast;
        window.__a11yLast = el;
        const s = getComputedStyle(el);
        const ring = (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== 'none';
        const label = el.getAttribute('aria-label') || el.labels?.[0]?.textContent || el.textContent || el.getAttribute('name') || '';
        return { what: `${el.tagName.toLowerCase()} "${label.trim().replace(/\s+/g, ' ').slice(0, 40)}"`, ring, stuck };
      });
      if (!f) break;
      checks++;
      if (!f.ring) problem(`Keyboard · ${t}`, `${f.what} shows no focus ring`);
      if (f.stuck) {
        problem(`Keyboard · ${t}`, `focus stuck on ${f.what}`);
        break;
      }
    }
    await ctx.close();
  }
}

// --- Reduced motion: no CSS animation runs. ----------------------------------------------------

async function sweepMotion(browser) {
  const { ctx, page } = await open(browser, BASE, { width: 1440, theme: 'cyber', motion: 'reduce' });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  for (const t of ['Dashboard', 'Cooling', 'Gaming', 'Settings']) {
    await tab(page, t);
    const running = await page.evaluate(() =>
      document.getAnimations().filter((a) => a.playState === 'running').map((a) => a.animationName ?? a.constructor.name),
    );
    checks++;
    if (running.length) problem(`Reduced motion · ${t}`, `still animating: ${running.join(', ')}`);
  }
  await ctx.close();
}

// --- Run. ---------------------------------------------------------------------------------------

const server = spawn(process.execPath, [`${root}node_modules/vite/bin/vite.js`, 'preview', '--port', String(PORT), '--strictPort'], {
  cwd: root,
  stdio: ['ignore', 'pipe', 'inherit'],
});
const stop = () => server.kill();
process.on('exit', stop);

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(BASE)).ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('vite preview did not start');
}

const started = Date.now();
try {
  await waitForServer();
  const browser = await chromium.launch();
  try {
    for (const theme of THEMES) {
      await Promise.all(WIDTHS.map((w) => sweepApp(browser, theme, w)));
      await Promise.all(
        WIDTHS.flatMap((w) => [sweepCompanion(browser, theme, w, false), sweepCompanion(browser, theme, w, true)]),
      );
    }
    for (const theme of ['midnight', 'daylight']) {
      await Promise.all(WIDTHS.map((w) => sweepDialogs(browser, theme, w)));
      await sweepShell(browser, theme);
    }
    await sweepKeyboard(browser);
    await sweepMotion(browser);
  } finally {
    await browser.close();
  }
} finally {
  stop();
}

const secs = Math.round((Date.now() - started) / 1000);
if (problems.length) {
  console.error(`${problems.length} accessibility problem(s) in ${checks} checks (${secs} s):`);
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}
console.log(`Accessibility: ${checks} checks clean (${secs} s).`);
