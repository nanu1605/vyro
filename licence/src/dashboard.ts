// The owner's dashboard (2.6; the owner, 2026-11-27: "Make an admin panel dashboard where i can
// see keys which are currently in use and also including the system the key is in use and names
// of the person who owns that key. I need a full dashboard."). Signed in with the key page's own
// passkeys; the page holds nothing until then, and every part of it is built from the data with
// textContent: what a PC says of itself comes from anyone's app.

import { IN_USE_DAYS } from './rules';
import { page, PASSKEY_SCRIPT } from './page';

const STYLE = `
  :root { --bar: #2563eb; --grid: rgb(23 32 43 / 10%); --good: #0aac23; --warning: #fbb315; --critical: #dc3c3d; --idle: #8a93a3; }
  @media (prefers-color-scheme: dark) { :root { --bar: #4b8ff0; --grid: rgb(255 255 255 / 10%); --idle: #6b7486; } }
  body { align-content: start; }
  main.full { padding: 24px; }
  .bar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 10px 16px; }
  .bar h1 { margin: 0; }
  .tools { display: flex; flex-wrap: wrap; gap: 8px; }
  .tools button, .row-tools button, #give button { width: auto; margin: 0; padding: 8px 14px; font-size: 0.92rem; }
  .notice { margin: 14px 0 0; padding: 10px 12px; border: 1px solid var(--line); border-radius: 10px; }
  .hero { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 14px; margin: 18px 0 6px; }
  .hero b { font-size: 3.2rem; line-height: 1; font-weight: 700; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(10.5rem, 1fr)); gap: 10px; margin: 14px 0 6px; }
  .tile { padding: 12px 14px; border: 1px solid var(--line); border-radius: 12px; }
  .tile b { display: block; font-size: 1.45rem; line-height: 1.25; }
  .tile span { color: var(--muted); font-size: 0.86rem; }
  .card { margin: 16px 0 0; padding: 14px 16px 10px; border: 1px solid var(--line); border-radius: 12px; }
  .card h2 { margin: 0; }
  .card .muted { margin: 2px 0 8px; }
  #chart { position: relative; min-width: 0; }
  #chart svg { display: block; overflow: visible; }
  #chart .tick { fill: var(--muted); font-size: 11px; }
  #chart .hit { fill: transparent; cursor: default; outline: none; }
  #chart .hit:focus-visible + .mark, #chart .mark.on { opacity: 0.78; }
  #chart .hit:focus-visible { stroke: var(--text); stroke-width: 2px; }
  .tip { position: absolute; z-index: 2; pointer-events: none; padding: 6px 10px; border: 1px solid var(--line); border-radius: 8px;
    background: var(--bg); box-shadow: 0 8px 24px -12px rgb(0 0 0 / 40%); white-space: nowrap; font-size: 0.86rem; }
  .tip b { display: block; font-size: 1rem; }
  .filters { display: flex; flex-wrap: wrap; align-items: end; gap: 10px 12px; margin: 20px 0 8px; }
  .filters label { margin: 0 0 4px; font-size: 0.85rem; }
  .filters input, .filters select { margin: 0; padding: 8px 10px; }
  .filters > div { flex: 0 1 12rem; }
  .filters > div.grow { flex: 1 1 16rem; }
  select { width: 100%; padding: 11px 12px; border: 1px solid var(--line); border-radius: 10px; background: var(--bg); color: var(--text); font: inherit; }
  select:focus-visible { outline: 3px solid var(--accent); outline-offset: 1px; }
  .table { width: 100%; border-collapse: collapse; font-size: 0.92rem; }
  .table th { padding: 8px 10px; border-bottom: 1px solid var(--line); color: var(--muted); font-weight: 600; text-align: left; white-space: nowrap; }
  .table td { padding: 10px; border-bottom: 1px solid var(--line); vertical-align: top; }
  .table td .muted { display: block; margin: 0; font-size: 0.84rem; overflow-wrap: break-word; }
  .table td.owner { min-width: 12rem; }
  .table td.pc { min-width: 14rem; }
  .table td.date { white-space: nowrap; }
  .badge { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; font-weight: 600; }
  .badge::before { content: ''; width: 9px; height: 9px; border-radius: 50%; background: var(--idle); }
  .badge.good::before { background: var(--good); }
  .badge.quiet::before { background: var(--warning); }
  .badge.off::before { background: var(--critical); }
  .mono { font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace; white-space: nowrap; }
  .row-tools { display: flex; flex-wrap: wrap; gap: 6px; }
  .row-tools button { padding: 6px 10px; font-size: 0.84rem; white-space: nowrap; }
  #give { margin-top: 16px; }
  #give .key { text-align: left; }
  .give-row { display: flex; flex-wrap: wrap; gap: 8px 10px; align-items: end; }
  .give-row > div { flex: 1 1 16rem; }
  .give-row input { margin: 0; }
  @media (max-width: 760px) {
    .table thead { display: none; }
    .table, .table tbody, .table tr, .table td { display: block; width: 100%; }
    .table tr { padding: 8px 0; border-bottom: 1px solid var(--line); }
    .table td { padding: 4px 0; border: 0; }
    .table td::before { content: attr(data-label); display: block; color: var(--muted); font-size: 0.78rem; font-weight: 600; }
    .hero b { font-size: 2.6rem; }
    .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    main.full { padding: 18px 14px; }
  }
`;

/** The dashboard: `ready` once a device is set up on the key page, to sign in with. Before the
 * sign-in it says only that: no keys, no names. */
export function dashboardPage(o: { nonce: string; seller: string; ready: boolean }): string {
  const ask = o.ready
    ? `<h1>Sign in</h1>
<p>With the passkey set up on this device.</p>
<form id="go"><button type="submit">Sign in</button></form>`
    : `<h1>Sign in</h1>
<p>No device is set up here yet: set one up first, at <a href="/give">/give</a>.</p>`;
  return page({
    title: 'Clokrix',
    seller: o.seller,
    nonce: o.nonce,
    full: true,
    style: STYLE,
    body: `<div id="ask">${ask}</div>
<div id="dash" hidden>
  <div class="bar">
    <h1>Clokrix Pro keys</h1>
    <div class="tools">
      <button id="refresh" class="quiet" type="button">Refresh</button>
      <button id="give-open" class="quiet" type="button">Give a key</button>
      <button id="csv" class="quiet" type="button">Export CSV</button>
      <button id="out" class="quiet" type="button">Sign out</button>
    </div>
  </div>
  <p id="notice" class="notice muted" hidden></p>
  <form id="give" class="card" hidden>
    <h2>Give a key</h2>
    <p class="muted">A key given, not sold: no payment or receipt goes with it. Signing in again on
    this device gives it.</p>
    <div class="give-row">
      <div><label for="label">Given to</label><input id="label" maxlength="80" autocomplete="off" placeholder="Their name (optional)"></div>
      <button type="submit">Give a key</button>
    </div>
    <div id="given" hidden><code id="given-key" class="key"></code><button id="given-copy" class="quiet" type="button">Copy</button></div>
  </form>
  <div class="hero"><b id="in-use">0</b><span>keys in use now: on a PC that checked in within ${IN_USE_DAYS} days</span></div>
  <div id="tiles" class="tiles"></div>
  <section class="card">
    <h2>Keys sold each month</h2>
    <p class="muted">The last 12 months, bought keys only; refunded ones are not counted.</p>
    <div id="chart"></div>
  </section>
  <div class="filters">
    <div class="grow"><label for="q">Search</label><input id="q" type="search" placeholder="Name, email, phone, PC or key" autocomplete="off"></div>
    <div><label for="status">Status</label><select id="status">
      <option value="">All</option><option value="use">In use</option><option value="quiet">Quiet</option>
      <option value="none">On no PC</option><option value="new">Not turned on yet</option>
      <option value="cancelled">Cancelled</option><option value="refunded">Refunded</option><option value="test">Test, off</option>
    </select></div>
    <div><label for="source">Source</label><select id="source">
      <option value="">All</option><option value="bought">Bought</option><option value="given">Given</option><option value="test">Test keys</option>
    </select></div>
  </div>
  <p id="count" class="muted"></p>
  <table class="table">
    <thead><tr><th>Status</th><th>Owner</th><th>Key</th><th>Bought or given</th><th>PC</th><th>Turned on</th><th>Last seen</th><th></th></tr></thead>
    <tbody id="rows"></tbody>
  </table>
</div>
<p id="done" class="after" role="status"></p>
<p id="err" class="err after" role="alert"></p>
<script nonce="${o.nonce}">
  const said = {
    passkey: 'That sign-in was refused. Try again.',
    pass: 'Signed out: 30 minutes after signing in, or 5 minutes after the last thing done. Sign in again.',
    'not-set-up': 'No device is set up here yet: set one up at /give first.',
    'too-many': 'That was a lot at once: wait a moment, then try again.',
    state: 'That key had changed meanwhile: it shows as it is now.',
    'no-key': 'That key is not here any more.',
  };
${PASSKEY_SCRIPT}
  const IN_USE = ${IN_USE_DAYS * 86400};
  const $ = (id) => document.getElementById(id);
  const err = $('err');
  const done = $('done');
  let pass = null;
  let keys = [];
  let live = false;
  let rz = 'ok';
  const shownKeys = new Set();

  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined && text !== null) e.textContent = text;
    return e;
  };
  const day = (t) => (t ? new Date(t * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : '');
  const money = (amount, currency) => {
    try {
      return new Intl.NumberFormat(undefined, { style: 'currency', currency, maximumFractionDigits: 0 }).format(amount / 100);
    } catch {
      return (amount / 100).toFixed(0) + ' ' + currency;
    }
  };
  const nowS = () => Math.floor(Date.now() / 1000);

  // Each key's state, as a filter names it and as it reads.
  function status(k) {
    if (k.revokedBy === 'refund') return { code: 'refunded', label: 'Refunded', tone: 'off' };
    if (k.revokedBy === 'owner') return { code: 'cancelled', label: 'Cancelled', tone: 'off' };
    if (k.source === 'test' && live) return { code: 'test', label: 'Test, off', tone: 'off' };
    if (k.pc) {
      const seen = k.checked || k.activated || 0;
      return nowS() - seen <= IN_USE ? { code: 'use', label: 'In use', tone: 'good' } : { code: 'quiet', label: 'Quiet', tone: 'quiet' };
    }
    return k.activated ? { code: 'none', label: 'On no PC', tone: 'idle' } : { code: 'new', label: 'Not turned on yet', tone: 'idle' };
  }
  const owner = (k) => {
    if (k.source === 'given') return { name: k.label ? 'Given to ' + k.label : 'Given (no name)', email: null, phone: null };
    const b = k.buyer || {};
    return { name: b.name || k.label || null, email: b.email || null, phone: b.phone || null };
  };
  const pcName = (k) => (k.about && k.about.name) || (k.pc ? 'PC ' + k.pc : '');
  const masked = (key) => key.slice(0, 5) + '…-' + key.slice(-5);

  function took(j) {
    pass = { pass: j.pass, by: j.by, at: j.at };
    if (j.keys) keys = j.keys;
    if (typeof j.live === 'boolean') live = j.live;
    if (j.razorpay) rz = j.razorpay;
  }
  function signedOut(why) {
    pass = null;
    keys = [];
    shownKeys.clear();
    $('given-key').textContent = '';
    $('given').hidden = true;
    $('give').hidden = true;
    $('dash').hidden = true;
    $('ask').hidden = false;
    if (why) err.textContent = why;
  }

  function tiles() {
    const n = (f) => keys.filter(f).length;
    $('in-use').textContent = String(n((k) => status(k).code === 'use'));
    const sold = keys.filter((k) => k.source === 'bought');
    const kept = sold.filter((k) => k.revokedBy !== 'refund');
    const revenue = {};
    for (const k of kept) if (k.amount && k.currency) revenue[k.currency] = (revenue[k.currency] || 0) + k.amount;
    const list = [
      [String(sold.length), 'bought'],
      ...Object.keys(revenue).sort().map((c) => [money(revenue[c], c), 'taken in ' + c + ', refunds left out']),
      [String(n((k) => k.source === 'given')), 'given'],
      [String(n((k) => status(k).code === 'quiet')), 'quiet: on a PC not seen in ' + Math.round(IN_USE / 86400) + ' days'],
      [String(n((k) => status(k).code === 'new')), 'not turned on yet'],
      [String(n((k) => k.revokedBy === 'owner')), 'cancelled'],
      [String(n((k) => k.revokedBy === 'refund')), 'refunded'],
    ];
    const tests = n((k) => k.source === 'test');
    if (tests) list.push([String(tests), live ? 'bought under Test keys: off' : 'bought under Test keys']);
    $('tiles').replaceChildren(
      ...list.map(([v, what]) => {
        const t = el('div', 'tile');
        t.append(el('b', null, v), el('span', null, what));
        return t;
      }),
    );
  }

  // Keys sold each month: one series, so no legend; each bar has its own tooltip, on hover and
  // on keyboard focus, and the table below has every key.
  const NS = 'http://www.w3.org/2000/svg';
  const svgEl = (tag, attrs) => {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, String(attrs[k]));
    return e;
  };
  function chart() {
    const box = $('chart');
    const now = new Date();
    const months = [];
    for (let i = 11; i >= 0; i--) {
      const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
      months.push({
        y: d.getFullYear(),
        m: d.getMonth(),
        n: 0,
        short: d.toLocaleDateString(undefined, { month: 'short' }),
        long: d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' }),
      });
    }
    for (const k of keys) {
      if (k.source !== 'bought' || k.revokedBy === 'refund') continue;
      const d = new Date(k.issued * 1000);
      const b = months.find((x) => x.y === d.getFullYear() && x.m === d.getMonth());
      if (b) b.n++;
    }
    // Its width as laid out without the old drawing, which would hold it wide.
    box.replaceChildren();
    const W = Math.max(240, box.clientWidth || 600);
    const H = 190;
    const top = 22;
    const bottom = 24;
    const left = 30;
    const right = 6;
    const most = Math.max(...months.map((b) => b.n));
    const step = most <= 4 ? 1 : Math.ceil(most / 4);
    const scale = Math.max(step, Math.ceil(most / step) * step);
    const plot = H - top - bottom;
    const band = (W - left - right) / months.length;
    const bw = Math.min(24, band * 0.6);
    const total = months.reduce((a, b) => a + b.n, 0);
    const svg = svgEl('svg', { width: W, height: H, role: 'img', 'aria-label': 'Keys sold each month, the last 12 months: ' + total + ' in all' });
    for (let v = 0; v <= scale; v += step) {
      const y = top + plot - (v / scale) * plot;
      svg.append(svgEl('line', { x1: left, x2: W - right, y1: y, y2: y, stroke: 'var(--grid)', 'stroke-width': 1 }));
      const t = svgEl('text', { x: left - 8, y: y + 4, 'text-anchor': 'end', class: 'tick' });
      t.textContent = String(v);
      svg.append(t);
    }
    const tip = el('div', 'tip');
    tip.hidden = true;
    const every = band >= 34 ? 1 : 2;
    months.forEach((b, i) => {
      const x = left + i * band + (band - bw) / 2;
      const h = (b.n / scale) * plot;
      const yb = top + plot;
      const y = yb - h;
      const hit = svgEl('rect', { x: left + i * band, y: top, width: band, height: plot, class: 'hit', tabindex: 0, 'aria-label': b.long + ': ' + b.n + ' sold' });
      const mark = svgEl('path', {
        class: 'mark',
        fill: 'var(--bar)',
        d: h > 0 ? (() => {
          const r = Math.min(4, h, bw / 2);
          return 'M' + x + ',' + yb + 'L' + x + ',' + (y + r) + 'Q' + x + ',' + y + ' ' + (x + r) + ',' + y + 'L' + (x + bw - r) + ',' + y +
            'Q' + (x + bw) + ',' + y + ' ' + (x + bw) + ',' + (y + r) + 'L' + (x + bw) + ',' + yb + 'Z';
        })() : '',
      });
      const on = () => {
        mark.classList.add('on');
        tip.replaceChildren(el('b', null, b.n + (b.n === 1 ? ' key' : ' keys')), el('span', 'muted', b.long));
        tip.hidden = false;
        const tx = Math.min(Math.max(0, left + i * band + band / 2 - 60), W - 130);
        tip.style.left = tx + 'px';
        tip.style.top = Math.max(0, y - 52) + 'px';
      };
      const off = () => {
        mark.classList.remove('on');
        tip.hidden = true;
      };
      hit.addEventListener('pointerenter', on);
      hit.addEventListener('pointerleave', off);
      hit.addEventListener('focus', on);
      hit.addEventListener('blur', off);
      svg.append(hit, mark);
      if (i % every === (months.length - 1) % every) {
        const t = svgEl('text', { x: left + i * band + band / 2, y: H - 6, 'text-anchor': 'middle', class: 'tick' });
        t.textContent = b.short;
        svg.append(t);
      }
    });
    // The busiest month named on its bar; the rest are in the tooltips and the table.
    if (most > 0) {
      const i = months.findIndex((b) => b.n === most);
      const t = svgEl('text', { x: left + i * band + band / 2, y: top + plot - (most / scale) * plot - 6, 'text-anchor': 'middle', class: 'tick' });
      t.textContent = String(most);
      svg.append(t);
    }
    box.replaceChildren(svg, tip);
  }

  function matches(k) {
    const s = status(k);
    const want = $('status').value;
    if (want && s.code !== want) return false;
    const src = $('source').value;
    if (src && k.source !== src) return false;
    const q = $('q').value.trim().toLowerCase();
    if (!q) return true;
    const o = owner(k);
    const a = k.about || {};
    return [o.name, o.email, o.phone, k.label, k.key, k.pc, a.name, a.cpu, a.gpu, a.windows, a.app, k.payment, k.order]
      .some((v) => v && String(v).toLowerCase().includes(q));
  }

  function cell(label, cls, ...parts) {
    const td = el('td', cls);
    td.dataset.label = label;
    td.append(...parts);
    return td;
  }
  function button(text, work, cancelled) {
    const b = el('button', 'quiet', text);
    b.type = 'button';
    b.addEventListener('click', () => run(b, work, cancelled));
    return b;
  }

  function rows() {
    const shown = keys.filter(matches);
    $('count').textContent = 'Showing ' + shown.length + ' of ' + keys.length + (keys.length === 1 ? ' key' : ' keys');
    $('rows').replaceChildren(
      ...shown.map((k) => {
        const tr = el('tr');
        const s = status(k);
        const o = owner(k);
        const who = el('div');
        who.append(el('b', null, o.name || (rz === 'ok' ? 'No name given' : 'Name not read from Razorpay')));
        if (o.email) who.append(el('span', 'muted', o.email));
        if (o.phone) who.append(el('span', 'muted', o.phone));
        if (k.source === 'bought' && !o.name && rz === 'ok') who.append(el('span', 'muted', 'Bought before names were asked'));
        const keyText = el('span', 'mono', shownKeys.has(k.lic) ? k.key : masked(k.key));
        const keyTools = el('div', 'row-tools');
        keyTools.append(
          button(shownKeys.has(k.lic) ? 'Hide' : 'Show', async () => {
            if (shownKeys.has(k.lic)) shownKeys.delete(k.lic);
            else shownKeys.add(k.lic);
            rows();
          }),
          button('Copy', async () => {
            await navigator.clipboard.writeText(k.key);
            done.textContent = 'Copied.';
          }),
        );
        const src = el('div');
        src.append(el('b', null, k.source === 'given' ? 'Given' : k.source === 'test' ? 'Test key' : k.amount && k.currency ? money(k.amount, k.currency) : 'Bought'));
        src.append(el('span', 'muted', day(k.issued)));
        const pc = el('div');
        if (k.pc) {
          pc.append(el('b', null, pcName(k)));
          const a = k.about;
          if (a) {
            const hw = [a.cpu, a.gpu].filter((x) => x).join(' · ');
            if (hw) pc.append(el('span', 'muted', hw));
            const sw = [a.windows, a.app ? 'Clokrix ' + a.app : ''].filter((x) => x).join(' · ');
            if (sw) pc.append(el('span', 'muted', sw));
            if (a.name) pc.append(el('span', 'muted', 'PC ' + k.pc));
          } else {
            pc.append(el('span', 'muted', 'Its details come once it runs Clokrix 2.6'));
          }
        } else {
          pc.append(el('span', 'muted', 'On no PC'));
        }
        const on = el('div');
        on.append(el('span', null, day(k.activated) || 'Never'));
        if (k.moved) on.append(el('span', 'muted', 'Moved ' + day(k.moved)));
        const seen = el('span', null, k.checked ? day(k.checked) : k.pc ? 'No check-in yet' : 'Never');
        const tools = el('div', 'row-tools');
        if (!k.revokedBy) tools.append(button('Cancel', () => cancelKey(k), ''));
        if (k.revokedBy === 'owner') tools.append(button('Restore', () => act('restore', k, {}, 'Restored: its PC is Pro again at its next check-in, or when the key is entered again.'), ''));
        if (k.pc) tools.append(button('Free from its PC', () => freeKey(k), ''));
        tools.append(button(k.label ? 'Rename' : 'Name', () => nameKey(k), ''));
        tr.append(
          cell('Status', null, el('span', 'badge ' + s.tone, s.label)),
          cell('Owner', 'owner', who),
          cell('Key', null, keyText, keyTools),
          cell('Bought or given', 'date', src),
          cell('PC', 'pc', pc),
          cell('Turned on', 'date', on),
          cell('Last seen', 'date', seen),
          cell('', null, tools),
        );
        return tr;
      }),
    );
  }

  function render() {
    tiles();
    chart();
    rows();
    const n = $('notice');
    n.hidden = rz === 'ok';
    n.textContent =
      rz === 'failed'
        ? 'Razorpay could not be reached: buyers\\u2019 names, emails and phones are missing until Refresh works.'
        : 'This checkout has no Razorpay keys set: buyers are not shown.';
  }

  function show() {
    $('ask').hidden = true;
    $('dash').hidden = false;
    render();
  }

  // An action on one key, with the pass; the key comes back as it now is.
  function update(row) {
    if (!row) return;
    const i = keys.findIndex((x) => x.lic === row.lic);
    const buyer = i >= 0 ? keys[i].buyer : null;
    const next = Object.assign({}, row, { buyer: row.buyer || buyer });
    if (i >= 0) keys[i] = next;
    else keys.unshift(next);
  }
  async function act(what, k, extra, said) {
    try {
      const j = await api('/dashboard/' + what, Object.assign({}, pass, { lic: k.lic }, extra));
      took(j);
      update(j.row);
      render();
      done.textContent = said;
    } catch (x) {
      if (x.code === 'pass') signedOut(x.message);
      else if (x.body && x.body.pass) {
        took(x.body);
        update(x.body.row);
        render();
      }
      throw x;
    }
  }
  const nameOf = (k) => owner(k).name || pcName(k) || masked(k.key);
  async function cancelKey(k) {
    const paid = k.source === 'bought' ? ' Cancelling refunds nothing: refunds are made in Razorpay, which then cancels the key by itself.' : '';
    if (!confirm('Cancel the key of ' + nameOf(k) + '? Its PC turns Free at its next weekly check-in, and it cannot be turned on again unless you restore it.' + paid)) return;
    await act('cancel', k, {}, 'Cancelled: its PC turns Free at its next weekly check-in.');
  }
  async function freeKey(k) {
    if (!confirm('Free the key of ' + nameOf(k) + ' from its PC? That PC turns Free at its next weekly check-in, and the key can then be entered on any PC.')) return;
    await act('free', k, {}, 'Freed: the key can be entered on any PC now.');
  }
  async function nameKey(k) {
    const label = prompt(k.source === 'given' ? 'Who was this key given to?' : 'A name for this key (shown when the buyer gave none):', k.label || '');
    if (label === null) return;
    await act('label', k, { label }, label.trim() ? 'Named.' : 'Name cleared.');
  }

  async function signIn() {
    const j = await api('/dashboard/data', await signature());
    took(j);
    show();
  }
  async function refresh(quiet) {
    if (!pass) return;
    try {
      const j = await api('/dashboard/data', pass);
      took(j);
      render();
      if (!quiet) done.textContent = 'Up to date.';
    } catch (x) {
      if (x.code === 'pass') signedOut(x.message);
      else if (!quiet) throw x;
    }
  }
  async function give() {
    const label = $('label').value;
    const j = await api('/dashboard/give', Object.assign(await signature(), { label }));
    took(j);
    update(j.row);
    render();
    $('given-key').textContent = j.key;
    $('given').hidden = false;
    $('label').value = '';
    done.textContent = 'A new key, given' + (label.trim() ? ' to ' + label.trim() : '') + ': copy it now.';
  }
  function csv() {
    const head = ['Status', 'Name', 'Email', 'Phone', 'Given to', 'Key', 'Source', 'Amount', 'Currency', 'Bought or given', 'PC', 'PC name',
      'Processor', 'Graphics', 'Windows', 'Clokrix', 'Turned on', 'Moved', 'Last seen', 'Cancelled or refunded'];
    const bad = '=+-@' + String.fromCharCode(9, 13);
    const q = (v) => {
      let s = v === null || v === undefined ? '' : String(v);
      if (s && bad.includes(s[0])) s = "'" + s;
      return '"' + s.split('"').join('""') + '"';
    };
    const iso = (t) => (t ? new Date(t * 1000).toISOString().slice(0, 10) : '');
    const lines = [head.map(q).join(',')];
    for (const k of keys.filter(matches)) {
      const b = k.buyer || {};
      const a = k.about || {};
      lines.push([status(k).label, b.name, b.email, b.phone, k.label, k.key, k.source, k.amount ? k.amount / 100 : '', k.currency, iso(k.issued), k.pc,
        a.name, a.cpu, a.gpu, a.windows, a.app, iso(k.activated), iso(k.moved), iso(k.checked), iso(k.revoked)].map(q).join(','));
    }
    const blob = new Blob([lines.join(String.fromCharCode(13, 10))], { type: 'text/csv' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'clokrix-keys-' + new Date().toISOString().slice(0, 10) + '.csv';
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }

  async function run(button, work, cancelled) {
    err.textContent = '';
    done.textContent = '';
    button.disabled = true;
    try {
      await work();
    } catch (x) {
      err.textContent =
        x && x.name === 'NotAllowedError'
          ? (cancelled || 'That did not finish.') + why(x)
          : x && x.name && x.name !== 'Error'
            ? 'That did not work.' + why(x)
            : x.message;
    } finally {
      button.disabled = false;
    }
  }
  const signInCancelled = 'Signing in on this device did not finish: cancelled, timed out, or this device has no passkey for this page.';
  const go = $('go');
  if (go)
    go.addEventListener('submit', (e) => {
      e.preventDefault();
      if (!window.PublicKeyCredential) {
        err.textContent = 'This browser cannot use passkeys.';
        return;
      }
      run(e.target.querySelector('button'), signIn, signInCancelled);
    });
  $('refresh').addEventListener('click', (e) => run(e.target, () => refresh(false), ''));
  $('give-open').addEventListener('click', () => {
    $('give').hidden = !$('give').hidden;
    if (!$('give').hidden) $('label').focus();
  });
  $('give').addEventListener('submit', (e) => {
    e.preventDefault();
    run(e.target.querySelector('button[type=submit]'), give, 'No key was given: signing in did not finish.');
  });
  $('given-copy').addEventListener('click', (e) =>
    run(e.target, async () => {
      await navigator.clipboard.writeText($('given-key').textContent);
      done.textContent = 'Copied.';
    }, ''),
  );
  $('csv').addEventListener('click', csv);
  $('out').addEventListener('click', () => signedOut(''));
  for (const id of ['q', 'status', 'source']) $(id).addEventListener('input', rows);
  let resized = 0;
  window.addEventListener('resize', () => {
    clearTimeout(resized);
    resized = setTimeout(() => pass && chart(), 150);
  });
  // Up to date while it is looked at: every 4 minutes, which also keeps the pass good, until 30
  // minutes after signing in.
  setInterval(() => {
    if (pass && !document.hidden) refresh(true);
  }, 240000);
</script>`,
  });
}
