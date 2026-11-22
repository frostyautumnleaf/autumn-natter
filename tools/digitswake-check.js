// The digits under the cursor follow the cursor, and the patch they were uncovered in closes.
//
// The field of digits is uncovered by the aura of the cursor. That aura used to be stamped at the
// place the pointer events gave it, and to take the whole of its reveal away from the place it was
// at in the same frame: a moved cursor read as a row of separate patches, each opened and shut at
// once. Now the aura comes to the cursor over a few tens of milliseconds, and leaves the places it
// stood at behind it, each of which closes over half a second. This check moves the cursor a long
// way across the chat in one go, and asks that the aura is on its way rather than at the cursor in
// the first frame after the jump, that the place the cursor was uncovered in still holds digits
// while the wake closes, and that the same place is left plain again when the wake is made an end
// of. The check of the look of it is the screenshot taken here, which is looked at.
//
// Both pages are checked: the window of the program and the page served by --remote carry the
// same rules.
//
//   node digitswake-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node digitswake-check.js
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The state of the engine, as the record of web::state, with the field of digits asked for.
function stateOf() {
  const messages = [];
  for (let n = 1; n <= 5; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'The thing I asked, number ' + n + '.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
      when: '09:0' + n });
    messages.push({ id: 'a' + n, role: 'Assistant', body: 'And the answer I gave, which is long '
      + 'enough to fill the place of the chat with words.', reasoning: '', reasoning_ms: 0,
      when: '09:0' + n });
  }
  return {
    rev: 1, unchanged: false, theme: 'dark', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'The wake of the cursor', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'The wake of the cursor', repo: '', when: 'Today 09:04',
      active: true, preview: 'the answer' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: false, highlight: true, chapters: true,
      paste_limit: 6000, background: 'none', rain_splash: true, cursor_digits: true },
    context: { used: '4,304', total: '8,192', ratio: 0.5 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: true, data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  window.__state = state;
  window.fetch = function (url) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(state); } });
    }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command) {
        if (command === 'get_state') { return Promise.resolve(state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// How much of the field is uncovered about a place of the canvas, and how far the aura is from the
// cursor it is following. `found` counts the squares of the canvas that carry any of a digit.
const READ = function (x, y) {
  const canvas = document.getElementById('digits');
  if (!canvas) { return { where: 'no canvas', found: -1 }; }
  const view = canvas.getContext('2d');
  const dpr = canvas.width / Math.max(1, canvas.getBoundingClientRect().width);
  const whole = view.getImageData(0, 0, canvas.width, canvas.height);
  const cx = Math.round(x * dpr);
  const cy = Math.round(y * dpr);
  const box = Math.round(22 * dpr);
  let found = 0;
  for (let dy = -box; dy <= box; dy += 2) {
    for (let dx = -box; dx <= box; dx += 2) {
      const gx = cx + dx, gy = cy + dy;
      if (gx < 0 || gy < 0 || gx >= canvas.width || gy >= canvas.height) { continue; }
      if (whole.data[((gy * canvas.width) + gx) * 4 + 3] > 6) { found += 1; }
    }
  }
  const follow = window.digits
    ? Math.round(Math.hypot(window.digits.x - window.digits.fx, window.digits.y - window.digits.fy))
    : -1;
  const behind = (window.digits && window.digits.trail)
    ? window.digits.trail.map(function (one) { return Math.round(one.x) + ',' + Math.round(one.y); })
    : [];
  return { where: 'read', found: found, follow: follow, lit: window.digits ? window.digits.lit() : -1,
    behind: behind };
};

async function run(page, file, name) {
  await page.setViewport({ width: 1280, height: 860 });
  await page.evaluateOnNewDocument(stubs, stateOf());
  await page.goto('file://' + file, { waitUntil: 'domcontentloaded' });
  await sleep(700);
  const box = await page.evaluate(function () {
    const place = document.getElementById('stage').getBoundingClientRect();
    return { left: place.left, top: place.top, width: place.width, height: place.height };
  });
  // The two places the cursor is put at, as far apart as the chat is wide, so the wake the cursor
  // leaves behind is well out of the aura that follows it.
  const before = { x: Math.round(box.left + box.width * 0.24),
    y: Math.round(box.top + box.height * 0.52) };
  const after = { x: Math.round(box.left + box.width * 0.78),
    y: Math.round(box.height * 0.52 + box.top) };
  if (after.x - before.x < 320) {
    throw new Error(name + ': the place of the chat is too narrow to leave a wake in');
  }
  const under = { x: before.x - box.left, y: before.y - box.top };
  const away = { x: after.x - box.left, y: after.y - box.top };

  // The cursor rests, and the numbers come up about it.
  await page.mouse.move(before.x - 30, before.y - 20);
  await page.mouse.move(before.x, before.y);
  await sleep(1300);
  let seen = await page.evaluate(READ, under.x, under.y);
  if (!(seen.found > 12)) {
    throw new Error(name + ': no digits came up about the cursor before it moved: '
      + JSON.stringify(seen));
  }

  // The cursor goes a long way in one go. The aura is on its way after a few frames, and it is at
  // the cursor within half a second. The place the cursor was at still holds its digits by then,
  // which is the wake of the aura, and that same place is left plain again when the wake is made
  // an end of.
  const gone = Date.now();
  await page.mouse.move(after.x, after.y, { steps: 1 });
  await sleep(90);
  const moved = await page.evaluate(READ, away.x, away.y);
  const left = await page.evaluate(READ, under.x, under.y);
  if (!(moved.follow > 24)) {
    throw new Error(name + ': the aura was at the cursor the moment the cursor jumped: '
      + JSON.stringify(moved));
  }
  if (!(moved.lit > 0.9)) {
    throw new Error(name + ': the reveal went down while the cursor was moved: ' + JSON.stringify(moved));
  }
  if (!(left.found > 0)) {
    throw new Error(name + ': the place the cursor left was covered up at once: '
      + JSON.stringify(left));
  }
  await page.screenshot({ path: path.join(__dirname, '..', '..', 'wake-' + (process.env.AN_PAGE
    ? 'web' : 'window') + '-' + name.replace(/[^a-z]+/gi, '') + '.png') });
  await sleep(380);
  const settled = await page.evaluate(READ, away.x, away.y);
  if (settled.follow > 2) {
    throw new Error(name + ': the aura never came to the cursor: ' + JSON.stringify(settled));
  }
  const spent = Date.now() - gone;
  if (spent < 780) { await sleep(790 - spent); }
  const shut = await page.evaluate(READ, under.x, under.y);
  if (shut.found !== 0) {
    throw new Error(name + ': the wake never closed: ' + JSON.stringify(shut));
  }
  return { name: name, wake: 'ok' };
}

(async function main () {
  const browser = await puppeteer.launch({
    headless: 'shell', executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const failures = [];
  for (const one of PAGES) {
    const page = await browser.newPage();
    page.on('pageerror', function (e) { failures.push(one.name + ': ' + e.message); });
    try {
      const report = await run(page, one.file, one.name);
      console.log('ok', report.name);
    } catch (e) {
      failures.push(one.name + ': ' + e.message);
    }
    await page.close();
  }
  await browser.close();
  if (failures.length) {
    failures.forEach(function (line) { console.log(line); });
    process.exit(1);
  }
  console.log('the aura follows the cursor, and the patch it was uncovered in closes behind it');
})();
