// The app's screen video in a real browser (1.7): examples/screen_check.rs runs the producer the
// app runs (on Windows the monitor through Windows' own encoder; elsewhere a test pattern through
// OpenH264) into a WebRTC session on this machine, and this plays the phone: Google Chrome
// (CHROME names another; Microsoft Edge on Windows if there is no Chrome), which decodes H.264
// as phones do. On Windows the browser's window is on the desktop being sent, showing colour
// bars and a moving block, so the picture has something to show.
//
// It checks that pictures decode from the start, that none is blank, that a still screen keeps
// arriving, and that a whole picture comes when asked for. Each finding is a line; on GitHub
// Actions a notice (or an error, which fails the step).
//
//   node scripts/screen-check.mjs
import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';
import { chromium } from 'playwright';

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const windows = process.platform === 'win32';
const ci = !!process.env.GITHUB_ACTIONS;
let problems = 0;
const ok = (m) => console.log(ci ? `::notice title=Screen check::${m}` : `✓ ${m}`);
const problem = (m) => {
  problems++;
  console.log(ci ? `::error title=Screen check::${m}` : `✗ ${m}`);
};

const built = spawnSync('cargo', ['build', '-q', '-p', 'vyro-core', '--example', 'screen_check'], { cwd: root, stdio: 'inherit', shell: windows });
if (built.status !== 0) process.exit(1);
const exe = `${root}/target/debug/examples/screen_check${windows ? '.exe' : ''}`;

async function browser() {
  const headless = !windows;
  if (process.env.CHROME) return chromium.launch({ executablePath: process.env.CHROME, headless, args: ['--no-sandbox'] });
  for (const channel of ['chrome', 'msedge']) {
    const b = await chromium.launch({ channel, headless }).catch(() => null);
    if (b) return b;
  }
  throw new Error('no Google Chrome or Microsoft Edge here');
}

const b = await browser();
const page = await b.newPage({ viewport: { width: 900, height: 600 } });
await page.setContent(`<!doctype html><style>
  body { margin: 0; height: 100vh; background: linear-gradient(90deg, #fff 0 14%, #ff0 14% 28%, #0ff 28% 42%, #0f0 42% 57%, #f0f 57% 71%, #f00 71% 85%, #00f 85%); }
  #block { position: fixed; top: 40%; left: 0; width: 120px; height: 120px; background: #000; animation: slide 2s linear infinite; }
  body.still #block { animation: none; }
  @keyframes slide { to { left: calc(100% - 120px); } }
  video { position: fixed; right: 0; bottom: 0; width: 320px; height: 180px; opacity: 0; }
</style><div id="block"></div><video autoplay playsinline muted></video>`);
console.log(`browser: ${b.version()}`);
const h264 = await page.evaluate(() => (RTCRtpReceiver.getCapabilities('video')?.codecs ?? []).some((c) => /h264/i.test(c.mimeType)));
if (!h264) {
  problem('this browser does not play H.264');
  await b.close();
  process.exit(1);
}

const offer = await page.evaluate(async () => {
  const p = new RTCPeerConnection();
  window.p = p;
  p.addTransceiver('video', { direction: 'recvonly' });
  window.ch = p.createDataChannel('input');
  p.ontrack = (e) => {
    const v = document.querySelector('video');
    v.srcObject = e.streams[0] ?? new MediaStream([e.track]);
    v.play().catch(() => {});
  };
  await p.setLocalDescription(await p.createOffer());
  await new Promise((done) => {
    if (p.iceGatheringState === 'complete') return done();
    p.onicegatheringstatechange = () => p.iceGatheringState === 'complete' && done();
    setTimeout(done, 3000);
  });
  return p.localDescription.sdp;
});

const child = spawn(exe, [], { cwd: root, stdio: ['pipe', 'pipe', 'inherit'] });
const said = [];
const waiters = [];
createInterface({ input: child.stdout }).on('line', (l) => {
  said.push(l);
  if (!l.startsWith('{')) console.log(`pc: ${l}`);
  for (const w of waiters.splice(0)) if (!w(l)) waiters.push(w);
});
const hear = (re, ms) =>
  new Promise((done, fail) => {
    const hit = said.find((l) => re.test(l));
    if (hit) return done(hit);
    const t = setTimeout(() => fail(new Error(`no "${re}" in ${ms / 1000} s`)), ms);
    waiters.push((l) => {
      if (!re.test(l)) return false;
      clearTimeout(t);
      done(l);
      return true;
    });
  });
child.stdin.write(`${JSON.stringify({ offer })}\n`);
const answer = JSON.parse(await hear(/^\{/, 30_000)).answer;
await page.evaluate((sdp) => window.p.setRemoteDescription({ type: 'answer', sdp }), answer);

/** The video's numbers now, and how much the picture varies (0: one flat colour). */
const look = () =>
  page.evaluate(async () => {
    const s = {};
    (await window.p.getStats()).forEach((r) => {
      if (r.type === 'inbound-rtp' && r.kind === 'video') {
        Object.assign(s, {
          decoded: r.framesDecoded ?? 0,
          keyframes: r.keyFramesDecoded ?? 0,
          fps: r.framesPerSecond ?? 0,
          freezes: r.freezeCount ?? 0,
          plis: r.pliCount ?? 0,
          nacks: r.nackCount ?? 0,
          lost: r.packetsLost ?? 0,
          size: `${r.frameWidth ?? 0}x${r.frameHeight ?? 0}`,
          decoder: r.decoderImplementation ?? '',
        });
      }
    });
    const v = document.querySelector('video');
    if (v.videoWidth) {
      const c = document.createElement('canvas');
      c.width = 64;
      c.height = 36;
      const g = c.getContext('2d', { willReadFrequently: true });
      g.drawImage(v, 0, 0, 64, 36);
      const px = g.getImageData(0, 0, 64, 36).data;
      const luma = [];
      for (let i = 0; i < px.length; i += 4) luma.push(0.299 * px[i] + 0.587 * px[i + 1] + 0.114 * px[i + 2]);
      const mean = luma.reduce((a, b) => a + b, 0) / luma.length;
      s.mean = Math.round(mean);
      s.spread = Math.round(Math.sqrt(luma.reduce((a, b) => a + (b - mean) ** 2, 0) / luma.length));
    }
    return s;
  });
const blank = (s) => s.spread !== undefined && s.spread < 2 && s.mean < 8;

try {
  const started = Date.now();
  await hear(/^connected$/, 30_000);
  ok(`connected in ${((Date.now() - started) / 1000).toFixed(1)} s`);
  let first = null;
  for (let i = 0; i < 100 && !first; i++) {
    await page.waitForTimeout(100);
    const s = await look();
    if (s.decoded > 0) first = s;
  }
  if (!first) throw new Error('no picture decoded within 10 s of connecting');
  ok(`first picture decoded ${((Date.now() - started) / 1000).toFixed(1)} s after the answer: ${JSON.stringify(first)}`);

  await page.waitForTimeout(6000);
  const moving = await look();
  ok(`moving, after 6 s: ${JSON.stringify(moving)}`);
  // A debug build converts slowly (the app's release build keeps up): pictures must flow, not
  // at any rate.
  if (moving.decoded - first.decoded < 10) problem(`only ${moving.decoded - first.decoded} pictures in 6 s of a moving screen`);
  if (blank(moving)) problem('the picture is blank');
  if (moving.freezes > 2) problem(`${moving.freezes} freezes on a moving screen`);

  if (windows) await page.evaluate(() => document.body.classList.add('still'));
  else child.stdin.write('still\n');
  await page.waitForTimeout(1500);
  const settled = await look();
  await page.waitForTimeout(5000);
  const still = await look();
  ok(`still, after 6.5 s: ${JSON.stringify(still)}`);
  if (still.decoded - settled.decoded < 3) problem(`a still screen: only ${still.decoded - settled.decoded} pictures in 5 s (one a second is due)`);
  if (blank(still)) problem('the picture went blank on a still screen');

  await page.evaluate(() => window.ch.send(JSON.stringify({ t: 'picture' })));
  await page.waitForTimeout(3000);
  const asked = await look();
  if (asked.keyframes > still.keyframes) ok(`a whole picture came when asked for (${asked.keyframes} keyframes)`);
  else problem(`no keyframe came within 3 s of asking (${asked.keyframes} keyframes)`);
} catch (e) {
  problem(e.message);
} finally {
  child.stdin.write('q\n');
  setTimeout(() => child.kill(), 2000).unref();
  await b.close();
}
console.log(problems ? `Screen check: ${problems} problem(s).` : 'Screen check: all good.');
process.exit(problems ? 1 : 0);
