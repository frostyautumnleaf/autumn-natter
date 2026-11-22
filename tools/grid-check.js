// The faint grid the background of the chat is drawn with.
//
// The grid is a Chatting setting, and it is on: the background of the chat is a field of hairlines
// in the colour of the theme instead of a solid colour. Every line at rest is one strength, a step
// off the background; now and then one line goes into a slow oscillation, drawn as a wave whose two
// ends keep to their places. This check asks that the grid is drawn at all, that its lines stand
// where the grid says they stand, that it is drawn in the colours of the theme and never stronger
// than a hairline of the interface, that a line in its oscillation really waves, that the pixel-art
// form of a theme keeps its own background, and that the setting puts the solid colour back.
//
//   node grid-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node grid-check.js
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

// The state of the engine, as the record of web::state: a chat with enough messages that the place
// the chat is in has a height for the grid to be drawn over.
function stateOf() {
  const messages = [];
  for (let n = 1; n <= 6; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'The ' + n + 'st thing I asked about.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
      when: '09:0' + n });
    messages.push({ id: 'a' + n, role: 'Assistant', body: 'And the answer I gave, long enough to '
      + 'fill the place of the chat with words the grid must sit under.',
      reasoning: '', reasoning_ms: 0, when: '09:0' + n });
  }
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'The grid of the background' },
    chats: [{ id: 'c1', title: 'The grid of the background', repo: '', when: 'Today 09:04',
      active: true, preview: 'the answer' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: false, highlight: true, chapters: true,
      paste_limit: 6000, background: 'none', rain_splash: true, cursor_digits: false, grid: true },
    context: { used: '4,304', total: '8,192', ratio: 0.5 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
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

// Everything one reading of the canvas can say: how much of it is drawn on, at what strength, in
// what colour, where the lines of it stand along one row, and how far apart they are.
const READ = function (row) {
  const canvas = document.getElementById('gridbg');
  if (!canvas) { return { where: 'no canvas' }; }
  const place = canvas.getBoundingClientRect();
  const view = canvas.getContext('2d');
  const dpr = canvas.width / Math.max(1, place.width);
  const whole = view.getImageData(0, 0, canvas.width, canvas.height);
  let painted = 0;
  let most = 0;
  let rgb = null;
  for (let at = 0; at < whole.data.length; at += 4) {
    const strength = whole.data[at + 3];
    if (strength > 0) { painted++; }
    if (strength > most) {
      most = strength;
      rgb = [whole.data[at], whole.data[at + 1], whole.data[at + 2]];
    }
  }
  // The columns of the canvas that carry any of a line, along the row the caller named.
  const y = Math.max(0, Math.min(canvas.height - 1, Math.round(row * dpr)));
  const lines = [];
  for (let x = 0; x < canvas.width; x++) {
    if (whole.data[(y * canvas.width + x) * 4 + 3] > 0) { lines.push(x); }
  }
  const gaps = [];
  for (let at = 1; at < lines.length; at++) { gaps.push(lines[at] - lines[at - 1]); }
  gaps.sort(function (a, b) { return a - b; });
  // The middle gap, and not the least one: a line in its oscillation lies nearly along a row here
  // and there, and its few columns next to one another are not a failure of the field.
  const middle = gaps.length ? gaps[Math.floor(gaps.length / 2)] : -1;
  const token = getComputedStyle(document.body).getPropertyValue('--grid').trim();
  return {
    where: 'read',
    painted: painted,
    most: most,
    rgb: rgb,
    dpr: dpr,
    lines: lines.length,
    gap: middle,
    cell: (window.GRID_CELL || 0) * dpr,
    token: token,
    want: window.grid ? !!window.grid.want : null,
    waves: window.grid && window.grid.waves ? window.grid.waves.length : -1,
    asked: window.grid ? window.gridAsked() : null,
    moving: window.grid ? window.gridMoves() : null,
  };
};

// The colour the grid was drawn in, read back against the token of the theme.
const TONE = function () {
  const canvas = document.getElementById('gridbg');
  if (!canvas) { return { where: 'no canvas', off: 999 }; }
  const whole = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height);
  let most = 0;
  let rgb = null;
  for (let at = 0; at < whole.data.length; at += 4) {
    if (whole.data[at + 3] > most) {
      most = whole.data[at + 3];
      rgb = [whole.data[at], whole.data[at + 1], whole.data[at + 2]];
    }
  }
  const token = getComputedStyle(document.body).getPropertyValue('--grid').trim();
  let made = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/i.exec(token);
  if (!made) { return { where: 'read', most: most, rgb: rgb, off: 999, token: token }; }
  const want = [Number(made[1]), Number(made[2]), Number(made[3])];
  // The strength of the line is low, and a canvas keeps a faint colour as a rounded multiple of
  // it, so a few steps either way are the drawing and not the wrong colour. What is being asked
  // for is that the line is of the theme in use, which is tens of steps away between any two.
  const off = (!rgb || !want) ? 999
    : Math.max(Math.abs(rgb[0] - want[0]), Math.abs(rgb[1] - want[1]), Math.abs(rgb[2] - want[2]));
  return { where: 'read', most: most, rgb: rgb, off: off, token: token, want: want };
};

// The shape of one line in its oscillation, read off the canvas: how far out of the column it
// stands in at three heights - near the top, in the middle, and near the bottom. A line at rest
// keeps to its column at every height. A line in its oscillation is furthest out in the middle,
// where the wave is biggest, and at nothing where it meets the two ends of the place the chat is
// in. The line swinging furthest out of straight is the one looked at.
const WAVY = function () {
  const canvas = document.getElementById('gridbg');
  if (!canvas) { return { where: 'no canvas', waves: 0, spread: 0 }; }
  const waves = (window.grid && window.grid.waves) || [];
  const out = { where: 'read', waves: waves.length, found: -1, spread: 0,
    topWorst: 0, topCount: 0, bottomWorst: 0, bottomCount: 0 };
  if (!waves.length) { return out; }
  const clock = window.performance ? performance.now() : Date.now();
  let pick = 0;
  let bestEase = -1;
  for (let at = 0; at < waves.length; at++) {
    const age = Math.max(0, Math.min(1, (clock - waves[at].born) / waves[at].life));
    const ease = Math.sin(Math.PI * age);
    if (ease > bestEase) { bestEase = ease; pick = at; }
  }
  const one = waves[pick];
  out.found = pick;
  out.ease = bestEase;
  out.axis = one.axis;
  if (one.axis === 'h' && one.at * window.GRID_CELL > canvas.height * 0.5) {
    // A line lying down that was taken far down the chat has little of itself over the words, and
    // the two bands of the middle and the bottom fall together. It is left for the next look.
    out.found = -1;
  }
  const place = canvas.getBoundingClientRect();
  const dpr = canvas.width / Math.max(1, place.width);
  const whole = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height);
  const upright = one.axis === 'v';
  const rest = one.at * window.GRID_CELL + 0.5;      // where the line stands at rest, in CSS pixels
  const home = rest * dpr;
  const reach = 20 * dpr;                            // less than half a cell, and no other line
  const cell = window.GRID_CELL * dpr;
  const span = upright ? place.height : place.width;
  // How far out of its column the painted line is at one height along it, in CSS pixels. Where a
  // line of the other way of the grid crosses, the sample is dropped: its hairline would be read
  // for the line itself.
  const off = function (share) {
    const along = share * span * dpr;
    const near = ((along % cell) + cell) % cell;
    if (near < 1.5 || near > cell - 1.5) { return null; }
    let found = null;
    for (let step = -reach; step <= reach; step++) {
      const x = upright ? home + step : along;
      const y = upright ? along : home + step;
      if (x < 0 || y < 0 || x >= canvas.width || y >= canvas.height) { continue; }
      if (whole.data[(Math.round(y) * canvas.width + Math.round(x)) * 4 + 3] > 0) {
        if (found === null || Math.abs(step) < Math.abs(found)) { found = step; }
      }
    }
    return found === null ? null : found / dpr;
  };
  const band = function (from, to, named) {
    let worst = 0;
    let count = 0;
    for (let share = from; share <= to; share += 0.004) {
      const value = off(share);
      if (value === null) { continue; }
      count++;
      if (Math.abs(value) > Math.abs(worst)) { worst = value; }
      if (named === 'mid' && Math.abs(value) > Math.abs(out.spread)) { out.spread = value; }
    }
    out[named + 'Worst'] = worst;
    out[named + 'Count'] = count;
  };
  band(0.004, 0.03, 'top');
  band(0.42, 0.58, 'mid');
  band(0.97, 0.996, 'bottom');
  out.spread = Math.abs(out.spread);
  return out;
};

async function run(page, file, name) {
  await page.setViewport({ width: 1280, height: 860 });
  await page.evaluateOnNewDocument(stubs, stateOf());
  await page.goto('file://' + file, { waitUntil: 'domcontentloaded' });
  // The page is not drawn until it has seen the state, and the grid is not drawn until the page
  // has been drawn: the first look of a check that does not wait for it finds an empty canvas.
  for (let try_ = 0; try_ < 40; try_++) {
    const ready = await page.evaluate(function () {
      return !!(window.cur && window.grid && window.grid.ctx && window.grid.w > 1);
    });
    if (ready) { break; }
    await sleep(250);
  }
  await sleep(700);
  if (process.env.AN_DBG) {
    console.log('EARLY ' + name + ' ' + JSON.stringify(await page.evaluate(function () {
      return { actions: (window.__actions || []).length, cls: document.body.className,
        col: (document.getElementById('col') || {}).childElementCount,
        drawn: typeof window.draw, set: typeof window.gridSetting,
        w: window.grid && window.grid.w, raf: window.grid && window.grid.raf,
        waves: window.grid && window.grid.waves && window.grid.waves.length,
        stage: getComputedStyle(document.getElementById('stage')).display,
        cw: document.getElementById('gridbg').width };
    })));
  }

  // The canvas is the deepest thing in the place the chat is in: it is there, it is in the stage,
  // it takes no pointer, and it is laid down before the field of digits, so the digits the cursor
  // uncovers come over it and not under it.
  const laid = await page.evaluate(function () {
    const canvas = document.getElementById('gridbg');
    const digits = document.getElementById('digits');
    const stage = document.getElementById('stage');
    if (!canvas || !digits || !stage) { return { where: 'no canvas' }; }
    const look = getComputedStyle(canvas);
    return {
      inStage: canvas.parentNode === stage,
      before: !!(canvas.compareDocumentPosition(digits) & Node.DOCUMENT_POSITION_FOLLOWING),
      events: look.pointerEvents,
      z: look.zIndex,
      sized: canvas.width > 1 && canvas.height > 1,
    };
  });
  if (!laid.inStage || !laid.before || laid.events !== 'none' || !laid.sized) {
    throw new Error(name + ': the grid is not laid under the chat: ' + JSON.stringify(laid));
  }

  // The setting asks for the grid, so the field is drawn: a row of the chat has the lines of it
  // in it, one device pixel over, a cell apart, and at a strength no stronger than a hairline.
  const mid = await page.evaluate(function () {
    const place = document.getElementById('stage').getBoundingClientRect();
    return Math.round(place.height * 0.5);
  });
  let seen = await page.evaluate(READ, mid);
  if (!(seen.painted > 0)) {
    throw new Error(name + ': nothing of the grid was drawn: ' + JSON.stringify(seen));
  }
  if (!(seen.lines > 8)) {
    throw new Error(name + ': too few lines of the grid: ' + JSON.stringify(seen));
  }
  if (Math.abs(seen.gap - seen.cell) > 1.5) {
    throw new Error(name + ': the lines are not a cell apart: ' + JSON.stringify(seen));
  }
  if (seen.most > 90) {
    throw new Error(name + ': the grid is stronger than a hairline may be: ' + JSON.stringify(seen));
  }

  // The colour of the grid is the token of the theme it is drawn in, so every skin gets its own
  // step off its own background. Every skin is put on over the same field and read back.
  const skinWord = await page.evaluate(function () { return window.themeWord; });
  for (const one of ['light', 'dark', 'silver', 'corporate', 'neon']) {
    await page.evaluate(function (word) {
      window.__state.theme = word;
      window.showTheme(word);
    }, one);
    await sleep(420);
    if (process.env.AN_SHOT) {
      await page.screenshot({
        path: path.join(__dirname, '..', '..', 'k24-grid-'
          + (process.env.AN_PAGE ? 'web' : 'window') + '-' + one + '.png')
      });
    }
    const tone = await page.evaluate(TONE);
    if (!(tone.most > 0)) {
      throw new Error(name + ': nothing of the grid was drawn in the ' + one + ' skin: '
        + JSON.stringify(tone));
    }
    // A dozen steps either way are the drawing and not the wrong colour: a canvas keeps a faint
    // colour as a rounded multiple of its strength, so at the strength of a hairline the rounding
    // of each byte is of the order of ten steps. Two skins are tens of steps apart in colour.
    if (tone.off > 12) {
      throw new Error(name + ': the grid is not of the colour of the ' + one + ' skin: '
        + JSON.stringify(tone));
    }
    if (tone.most > 90) {
      throw new Error(name + ': the grid is too strong to read a chat over in the ' + one
        + ' skin: ' + JSON.stringify(tone));
    }
  }
  await page.evaluate(function (word) {
    window.__state.theme = word;
    window.showTheme(word);
  }, skinWord);
  await sleep(300);

  // A line goes into its oscillation now and then: after a while at least one line is swinging,
  // and it is swinging far enough out of straight to be seen. A line at rest keeps to its column
  // at every height; a line in its oscillation does not, and that is what is looked for here.
  let live = null;
  for (let try_ = 0; try_ < 30; try_++) {
    await sleep(400);
    live = await page.evaluate(WAVY);
    if (live.found >= 0 && live.spread > 1.5) { break; }
  }
  if (!live || live.waves === 0) {
    throw new Error(name + ': no line ever went into its oscillation: ' + JSON.stringify(live));
  }
  if (!(live.spread > 1.5)) {
    throw new Error(name + ': a line in its oscillation is drawn as straight as the rest: '
      + JSON.stringify(live));
  }
  // The two ends of a swinging line keep to their places, so the wave is nothing where the line
  // meets the top and the bottom of the place the chat is in.
  if (!(live.topCount > 2) || Math.abs(live.topWorst) > 1.2 || Math.abs(live.bottomWorst) > 1.2) {
    throw new Error(name + ': a swinging line does not keep its ends: ' + JSON.stringify(live));
  }
  const sound = await page.evaluate(function () {
    const waves = (window.grid && window.grid.waves) || [];
    let whole = 0;
    for (let at = 0; at < waves.length; at++) {
      const one = waves[at];
      if (isFinite(one.amp) && isFinite(one.life) && isFinite(one.sway) && isFinite(one.phase)
        && one.amp > 0 && one.life > 0 && one.sway > 0 && (one.axis === 'v' || one.axis === 'h')) {
        whole++;
      }
    }
    return { waves: waves.length, whole: whole, most: window.GRID_MOST };
  });
  if (sound.waves !== sound.whole || sound.waves > sound.most) {
    throw new Error(name + ': a line in its oscillation is drawn with nonsense: '
      + JSON.stringify(sound));
  }

  // The pixel-art form of a theme lays a check of squares of its own down over the page, so the
  // grid stands down while that form is on and comes back when it is taken off again.
  await page.evaluate(function () {
    window.__state.theme_pixel = true;
    window.showPixelArt(true);
  });
  await sleep(320);
  let pixel = await page.evaluate(READ, mid);
  if (pixel.asked !== false || !(pixel.painted === 0)) {
    throw new Error(name + ': the grid did not stand down for the pixel-art form: '
      + JSON.stringify(pixel));
  }
  await page.evaluate(function () {
    window.__state.theme_pixel = false;
    window.showPixelArt(false);
  });
  await sleep(420);
  pixel = await page.evaluate(READ, mid);
  if (pixel.asked !== true || !(pixel.painted > 0)) {
    throw new Error(name + ': the grid did not come back with the standard form: '
      + JSON.stringify(pixel));
  }

  // The setting takes the grid away: the canvas is emptied, which is what puts the solid colour of
  // the theme back where the grid was.
  await page.evaluate(function () { window.__state.chatting.grid = false; });
  await sleep(1500);
  const gone = await page.evaluate(READ, mid);
  if (gone.want !== false || !(gone.painted === 0)) {
    throw new Error(name + ': the grid stayed with the setting off: ' + JSON.stringify(gone));
  }

  // The settings panel carries the switch, and writes it back to the program.
  const panel = await page.evaluate(function () {
    const box = document.getElementById('fChatGrid');
    if (!box) { return { where: 'no box' }; }
    const out = window.readProgram ? window.readProgram() : null;
    box.checked = false;
    const off = window.readProgram ? window.readProgram() : null;
    box.checked = true;
    return {
      label: !!box.closest('.fld'),
      on: out ? out.chat_grid : null,
      off: off ? off.chat_grid : null
    };
  });
  if (!panel.label || panel.off !== false) {
    throw new Error(name + ': the switch of the grid is not the setting: ' + JSON.stringify(panel));
  }
  await page.evaluate(function () {
    document.getElementById('fChatGrid').checked = true;
    window.__state.chatting.grid = true;
  });
  await sleep(1500);
  const back = await page.evaluate(READ, mid);
  if (!(back.painted > 0)) {
    throw new Error(name + ': the grid did not come back with the setting on: '
      + JSON.stringify(back));
  }
  return { name: name, grid: 'ok' };
}

(async function main() {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  const failures = [];
  for (const one of PAGES) {
    const page = await browser.newPage();
    page.on('pageerror', function (e) { failures.push(one.name + ': ' + e.message); });
    page.on('console', function (m) {
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
  console.log('the background of the chat is a grid that swings now and then');
})();
