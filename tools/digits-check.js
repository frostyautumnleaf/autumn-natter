// The cursor uncovers a field of random digits in the background of the chat.
//
// The feature is a Chatting setting, and it remains on. The numbers stand in rows and columns,
// they are of the colour of the leaf mark, and only the soft round aura about the pointer
// uncovers them: the edge of the aura is feathered, so the numbers fade out instead of ending at
// a circle, and the aura breathes while the pointer stands still. This check asks that the field
// is uncovered where the pointer is and nowhere else, that it is gone when the pointer has left
// the chat, and that the setting takes it away altogether.
//
//   node digits-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node digits-check.js
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The state of the engine, as the record of web::state: one chat with a few messages, so the
// place the chat is in has a height to uncover digits over.
function stateOf() {
  const messages = [];
  for (let n = 1; n <= 6; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'The ' + n + 'st thing I asked about.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
      when: '09:0' + n });
    messages.push({ id: 'a' + n, role: 'Assistant', body: 'And the answer I gave, which is long '
      + 'enough to fill the place of the chat with words that the digits must not stand over.',
      reasoning: '', reasoning_ms: 0, when: '09:0' + n });
  }
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'The digits under the cursor' },
    chats: [{ id: 'c1', title: 'The digits under the cursor', repo: '', when: 'Today 09:04',
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
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  // The state is kept where a check may turn one setting of it over, for the page reads the
  // settings out of the state again and again and would put the switch back.
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

// What the canvas of the field holds: how much of it is uncovered, and where. The counts are of
// squares of the canvas that carry any of a digit at all.
const READ = function (x, y) {
  const canvas = document.getElementById('digits');
  if (!canvas) { return { where: 'no canvas', near: -1, far: -1, edge: -1 }; }
  const place = canvas.getBoundingClientRect();
  const view = canvas.getContext('2d');
  const dpr = canvas.width / Math.max(1, place.width);
  const half = Math.round(30 * dpr);
  const whole = view.getImageData(0, 0, canvas.width, canvas.height);
  const seen = new Uint8Array(canvas.width * canvas.height);
  for (var place_ = 0; place_ < whole.data.length; place_ += 4) {
    if (whole.data[place_ + 3] > 6) { seen[place_ / 4] = 1; }
  }
  // A square about the cursor, one well out on the other side of the chat, and one just inside
  // the edge of the aura where the feather has to have something to fade.
  const at = function (px, py) {
    const cx = Math.round(px * dpr);
    const cy = Math.round(py * dpr);
    const box = Math.round(24 * dpr);
    let found = 0;
    for (let dy = -box; dy <= box; dy += 2) {
      for (let dx = -box; dx <= box; dx += 2) {
        const gx = cx + dx, gy = cy + dy;
        if (gx < 0 || gy < 0 || gx >= canvas.width || gy >= canvas.height) { continue; }
        found += seen[gy * canvas.width + gx];
      }
    }
    return found;
  };
  return {
    where: 'read',
    near: at(x, y),
    far: at(Math.max(30, place.width - 30), Math.max(30, place.height - 30)),
    edge: at(x + 143, y),
    lit: window.digits ? window.digits.lit() : -1,
    ask: window.digits ? window.digits.want : null,
  };
};

// The colour the field was drawn with. The most opaque pixel of the canvas is a pixel of one
// digit as it was filled, so it is read back against the tone the page says it used, which is
// the leaf colour of the theme the page is drawn in.
const TONE = function () {
  const canvas = document.getElementById('digits');
  if (!canvas) { return { where: 'no canvas', most: -1, off: 999 }; }
  const whole = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height);
  let most = -1;
  let rgb = null;
  for (let at = 0; at < whole.data.length; at += 4) {
    if (whole.data[at + 3] > most) {
      most = whole.data[at + 3];
      rgb = [whole.data[at], whole.data[at + 1], whole.data[at + 2]];
    }
  }
  const leaf = getComputedStyle(document.body).getPropertyValue('--leaf').trim();
  const made = /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.exec(leaf);
  let want = null;
  if (made) {
    let hex = made[1];
    if (hex.length === 3) { hex = hex[0] + hex[0] + hex[1] + hex[1] + hex[2] + hex[2]; }
    want = [parseInt(hex.slice(0, 2), 16), parseInt(hex.slice(2, 4), 16),
      parseInt(hex.slice(4, 6), 16)];
  }
  const off = (rgb && want)
    ? Math.max(Math.abs(rgb[0] - want[0]), Math.abs(rgb[1] - want[1]), Math.abs(rgb[2] - want[2]))
    : 999;
  return {
    where: 'read', most: most, rgb: rgb, off: off, leaf: leaf,
    tone: (window.digits && window.digits.tone) || [],
  };
};

async function run(page, file, name) {
  await page.setViewport({ width: 1280, height: 860 });
  await page.evaluateOnNewDocument(stubs, stateOf());
  await page.goto('file://' + file, { waitUntil: 'domcontentloaded' });
  await sleep(700);

  // The field is not uncovered before the cursor comes into the chat. The place the chat is in is
  // read at the middle of it and at the corner furthest away.
  const blank = await page.evaluate(function () {
    const place = document.getElementById('stage').getBoundingClientRect();
    return { width: place.width, height: place.height };
  });
  let seen = await page.evaluate(READ, Math.round(blank.width * 0.4), Math.round(blank.height * 0.4));
  if (seen.near !== 0 || seen.far !== 0) {
    throw new Error(name + ': digits stood before the cursor came in: ' + JSON.stringify(seen));
  }

  // The cursor rests in the middle of the chat, and the numbers come up about it.
  const box = await page.evaluate(function () {
    const place = document.getElementById('stage').getBoundingClientRect();
    return { left: place.left, top: place.top, width: place.width, height: place.height };
  });
  const at = { x: Math.round(box.left + box.width * 0.35), y: Math.round(box.top + box.height * 0.45) };
  const under = { x: at.x - box.left, y: at.y - box.top };
  await page.mouse.move(at.x - 40, at.y - 30);
  await page.mouse.move(at.x, at.y);
  await sleep(1300);
  seen = await page.evaluate(READ, under.x, under.y);
  if (!(seen.near > 12)) {
    throw new Error(name + ': no digits came up about the cursor: ' + JSON.stringify(seen));
  }
  if (seen.far !== 0) {
    throw new Error(name + ': the field is uncovered away from the cursor too: ' + JSON.stringify(seen));
  }
  if (!(seen.edge >= 0 && seen.edge < seen.near)) {
    throw new Error(name + ': the edge of the aura is not feathered: ' + JSON.stringify(seen));
  }
  if (!(seen.lit > 0.9)) {
    throw new Error(name + ': the reveal never came to full: ' + JSON.stringify(seen));
  }

  // The aura breathes while the cursor stands still, and the middle of it wanders: two reads a
  // few hundred milliseconds apart are not at the same size. The check of the breathing itself is
  // the screenshot taken here, which is looked at; what is checked here is that the reveal held
  // to full over the while the cursor rested.
  await page.screenshot({ path: path.join(__dirname, '..', '..', 'k16-' + (process.env.AN_PAGE
    ? 'web' : 'window') + '-' + name.replace(/[^a-z]+/gi, '') + '.png') });

  // The colour of the digits is the leaf colour of the theme the page is drawn in, so the silver
  // skin and the corporate skin uncover numbers of grey along with the rest of their screens.
  // Every skin is put on over the same field and read back.
  const skinWord = await page.evaluate(function () { return window.themeWord; });
  for (const one of ['light', 'dark', 'silver', 'corporate', 'neon']) {
    // The state is turned over first, for the page reads the theme out of the state it polls and
    // would put the old one back on within a poll of the skin it was just given.
    await page.evaluate(function (word) {
      window.__state.theme = word;
      window.showTheme(word);
    }, one);
    await sleep(260);
    if (process.env.AN_SHOT) {
      await page.screenshot({ path: path.join(__dirname, '..', '..', 'k16-skin-' + one + '.png') });
    }
    const tone = await page.evaluate(TONE);
    if (tone.most < 20) {
      throw new Error(name + ': nothing of the field was drawn in the ' + one + ' skin: '
        + JSON.stringify(tone));
    }
    if (tone.off > 2) {
      throw new Error(name + ': the digits are not of the leaf colour of the ' + one + ' skin: '
        + JSON.stringify(tone));
    }
  }
  await page.evaluate(function (word) {
    window.__state.theme = word;
    window.showTheme(word);
  }, skinWord);
  await sleep(260);

  // The cursor leaves the chat, and the field is put away with it.
  await page.mouse.move(4, 4);
  await page.evaluate(function () { if (window.digitsAway) { window.digitsAway(); } });
  await sleep(1800);
  seen = await page.evaluate(READ, under.x, under.y);
  if (seen.near !== 0 || seen.far !== 0) {
    throw new Error(name + ': digits remained after the cursor went: ' + JSON.stringify(seen));
  }

  // The setting takes the field away altogether: a pointer over the chat uncovers nothing. The
  // state is turned over first, for the page reads the settings out of the state again.
  await page.evaluate(function () {
    window.__state.chatting.cursor_digits = false;
  });
  await sleep(1500);
  const back = { x: Math.round(box.left + box.width * 0.5), y: Math.round(box.top + box.height * 0.5) };
  await page.mouse.move(back.x - 20, back.y);
  await page.mouse.move(back.x, back.y);
  await sleep(700);
  seen = await page.evaluate(READ, Math.round(back.x - box.left), Math.round(back.y - box.top));
  if (seen.near !== 0) {
    throw new Error(name + ': the field came back with the setting off: ' + JSON.stringify(seen));
  }
  return { name: name, near: 'ok' };
}

(async function main() {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  const failures = [];
  for (const one of PAGES) {
    const page = await browser.newPage();
    page.on('pageerror', function (e) { failures.push(one.name + ': ' + e.message); });
    page.on('console', function (m) {
      // The pages name a mark and a few files of their own that no folder of the test holds, so
      // a resource that is not there is not a fault of the page.
      if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) {
        failures.push(one.name + ': console: ' + m.text());
      }
    });
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
  console.log('the digits under the cursor are uncovered where the cursor is');
})();
