// Accessibility sweep (P27): axe-core on every tab of the app in all six themes at two desktop
// widths and phone width, the main dialogs, the desktop widgets and the phone companion, plus keyboard focus
// rings and reduced motion. Since 1.2 also: text stays inside every desktop widget (with the longest
// names two GPUs give), and the Dashboard's cards pack without blank stretches or overlaps, their
// text inside them in whole words even at their narrowest. Since 1.3 also: custom accents that are
// hard to read as picked (navy on Midnight, pale yellow on Daylight), and Settings with the colour
// wheel open. Since 1.4 also: Settings → Remote's reach from anywhere (the relay's confirmation,
// a phone's pairing code, the options). Since 1.5 also: What's new after an update. Since 1.6 also: GPU
// tuning's confirmation, and the view with tuning on and a stability test running. Since 1.7 also: the
// screen-shared banner. Since 2.0.2 also: the views' cards pack without overlaps or gaps, also as
// the window narrows and widens again (and the Dashboard's widgets stay inside their grid); they
// move by dragging their grip, by its arrow keys, back on Escape, and stay moved after a reload and
// at the window size they were moved at; and nothing scrolls sideways, not even an area inside a
// view, from the narrowest window up. Since 2.0.3 also: Ctrl K to a Settings or Health section
// lands on it from a fresh start. The phone app itself is swept by scripts/relay-e2e.mjs.
// Runs against the browser preview
// (sample data, two GPUs as on the owner's PC) of a finished build:
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
const WIDTHS = [1920, 1440, 390];
const TABS = [
  'Dashboard', 'Cooling', 'GPU tuning', 'History', 'Health', 'Insights', 'Gaming', 'Benchmark', 'Processes',
  'Hardware', 'My PC', 'Storage', 'Network', 'Displays', 'Devices', 'System', 'Sensors', 'Widgets', 'Settings',
];

// Desktop widgets (P30), one of each kind and style, as the browser preview keeps them.
const DESK = (id, kind, sources, style) => ({
  id, kind, sources, x: 0, y: 0, placed: false, scale: 100, opacity: 85, style, layer: 'desktop', locked: false, clickThrough: false,
});
const WIDGETS = {
  version: 1,
  shown: true,
  widgets: [
    DESK('gauge1', 'gauge', ['cpu.usage'], 'app'),
    DESK('reading1', 'reading', ['cpu.temp'], 'solid'),
    DESK('graph1', 'graph', ['gpu.0.usage'], 'app'),
    DESK('stack1', 'stack', ['cpu.usage', 'cpu.temp', 'gpu.0.usage', 'gpu.0.temp', 'mem.load', 'cpu.power'], 'glass'),
    DESK('clock1', 'clock', ['cpu.temp', 'gpu.0.temp'], 'app'),
    DESK('network1', 'network', [], 'glass'),
    DESK('game1', 'game', [], 'solid'),
    { ...DESK('top1', 'top', ['proc.ram'], 'app'), locked: true, clickThrough: true },
  ],
};
const SEED_WIDGETS = `localStorage.setItem('vyro.widgets', ${JSON.stringify(JSON.stringify(WIDGETS))})`;
// The longest texts each kind shows: the second GPU's readings ("GPU hotspot (GPU 2)"), a dial
// on a rate in the hundreds, a clock with two GPU readings (1.2's finding: past the edge).
const LONG = {
  version: 1,
  shown: true,
  widgets: [
    DESK('clock2', 'clock', ['gpu.1.temp', 'gpu.0.hotspot'], 'app'),
    DESK('gauge2', 'gauge', ['net.down'], 'app'),
    DESK('reading2', 'reading', ['gpu.1.vram'], 'app'),
    DESK('stack2', 'stack', ['gpu.1.usage', 'gpu.1.temp', 'gpu.0.hotspot', 'gpu.1.clock', 'gpu.0.vram', 'cpu.power'], 'app'),
    DESK('graph2', 'graph', ['gpu.1.clock'], 'app'),
    DESK('clock3', 'clock', ['gpu.0.hotspot'], 'glass'),
  ],
};
const SEED_LONG = `localStorage.setItem('vyro.widgets', ${JSON.stringify(JSON.stringify(LONG))})`;

/** A widget window's size, logical px: `widgetSize` in src/widgets.ts at 100 %. */
function deskSize(w) {
  const n = Math.max(1, w.sources.length);
  return {
    gauge: [200, 200],
    graph: [300, 150],
    stack: [250, 52 + 36 * n],
    clock: [260, 100 + 20 * w.sources.length],
    network: [280, 150],
    game: [260, 150],
    top: [260, 200],
  }[w.kind] ?? [220, 104];
}

// The owner's Dashboard (1.2's finding): the thread bars across the top, then six cards of very
// different heights side by side.
const r3 = (source) => ['reading', 3, 1, source];
const OWNER = [
  ['gauge', 6, 3, 'cpu'], ['gauge', 6, 3, 'gpu.0'],
  r3('cpu.temp'), r3('cpu.power'), r3('gpu.0.temp'), r3('gpu.0.hotspot'), r3('gpu.0.vram'), r3('mem.used'), r3('power.system'), r3('fan.cpu'),
  ['card', 12, 2, 'cores'], ['card', 4, 2, 'cpuSensors'], ['card', 4, 2, 'gpus'], ['card', 4, 2, 'memory'],
  ['card', 4, 2, 'power'], ['card', 4, 2, 'board'], ['card', 4, 2, 'self'],
];
// Every card at its narrowest, 3 of 12 (1.2's second finding): in a window about half a screen
// wide, each value in their lists broke a letter or two a line.
const NARROW = [
  ['card', 3, 2, 'cpuSensors'], ['card', 3, 2, 'self'], ['card', 3, 2, 'power'], ['card', 3, 2, 'board'],
  ['card', 3, 2, 'gpus'], ['card', 3, 2, 'memory'], ['card', 3, 2, 'cores'], ['gauge', 3, 3, 'gpu.0'],
  r3('gpu.0.hotspot'), r3('gpu.0.vram'), r3('power.system'), r3('fan.cpu'),
];
const layout = (id, name, widgets) => ({ id, name, preset: null, locked: false, widgets: widgets.map(([kind, w, h, source], i) => ({ id: `w${i + 1}`, kind, w, h, source })) });
const seedDash = (active) =>
  `localStorage.setItem('vyro.dashboard', ${JSON.stringify(JSON.stringify({ version: 1, active, layouts: [layout('owner', 'Owner', OWNER), layout('narrow', 'Narrow', NARROW)] }))})`;

const problems = [];
const problem = (where, what) => problems.push(`${where}: ${what}`);
let checks = 0;

// Views and dialogs come in, and the sidebar's highlight slides, with short animations; axe must
// see where they end, not a frame on the way. On CI's software GPU, leaving My PC delayed them past
// a fixed wait, and axe caught a label fading to muted over the moving highlight (4.2:1 for a few
// frames in Midnight). Readouts easing to a new value (transitions outside the sidebar) restart
// every second and are not waited for. In a later run axe caught Hardware's label muted with the
// highlight still under it, on My PC, so the highlight must also sit on the selected view. On CI,
// My PC can go over 5 s without drawing a frame (likely its 3-D view), every animation frozen part
// way, so the wait is 15 s. After that axe checks the page as it is, and what still moved is
// logged. A contrast violation is checked again a second later, and only one seen both times counts.
async function settle(page, where) {
  const limit = 15000;
  const until = Date.now() + limit;
  for (;;) {
    const left = await page.evaluate(moving);
    const off = await page.evaluate(markOff);
    if (off > 1) left.push(`the highlight ${Math.round(off)} px off the selected view`);
    if (!left.length) return;
    if (Date.now() >= until) {
      console.warn(`${where}: still moving after ${limit / 1000} s, checked as it is: ${left.join('; ')}`);
      return;
    }
    await page.waitForTimeout(100);
  }
}

/** The finite animations still running, less transitions outside the sidebar: "type name on target at now/end ms". */
function moving() {
  return document
    .getAnimations()
    .filter((a) => a.playState === 'running' && a.effect?.getComputedTiming().endTime !== Infinity)
    .filter((a) => !(a instanceof CSSTransition) || a.effect?.target?.closest?.('.sidebar'))
    .map((a) => {
      const t = a.effect?.target;
      const on = t ? `${t.tagName.toLowerCase()}${t.id ? `#${t.id}` : ''}${[...t.classList].map((c) => `.${c}`).join('')}` : 'no element';
      const name = a.transitionProperty ?? a.animationName ?? a.id;
      const end = a.effect?.getComputedTiming().endTime ?? 0;
      return `${a.constructor.name}${name ? ` ${name}` : ''} on ${on} at ${Math.round(a.currentTime)}/${Math.round(end)} ms`;
    });
}

/** How far the sidebar's highlight is off the selected view, px; null where either is missing. */
function markOff() {
  const m = document.querySelector('.sidebar .mark')?.getBoundingClientRect();
  const b = document.querySelector('.sidebar [role="tab"][aria-selected="true"]')?.getBoundingClientRect();
  return m && b ? Math.max(Math.abs(m.top - b.top), Math.abs(m.height - b.height)) : null;
}

async function axe(page, where) {
  await settle(page, where);
  if (!(await page.evaluate(() => 'axe' in window))) await page.addScriptTag({ content: AXE });
  const run = () =>
    page.evaluate(async () =>
      (await window.axe.run(document, { resultTypes: ['violations'] })).violations.map((v) => ({
        id: v.id,
        impact: v.impact,
        nodes: v.nodes.map((n) => n.target.join(' ')),
      })),
    );
  const found = await run();
  const contrast = found.find((v) => v.id === 'color-contrast');
  if (contrast) {
    await page.waitForTimeout(1000);
    await settle(page, where);
    const again = (await run()).find((v) => v.id === 'color-contrast')?.nodes ?? [];
    const gone = contrast.nodes.filter((n) => !again.includes(n));
    if (gone.length) console.warn(`${where}: color-contrast gone a second later, not counted: ${gone.join(' | ')}`);
    contrast.nodes = contrast.nodes.filter((n) => again.includes(n));
  }
  checks++;
  for (const v of found) if (v.nodes.length) problem(where, `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).join(' | ')}`);
}

/**
 * Nothing scrolls sideways (2.0.2: "no horizontal scrolling throughout vyro"): not the window,
 * measured with the frame's clip lifted so what it hides still counts, and no area in it (a table
 * in a card, a dialog's list) from 480 px, the narrowest the window gets (tauri.conf.json), or at
 * any width with `inner`.
 */
async function noSidewaysScroll(page, where, inner = (page.viewportSize()?.width ?? 0) >= 480) {
  const found = await page.evaluate((inner) => {
    const main = document.querySelector('.main');
    const clip = main?.style.overflowX ?? '';
    if (main) main.style.overflowX = 'visible';
    const extra = document.documentElement.scrollWidth - document.documentElement.clientWidth;
    if (main) main.style.overflowX = clip;
    const out = extra > 1 ? [`scrolls sideways by ${extra} px`] : [];
    if (inner)
      for (const el of document.querySelectorAll('body *')) {
        if (!el.clientWidth || el.scrollWidth - el.clientWidth <= 1 || !/auto|scroll/.test(getComputedStyle(el).overflowX)) continue;
        const name = el.getAttribute('aria-label') ?? [...el.classList].filter((c) => !c.startsWith('svelte-')).join('.');
        out.push(`${el.tagName.toLowerCase()} "${name}" scrolls sideways by ${el.scrollWidth - el.clientWidth} px`);
      }
    return out;
  }, inner);
  checks++;
  for (const f of found) problem(where, f);
}

async function open(browser, url, { width, theme, motion = 'no-preference', init, accent = 'theme' } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height: 900 }, reducedMotion: motion });
  await ctx.addInitScript(
    ([t, a, extra]) => {
      try {
        localStorage.setItem('vyro.look', JSON.stringify({ theme: t, accent: a, openOn: 'dashboard' }));
      } catch {
        /* the page works without it */
      }
      if (extra) Function(extra)();
    },
    [theme, accent, init ?? ''],
  );
  const page = await ctx.newPage();
  const where = `${url.replace(BASE, '') || '/'} ${theme} ${width}px`;
  page.on('pageerror', (e) => problem(where, `page error: ${e.message}`));
  page.on('console', (m) => {
    if (m.type() === 'error' && !/status of 40[13]/.test(m.text())) problem(where, `console: ${m.text()}`);
  });
  return { ctx, page };
}

/**
 * A view's card grid (2.0.2): no card on another, none past the grid's sides, no column empty, and
 * no gap: down each column a card reaches to the gutter above the next, the last to the grid's last
 * line (the owner's report: "lot of empty spaces in between").
 */
async function cardsPacked(page, where) {
  const found = await page.evaluate(() => {
    const bad = [];
    for (const g of document.querySelectorAll('[role="tabpanel"]:not([hidden]) .card-grid')) {
      const gr = g.getBoundingClientRect();
      // The card itself, as far as it reaches down; its cell keeps its own height.
      const boxes = [...g.querySelectorAll(':scope > .card-cell')].map((c) => ({ id: c.dataset.card, r: (c.querySelector(':scope > .card') ?? c).getBoundingClientRect() }));
      if (!boxes.length) continue;
      const gap = parseFloat(getComputedStyle(g).columnGap) || 16;
      const last = Math.max(...boxes.map((b) => b.r.bottom));
      for (let i = 0; i < boxes.length; i++) {
        const a = boxes[i].r;
        if (a.left < gr.left - 1 || a.right > gr.right + 1) bad.push(`"${boxes[i].id}" past the grid's side`);
        for (let j = i + 1; j < boxes.length; j++) {
          const b = boxes[j].r;
          if (a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1) bad.push(`"${boxes[i].id}" and "${boxes[j].id}" overlap`);
        }
        const below = boxes.filter((b) => b.r.left < a.right - 1 && a.left < b.r.right - 1 && b.r.top >= a.bottom - 1).map((b) => b.r.top);
        const room = below.length ? Math.min(...below) - a.bottom - gap : last - a.bottom;
        if (Math.abs(room) > 2) bad.push(`${Math.round(room)} px of room under "${boxes[i].id}"`);
      }
      const cols = getComputedStyle(g).gridTemplateColumns.split(' ').filter(Boolean).length;
      const colW = (gr.width - (cols - 1) * gap) / cols;
      for (let c = 0; c < cols; c++) {
        const x = gr.left + c * (colW + gap) + colW / 2;
        if (!boxes.some((b) => b.r.left <= x && x <= b.r.right)) bad.push(`column ${c + 1} of ${cols} has no card`);
      }
    }
    return bad;
  });
  checks++;
  for (const f of found) problem(where, f);
}

/** The Dashboard's widgets: inside their grid, none on another (2.0.2: after the window resized). */
async function widgetsPacked(page, where) {
  const found = await page.evaluate(() => {
    const bad = [];
    for (const g of document.querySelectorAll('[role="tabpanel"]:not([hidden]) .dash-grid')) {
      const gr = g.getBoundingClientRect();
      const boxes = [...g.querySelectorAll(':scope > .dash-cell')].map((c) => ({ name: c.querySelector('h2')?.textContent ?? '?', r: c.getBoundingClientRect() }));
      for (let i = 0; i < boxes.length; i++) {
        const a = boxes[i].r;
        if (a.left < gr.left - 1 || a.right > gr.right + 1 || a.width < 40) bad.push(`"${boxes[i].name}" ${a.width < 40 ? `squeezed to ${Math.round(a.width)} px` : "past the grid's side"}`);
        for (let j = i + 1; j < boxes.length; j++) {
          const b = boxes[j].r;
          if (a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1) bad.push(`"${boxes[i].name}" and "${boxes[j].name}" overlap`);
        }
      }
    }
    return bad;
  });
  checks++;
  for (const f of found) problem(where, f);
}

async function tab(page, name) {
  await page.getByRole('tab', { name: new RegExp(`^${name}`) }).click();
  await page.waitForTimeout(600);
}

// --- The app: every tab, every theme, both widths. -------------------------------------------

async function sweepApp(browser, theme, width) {
  const { ctx, page } = await open(browser, BASE, { width, theme, init: SEED_WIDGETS });
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
    await cardsPacked(page, where);
  }
  await ctx.close();
}

// --- Custom accents (1.3): colours text cannot take as picked. ----------------------------------

async function sweepCustomAccent(browser, theme, accent, width) {
  const { ctx, page } = await open(browser, BASE, { width, theme, accent });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  for (const t of ['Dashboard', 'Hardware', 'Devices', 'Settings']) {
    await tab(page, t);
    const where = `${t} · ${theme} with accent ${accent} ${width}px`;
    if (t === 'Settings') {
      // The colour wheel shows with a custom accent.
      const wheel = page.getByRole('slider', { name: /hue and saturation/ });
      await wheel.scrollIntoViewIfNeeded();
      if ((await wheel.count()) === 0) problem(where, 'no colour wheel for a custom accent');
    }
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

  // What's new (1.5), as after an update from 1.4.1: once, and gone after Got it.
  await page.goto(`${BASE}/?whatsnew=1.4.1`);
  const news = page.getByRole('dialog', { name: /^What’s new in Vyro/ });
  await news.waitFor({ timeout: 5000 }).catch(() => problem(at("What's new"), 'does not show after an update'));
  await page.waitForTimeout(500);
  await axe(page, at("What's new"));
  await noSidewaysScroll(page, at("What's new"));
  await news.getByRole('button', { name: 'Got it' }).click();
  if (await news.isVisible()) problem(at("What's new"), 'still shows after Got it');
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);

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

  // GPU tuning (1.6): its confirmation, then the view with tuning on, a preset applied and a
  // stability test running.
  await tab(page, 'GPU tuning');
  await page.getByRole('button', { name: 'Turn on GPU tuning…' }).click();
  await page.waitForTimeout(300);
  await axe(page, at('GPU tuning confirmation'));
  await page.getByRole('dialog').getByRole('button', { name: 'Turn on', exact: true }).click();
  await page.getByRole('button', { name: 'Apply Performance' }).click();
  await page.getByRole('button', { name: 'Test Efficient' }).click();
  await page.getByRole('progressbar', { name: 'Test progress' }).waitFor();
  await page.waitForTimeout(300);
  await axe(page, at('GPU tuning on, testing'));
  await noSidewaysScroll(page, at('GPU tuning on, testing'));
  await page.getByRole('button', { name: 'Stop', exact: true }).click();

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
  // Reach from anywhere (1.4): its confirmation, then pairing a phone, and the options.
  await remote.getByRole('button', { name: 'Turn on reach from anywhere…' }).click();
  await axe(page, at('Relay confirmation'));
  await remote.getByRole('button', { name: 'Turn on reach from anywhere', exact: true }).click();
  await remote.getByRole('button', { name: 'Pair a phone' }).click();
  await remote.getByRole('img', { name: /pairs a phone/ }).waitFor();
  await remote.getByText('Options', { exact: true }).click();
  await page.waitForTimeout(300);
  await axe(page, at('Relay pairing and options'));
  await noSidewaysScroll(page, at('Relay pairing and options'));
  // The screen shared with a phone (1.7): the banner on every view (with the phone's
  // microphone on), and Stop.
  await page.goto(`${BASE}/?screen=mic`);
  await page.getByText('Your screen is shared').waitFor();
  await page.getByText('this PC hears the phone').waitFor();
  await axe(page, at('Screen shared banner'));
  await noSidewaysScroll(page, at('Screen shared banner'));
  await page.getByRole('button', { name: 'Stop sharing' }).click();
  await page.getByText('Your screen is shared').waitFor({ state: 'detached' });
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
  // The highlight ends on the selected view once the sidebar unfolds: by its button, and when a
  // narrow window (the rail) is widened. Widgets sits below the "This PC" label, which wrapped
  // mid-unfold in the first 1.1 build.
  await tab(page, 'Widgets');
  await page.getByRole('button', { name: 'Expand the sidebar' }).click();
  await page.waitForTimeout(600);
  await markOnSelected(page, at('Sidebar expanded'));
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.waitForTimeout(600);
  await markOnSelected(page, at('Sidebar narrow'));
  await page.setViewportSize({ width: 1920, height: 900 });
  await page.waitForTimeout(600);
  await markOnSelected(page, at('Sidebar widened'));
  await ctx.close();
}

async function markOnSelected(page, where) {
  const off = (await page.evaluate(markOff)) ?? Infinity;
  checks++;
  if (off > 1) problem(where, `the sidebar's highlight is ${Math.round(off)} px off the selected view`);
}

// --- Desktop widget windows (P30, 1.2): each kind at its window's size, and a widget's menu. --

async function sweepWidgets(browser, theme) {
  for (const [set, seed] of [
    [WIDGETS, SEED_WIDGETS],
    [LONG, SEED_LONG],
  ]) {
    for (const w of set.widgets) {
      const [width, height] = deskSize(w);
      const { ctx, page } = await open(browser, `${BASE}/#widget=${w.id}`, { width, theme, init: seed });
      await page.setViewportSize({ width, height });
      await page.goto(`${BASE}/#widget=${w.id}`);
      await page.waitForTimeout(1200);
      const at = (what) => `Widget ${w.kind} ${w.id} (${w.style})${what} · ${theme}`;
      // Glass shows the desktop through, which no check can see: its text is drawn with a shadow
      // for any wallpaper, so contrast is left to the field check.
      if (w.style === 'glass') await page.addStyleTag({ content: ':root.widget body { background: #1b2433 }' });
      await axe(page, at(''));
      await textInside(page, at(''));
      if (w.id === 'gauge1') {
        await page.mouse.click(width / 2, height / 2, { button: 'right' });
        await page.getByRole('menu').waitFor();
        await axe(page, at(', menu'));
      }
      await ctx.close();
    }
  }
}

/** Every text in the widget's face lies inside its card; text cut short with "…" counts by its box. */
async function textInside(page, where, sel = '.desk', what = 'the widget') {
  const out = await page.evaluate((sel) => {
    const faces = [...document.querySelectorAll(sel)];
    if (!faces.length) return [`nothing at ${sel}`];
    const bad = [];
    for (const desk of faces) {
      const b = desk.getBoundingClientRect();
      const box = { left: b.left + 1, right: b.right - 1, top: b.top + 1, bottom: b.bottom - 1 };
      const name = faces.length > 1 ? `in "${desk.querySelector('h2')?.textContent ?? desk.textContent.trim().slice(0, 20)}" ` : '';
      const cutBy = (el) => {
        for (let e = el; e && e !== desk; e = e.parentElement) {
          const s = getComputedStyle(e);
          if (s.textOverflow === 'ellipsis' && s.overflowX !== 'visible') return e;
        }
        return null;
      };
      const walk = document.createTreeWalker(desk, NodeFilter.SHOW_TEXT);
      for (let n = walk.nextNode(); n; n = walk.nextNode()) {
        const text = n.textContent.trim();
        const el = n.parentElement;
        if (!text || !el || el.closest('.sr-only') || !el.checkVisibility()) continue;
        const clip = cutBy(el);
        let rects;
        if (clip) rects = [clip.getBoundingClientRect()];
        else if (el instanceof SVGElement) rects = [el.getBoundingClientRect()];
        else {
          const r = document.createRange();
          r.selectNodeContents(n);
          rects = [...r.getClientRects()];
        }
        for (const r of rects) {
          const past = Math.max(r.right - box.right, box.left - r.left, r.bottom - box.bottom, box.top - r.top);
          if (r.width > 0 && past > 0.5) {
            bad.push(`${name}"${text.slice(0, 32)}" is ${Math.ceil(past)} px past the edge`);
            break;
          }
        }
      }
    }
    return bad;
  }, sel);
  checks++;
  for (const b of out) problem(where, `text outside ${what}: ${b}`);
}

/** No word in a Dashboard widget is split across two lines (1.2: in narrow cards values broke a
 * letter or two a line), unless the word alone is wider than its card. Breaks after "-" and "/"
 * are the browser's own; a no-break space holds a word together like a letter. */
async function wordsWhole(page, where) {
  const out = await page.evaluate(() => {
    const bad = [];
    const r = document.createRange();
    for (const cell of document.querySelectorAll('.dash-grid .dash-cell')) {
      const name = cell.querySelector('h2')?.textContent ?? cell.textContent.trim().slice(0, 20);
      // Its widget's own width, not the box the word is in: a box squeezed narrow is the fault.
      const face = cell.querySelector(':scope > .card, :scope > .panel') ?? cell;
      const fs = getComputedStyle(face);
      const room = face.clientWidth - parseFloat(fs.paddingLeft) - parseFloat(fs.paddingRight);
      const walk = document.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
      for (let n = walk.nextNode(); n; n = walk.nextNode()) {
        const el = n.parentElement;
        if (!el || !n.data.trim() || el.closest('.sr-only') || !el.checkVisibility()) continue;
        const s = n.data;
        for (const m of s.matchAll(/[^ \t\n\r]+/g)) {
          let prev = null;
          let split = false;
          for (let i = m.index; i < m.index + m[0].length; i++) {
            r.setStart(n, i);
            r.setEnd(n, i + 1);
            const c = r.getBoundingClientRect();
            if (!c.width) continue;
            if (prev && c.top > prev.top + prev.height / 2 && !'-/–—'.includes(s[i - 1])) split = true;
            prev = c;
          }
          if (!split) continue;
          r.setStart(n, m.index);
          r.setEnd(n, m.index + m[0].length);
          const wide = [...r.getClientRects()].reduce((a, b) => a + b.width, 0);
          if (wide <= room + 1) bad.push(`"${m[0].slice(0, 32)}" in "${name}" is split across lines`);
        }
      }
    }
    return bad;
  });
  checks++;
  for (const b of out) problem(where, b);
}

// --- The Dashboard's packing (1.2): the owner's layout at each width. ------------------------------

async function sweepDashboard(browser, theme, width, shows = 'owner') {
  const { ctx, page } = await open(browser, BASE, { width, theme, init: seedDash(shows) });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  // The preview always runs a game, and its profile hides the detail cards: keep yours.
  await tab(page, 'Settings');
  await page.getByRole('switch', { name: /^Switch profiles for games/ }).uncheck();
  await tab(page, 'Dashboard');
  await page.waitForTimeout(800);
  const where = `Dashboard (${shows}) · ${theme} ${width}px`;
  const found = await page.evaluate(() => {
    const bad = [];
    const cells = [...document.querySelectorAll('.dash-grid .dash-cell')];
    if (cells.length < 10) bad.push(`only ${cells.length} widgets shown`);
    const boxes = cells.map((c) => ({ c, r: c.getBoundingClientRect(), name: c.querySelector('h2')?.textContent ?? c.textContent.trim().slice(0, 20) }));
    // No two widgets on top of each other.
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++) {
        const [a, b] = [boxes[i].r, boxes[j].r];
        if (a.left < b.right - 1 && b.left < a.right - 1 && a.top < b.bottom - 1 && b.top < a.bottom - 1) bad.push(`"${boxes[i].name}" and "${boxes[j].name}" overlap`);
      }
    // A card taller than its layout size is so for its content: no blank stretch under it.
    for (const { c, name } of boxes) {
      const card = c.querySelector(':scope > .card, :scope > .panel');
      if (!card || !card.lastElementChild) continue;
      const min = parseFloat(getComputedStyle(c).minHeight) || 0;
      if (c.offsetHeight <= min + 1) continue;
      const cs = getComputedStyle(card);
      const blank = card.getBoundingClientRect().bottom - parseFloat(cs.paddingBottom) - parseFloat(cs.borderBottomWidth) - card.lastElementChild.getBoundingClientRect().bottom;
      if (blank > 2) bad.push(`"${name}" has ${Math.round(blank)} px of blank under its content`);
    }
    // The thread bars span their card (or are at their widest).
    const ul = document.querySelector('.dash-grid .cores');
    const lis = ul ? [...ul.children] : [];
    if (lis.length) {
      const u = ul.getBoundingClientRect();
      const span = Math.max(...lis.map((l) => l.getBoundingClientRect().right)) - Math.min(...lis.map((l) => l.getBoundingClientRect().left));
      const widest = lis[0].getBoundingClientRect().width >= 7 * parseFloat(getComputedStyle(document.documentElement).fontSize) - 1;
      if (span < u.width * 0.9 && !widest) bad.push(`the thread bars fill ${Math.round((span / u.width) * 100)} % of their card`);
    } else bad.push('no thread bars');
    return bad;
  });
  checks++;
  for (const f of found) problem(where, f);
  await textInside(page, where, '.dash-grid .dash-cell', 'its card');
  await wordsWhole(page, where);
  await noSidewaysScroll(page, where);
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
  // A phone's page: nothing in it scrolls sideways at a phone's width either.
  await noSidewaysScroll(page, where, true);
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

// --- Moving cards (2.0.2): by the grip, by its keys, back on Escape, kept after a reload. --------

async function sweepCards(browser) {
  const { ctx, page } = await open(browser, BASE, { width: 1440, theme: 'midnight' });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  await tab(page, 'Settings');
  const order = () => page.evaluate(() => [...document.querySelectorAll('#panel-settings .card-cell')].map((c) => c.dataset.card));
  const said = () => page.locator('#panel-settings [aria-live]').last().textContent();
  const drag = async (name, onto, { drop = true } = {}) => {
    await page.locator(`#${onto}`).scrollIntoViewIfNeeded();
    await page.getByRole('button', { name: `Move ${name}` }).scrollIntoViewIfNeeded();
    const g = await page.getByRole('button', { name: `Move ${name}` }).boundingBox();
    const t = await page.locator(`#${onto}`).boundingBox();
    if (!g || !t || t.y < 0 || t.y > 900) return problem('Cards', `"${onto}" and ${name}'s grip not both on screen`);
    await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2);
    await page.mouse.down();
    for (let i = 1; i <= 12; i++) {
      await page.mouse.move(g.x + ((t.x + 40 - g.x) * i) / 12, g.y + ((t.y + 40 - g.y) * i) / 12);
      await page.waitForTimeout(30);
    }
    await page.waitForTimeout(300);
    if (drop) await page.mouse.up();
  };
  const first = await order();
  checks++;
  // Dragged by its grip onto the first card, it takes its place.
  await drag('General', 'appearance');
  await page.waitForTimeout(400);
  let now = await order();
  if (now[0] !== 'general') problem('Cards', `dragged onto Appearance, General is at ${now.indexOf('general') + 1}: ${now.join(', ')}`);
  if (!/General: 1 of/.test((await said()) ?? '')) problem('Cards', `after the drag the screen reader hears "${await said()}"`);
  // Its arrow keys move it, and it keeps the focus.
  await page.getByRole('button', { name: 'Move General' }).focus();
  await page.keyboard.press('ArrowDown');
  await page.waitForTimeout(400);
  now = await order();
  if (now[1] !== 'general') problem('Cards', `ArrowDown on its grip left General at ${now.indexOf('general') + 1}`);
  const focused = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'));
  if (focused !== 'Move General') problem('Cards', `after ArrowDown the focus is on ${focused ?? 'nothing'}`);
  // Escape while dragging puts it back.
  const before = await order();
  await drag('Profile', 'appearance', { drop: false });
  const mid = await order();
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await page.waitForTimeout(400);
  if (mid.join() === before.join()) problem('Cards', 'dragging Profile over Appearance moved nothing');
  if ((await order()).join() !== before.join()) problem('Cards', `Escape left ${(await order()).join(', ')}`);
  if (await page.locator('.card-ghost').count()) problem('Cards', 'the dragged card stayed on screen after Escape');
  // Kept after a reload, in the section list too, each card where it was.
  const places = () => page.evaluate(() => [...document.querySelectorAll('#panel-settings .card-cell')].map((c) => `${c.dataset.card} ${c.style.gridColumn} ${c.style.gridRowStart}`).join('; '));
  const left = await places();
  await page.reload();
  await page.waitForTimeout(1200);
  await tab(page, 'Settings');
  now = await order();
  if (now.join() !== before.join()) problem('Cards', `after a reload: ${now.join(', ')}`);
  if ((await places()) !== left) problem('Cards', `after a reload the cards moved: ${await places()}`);
  const toc = await page.locator('#panel-settings nav a').first().textContent();
  if (toc !== 'Appearance') problem('Cards', `the section list starts with ${toc}, not Appearance`);
  // A wider window fits them anew from the owner's order; back at this size they are as left.
  await page.setViewportSize({ width: 2560, height: 900 });
  await page.waitForTimeout(600);
  await cardsPacked(page, 'Cards at 2560 px');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.waitForTimeout(600);
  if ((await places()) !== left) problem('Cards', `back at 1440 px the cards moved: ${await places()}`);
  await page.getByRole('button', { name: 'Put cards back' }).click();
  await page.waitForTimeout(400);
  if ((await order()).join() !== first.join()) problem('Cards', `put back: ${(await order()).join(', ')}`);
  await ctx.close();
}

// --- The narrowest windows (2.0.2): every view, nothing in it scrolls sideways. -----------------
// Processes' table did at 481 to 540 px, its PID column hidden only from 480 down.

async function sweepNarrow(browser) {
  for (const width of [480, 540]) {
    const { ctx, page } = await open(browser, BASE, { width, theme: 'midnight', init: SEED_WIDGETS });
    await page.goto(`${BASE}/`);
    await page.waitForTimeout(1200);
    for (const t of TABS) {
      await tab(page, t);
      await noSidewaysScroll(page, `${t} · narrow ${width}px`);
    }
    await ctx.close();
  }
}

// --- Resizing (2.0.2): the window narrowed and widened again. -----------------------------------
// A view planned for a wide window kept a card on a column a narrower one no longer had, and the
// window scrolled sideways (the owner's report, on a 2000 px screen); the Dashboard's widgets kept
// twelve columns past 900 px. Each view with cards, from 2560 px down to the narrowest window and
// back: nothing scrolls sideways, the cards pack without a gap, the widgets stay inside their grid.
// And the sidebar folded and unfolded, which changes the width without the window.

async function sweepResize(browser) {
  const { ctx, page } = await open(browser, BASE, { width: 2560, theme: 'midnight', init: SEED_WIDGETS });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  for (const t of ['Dashboard', 'Settings', 'Hardware', 'System', 'Storage', 'Network', 'Displays', 'Devices', 'Cooling', 'Gaming']) {
    await page.setViewportSize({ width: 2560, height: 900 });
    await tab(page, t);
    for (const w of [1920, 1600, 1280, 1000, 760, 480, 1100, 1920, 2560]) {
      await page.setViewportSize({ width: w, height: 900 });
      // 150 ms for the plan once the window stops, 180 ms for the cards to glide.
      await page.waitForTimeout(500);
      const where = `Resized · ${t} ${w}px`;
      await noSidewaysScroll(page, where);
      await cardsPacked(page, where);
      await widgetsPacked(page, where);
    }
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  await tab(page, 'Settings');
  for (const name of ['Collapse the sidebar', 'Expand the sidebar']) {
    await page.getByRole('button', { name }).click();
    await page.waitForTimeout(700);
    const where = `Resized · Settings, ${name.toLowerCase()}`;
    await noSidewaysScroll(page, where);
    await cardsPacked(page, where);
  }
  await ctx.close();
}

// --- Ctrl K to a card (2.0.3): the card in view once its view has settled. ----------------------
// On a view's first showing its cards are placed again as their data arrives (Remote's, Updates'),
// and the card scrolled to once, 150 ms after the view opened, was elsewhere a moment later: Units
// 555 px down the window rather than under the top bar, Updates below it. Health's Alerts and
// Diagnostics had no id to scroll to at all. Each from a fresh start: the card's top under the top
// bar, or the page at its end with the card on screen.

async function sweepSections(browser) {
  const SECTIONS = [
    ['Units', 'units', 'Settings'],
    ['Updates', 'updates', 'Settings'],
    ['Hidden sensors', 'hidden-sensors', 'Settings'],
    ['Remote', 'remote', 'Settings'],
    ['Alerts', 'alerts', 'Health'],
    ['Diagnostics', 'diagnostics', 'Health'],
  ];
  for (const [label, id, kind] of SECTIONS) {
    const { ctx, page } = await open(browser, BASE, { width: 1440, theme: 'midnight' });
    await page.goto(`${BASE}/`);
    await page.waitForTimeout(1200);
    const where = `Ctrl K · ${label}`;
    await page.keyboard.press('Control+k');
    // The palette loads on first use, and takes the focus once open.
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'combobox');
    await page.keyboard.type(label);
    await page.waitForTimeout(200);
    const first = await page.locator('#cmdk-0').innerText();
    if (!first.startsWith(label) || !first.includes(kind)) {
      problem(where, `the first result is "${first.replace(/\n/g, ' · ')}"`);
      await ctx.close();
      continue;
    }
    await page.keyboard.press('Enter');
    await page.waitForTimeout(2500);
    checks++;
    const at = await page.evaluate((id) => {
      const el = document.getElementById(id);
      if (!el) return null;
      const end = document.documentElement.scrollHeight - innerHeight;
      const margin = parseFloat(getComputedStyle(el).scrollMarginTop) || 0;
      return { top: el.getBoundingClientRect().top, margin, end: scrollY >= end - 1 };
    }, id);
    if (!at) problem(where, `no element #${id} to scroll to`);
    else if (Math.abs(at.top - at.margin) > 2 && !(at.end && at.top > at.margin && at.top < 900 - 40)) {
      const more = at.end ? '' : ', and the page could scroll further';
      problem(where, `its top is ${Math.round(at.top)} px down the window, not ${Math.round(at.margin)}${more}`);
    }
    await ctx.close();
  }
}

// --- Reduced motion: no CSS animation runs. ----------------------------------------------------

async function sweepMotion(browser) {
  const { ctx, page } = await open(browser, BASE, { width: 1440, theme: 'cyber', motion: 'reduce' });
  await page.goto(`${BASE}/`);
  await page.waitForTimeout(1200);
  for (const t of ['Dashboard', 'Cooling', 'Gaming', 'Widgets', 'Settings']) {
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
      await Promise.all([...WIDTHS, 800].map((w) => sweepDashboard(browser, theme, w)));
      // The narrowest window that keeps 12 columns (cards 3 wide about 195 px), and a wider one.
      await Promise.all([920, 1100].map((w) => sweepDashboard(browser, theme, w, 'narrow')));
      await sweepShell(browser, theme);
      await sweepWidgets(browser, theme);
    }
    await Promise.all(
      [1440, 390].flatMap((w) => [
        sweepCustomAccent(browser, 'midnight', '#1a237e', w),
        sweepCustomAccent(browser, 'daylight', '#fff59d', w),
      ]),
    );
    await sweepKeyboard(browser);
    await sweepCards(browser);
    await sweepNarrow(browser);
    await sweepResize(browser);
    await sweepSections(browser);
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
