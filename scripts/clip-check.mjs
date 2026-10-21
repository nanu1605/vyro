// A voice clip pasted into a real browser (1.7): examples/clip_check.rs makes a clip and puts it
// on the clipboard as the app does, then presses Ctrl+V as the phone's Paste key does; this opens
// Google Chrome (CHROME names another; Microsoft Edge if there is no Chrome) on a page that keeps
// what a paste brings. The clip must arrive as a file of its own name and size, as WhatsApp Web
// and other chats take a pasted file. Each finding is a line; on GitHub Actions a notice (or an
// error, which fails the step). Windows only.
//
//   node scripts/clip-check.mjs
import { spawn, spawnSync } from 'node:child_process';
import { basename } from 'node:path';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright';

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const ci = !!process.env.GITHUB_ACTIONS;
let problems = 0;
const ok = (m) => console.log(ci ? `::notice title=Clip check::${m}` : `✓ ${m}`);
const problem = (m) => {
  problems++;
  console.log(ci ? `::error title=Clip check::${m}` : `✗ ${m}`);
};

if (process.platform !== 'win32') {
  console.log('The clipboard check is for Windows.');
  process.exit(0);
}

const built = spawnSync('cargo', ['build', '-q', '-p', 'vyro-core', '--example', 'clip_check'], { cwd: root, stdio: 'inherit', shell: true });
if (built.status !== 0) process.exit(1);
const pc = spawn(`${root}/target/debug/examples/clip_check.exe`, { cwd: root, stdio: ['pipe', 'pipe', 'inherit'] });
const waiting = [];
createInterface({ input: pc.stdout }).on('line', (line) => {
  console.log(`  clip_check: ${line}`);
  for (const w of waiting.splice(0)) w(line);
});
/** The next line from the example that matches. */
const next = (re, ms = 30_000) =>
  new Promise((done, fail) => {
    const t = setTimeout(() => fail(new Error(`no "${re}" from the example in ${ms / 1000} s`)), ms);
    const wait = (line) => {
      const m = re.exec(line);
      if (m) {
        clearTimeout(t);
        done(m);
      } else waiting.push(wait);
    };
    waiting.push(wait);
  });

let browser;
try {
  const [, path, bytes] = await next(/^clip: (.+) (\d+)$/);
  ok(`a ${bytes}-byte clip is on the clipboard: ${basename(path)}`);
  browser = process.env.CHROME
    ? await chromium.launch({ executablePath: process.env.CHROME, headless: false })
    : ((await chromium.launch({ channel: 'chrome', headless: false }).catch(() => null)) ??
      (await chromium.launch({ channel: 'msedge', headless: false })));
  console.log(`browser: ${browser.version()}`);
  const page = await browser.newPage({ viewport: { width: 700, height: 400 } });
  await page.setContent(`<!doctype html><title>Paste here</title>
    <textarea autofocus style="width: 90vw; height: 80vh"></textarea>
    <script>
      window.pasted = null;
      addEventListener('paste', (e) => {
        window.pasted = [...e.clipboardData.files].map((f) => ({ name: f.name, size: f.size, type: f.type }));
      });
    </script>`);
  await page.bringToFront();
  await page.locator('textarea').click();
  pc.stdin.write('paste\n');
  await next(/^pasted$/);
  const files = await page
    .waitForFunction(() => window.pasted, null, { timeout: 10_000 })
    .then((h) => h.jsonValue())
    .catch(() => null);
  if (!files) problem('Ctrl+V brought no paste to the page');
  else if (files.length !== 1) problem(`the paste brought ${files.length} files: ${JSON.stringify(files)}`);
  else {
    const [f] = files;
    if (f.name !== basename(path)) problem(`the pasted file is named ${f.name}, not ${basename(path)}`);
    else if (f.size !== Number(bytes)) problem(`the pasted file is ${f.size} bytes, not ${bytes}`);
    else ok(`the browser receives the pasted clip as a file: ${f.name}, ${f.size} bytes, ${f.type || 'no type'}`);
  }
} catch (e) {
  problem(e.message);
} finally {
  await browser?.close();
  pc.kill();
}
process.exit(problems ? 1 : 0);
