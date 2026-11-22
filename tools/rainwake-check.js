// When the rain begins to fall, and when it is let go again - checked in both pages of the
// program with no engine behind it.
//
// The rain is drawn over the place the chat is in, and that place is folded away while the home
// screen is shown. A setting turned to rain while no chat is open therefore cannot start anything:
// there is nothing for the drops to fall over. The going had to be asked after again when a chat
// came - which it was not, and the rain was then seen only at the next change of theme, because
// the colour of the rain is read again there and the going is asked after with it. These items
// hold the line, in each face:
// * with the setting at rain and no chat open, nothing is drawn on the canvas of the rain;
// * a chat opening, with no change of theme and no second word from the settings, sets the rain
//   going - drops exist, and squares are painted;
// * the rain is still falling at the next look at the state, and its squares are still painted;
// * the setting turned off again lets the drops go, and the canvas of the rain is emptied;
// * a page opened where the system asks for few movements shows no rain at all, and the movement
//   comes back when the system stops asking - with no state, no theme and no setting moved in
//   between, which is what the ask for few movements is supposed to do.
//
//   node rainwake-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node rainwake-check.js
//   node rainwake-check.js shots
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'face', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The state of the engine: at the home screen or with a chat open, and with the background of the
// chat at the word given. Nothing else about it moves between one look and the next.
function stateOf(home, background, rev) {
  const messages = [];
  for (let n = 1; n <= 4; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'The ' + n + 'st thing I asked about.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
      when: '09:0' + n });
    messages.push({ id: 'a' + n, role: 'Assistant', body: 'And the answer I gave, long enough to '
      + 'fill a few lines of the chat so that the rain has somewhere to fall behind.',
      reasoning: '', reasoning_ms: 0, when: '09:0' + n });
  }
  return {
    rev: rev, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' }, repositories: [],
    chat: { id: 'c1', title: 'Rain over a chat' },
    chats: [{ id: 'c1', title: 'Rain over a chat', repo: '', when: 'Today 09:04', active: true,
      preview: 'the answer' }],
    home: !!home, greeting: 'Still here, still ready.', overview: false, messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: false, highlight: true, chapters: true,
      paste_limit: 6000, background: background, rain_splash: true, cursor_digits: false,
      grid: true },
    context: { used: '4,304', total: '8,192', ratio: 0.5 },
    generating: false, busy: false, code: true, sandbox: false, sandbox_available: true,
    sandbox_note: 'bubblewrap works.', status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// What is put in before the page runs: the two bridges, and the reading of the canvas of the rain.
const LOOK = `
// How many squares of a canvas the program has drawn into.
window.__painted = function (where) {
  var canvas = document.getElementById(where);
  if (!canvas || !canvas.width || !canvas.height) { return -1; }
  var given = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
  var lit = 0;
  for (var i = 3; i < given.length; i += 4) { if (given[i] > 8) { lit += 1; } }
  return lit;
};
// The same, looked at over a stretch of time, for a frame is a moment and the rain is only
// drawn in some of them.
window.__over = function (where, forLong) {
  return new Promise(function (done) {
    var most = 0;
    var began = Date.now();
    var step = function () {
      most = Math.max(most, window.__painted(where));
      if (Date.now() - began > forLong) { done(most); return; }
      setTimeout(step, 40);
    };
    step();
  });
};
// What the page says about the going of the rain.
window.__rain = function () {
  return {
    want: window.fall ? window.fall.want : null,
    drops: window.rain ? window.rain.drops.length : -1,
    raf: !!(window.rain && window.rain.raf),
    asked: window.rainAsked ? window.rainAsked() : null,
    reduce: !!window.reduce
  };
};
`;

// One reading of one canvas, looked at over a stretch of time. The reading is asked for on its
// own, for a promise lying inside an answer is not waited for.
async function litOf(page, where, forLong) {
  return page.evaluate(function (name, long) { return window.__over(name, long); },
    where, forLong);
}

async function main() {
  const want = process.argv[2] === 'shots';
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 860 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(LOOK);
    await page.evaluateOnNewDocument(function (given) {
      window.__state = given;
      window.__set = function (state) { window.__state = state; };
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(window.__state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, stateOf(true, 'rain', 1));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });

    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };
    console.log('\n== ' + one.name);

    // The setting is at rain, and no chat is open: the place the rain would fall over is folded
    // away, so nothing is drawn - and nothing is asked for again either.
    await sleep(700);
    const home = await page.evaluate(() => window.__rain());
    const homeLit = await litOf(page, 'fall', 400);
    console.log('  at the home screen :', JSON.stringify(home), 'lit:', homeLit);
    if (home.want !== 'rain') {
      note('the setting is not at rain: ' + JSON.stringify(home));
    }
    if (home.raf || homeLit > 0) {
      note('the rain is drawn although no chat is open: ' + JSON.stringify(home) + ' ' + homeLit);
    }
    if (want) { await page.screenshot({ path: path.join(__dirname,
      'n1-rain-' + one.tag + '-1-home.png') }); }

    // A chat is opened. Nothing else moves: the theme is not touched, the settings say no more
    // than they said a moment ago, and the only thing that came is a place for the rain to fall
    // over. This is where the rain has to begin.
    await page.evaluate(function (given) {
      window.__set(given);
      if (typeof poll === 'function') { poll(); }
    }, stateOf(false, 'rain', 2));
    await sleep(900);
    const open = await page.evaluate(() => window.__rain());
    const openLit = await litOf(page, 'fall', 1200);
    console.log('  the chat opened    :', JSON.stringify(open), 'lit:', openLit);
    if (!open.asked || !open.raf) {
      note('the rain did not begin when the chat came: ' + JSON.stringify(open));
    }
    if (!(open.drops > 4)) {
      note('no drops of the rain: ' + JSON.stringify(open));
    }
    if (!(openLit > 40)) {
      note('the rain is not painted over the chat: ' + JSON.stringify(open) + ' ' + openLit);
    }
    if (want) { await page.screenshot({ path: path.join(__dirname,
      'n1-rain-' + one.tag + '-2-chat.png') }); }

    // The going is kept: another look at the state, and the rain is still being drawn.
    await page.evaluate(function (given) {
      window.__set(given);
      if (typeof poll === 'function') { poll(); }
    }, stateOf(false, 'rain', 3));
    await sleep(700);
    const kept = await page.evaluate(() => window.__rain());
    const keptLit = await litOf(page, 'fall', 900);
    console.log('  a later look       :', JSON.stringify(kept), 'lit:', keptLit);
    if (!kept.raf || !(keptLit > 40)) {
      note('the rain stopped at a later look at the state: ' + JSON.stringify(kept)
        + ' ' + keptLit);
    }

    // The setting is turned off: the drops are let go, and the canvas of the rain is emptied.
    await page.evaluate(function (given) {
      window.__set(given);
      if (typeof poll === 'function') { poll(); }
    }, stateOf(false, 'none', 4));
    await sleep(1200);
    const off = await page.evaluate(() => window.__rain());
    const offLit = await litOf(page, 'fall', 500);
    console.log('  the setting off    :', JSON.stringify(off), 'lit:', offLit);
    if (off.want !== 'none' || off.raf || offLit > 0) {
      note('the rain is drawn although the setting is off: ' + JSON.stringify(off) + ' ' + offLit);
    }

    // And back on again, with the chat remaining open: the edge of the setting starts it as it
    // always did.
    await page.evaluate(function (given) {
      window.__set(given);
      if (typeof poll === 'function') { poll(); }
    }, stateOf(false, 'rain', 5));
    await sleep(900);
    const again = await page.evaluate(() => window.__rain());
    const againLit = await litOf(page, 'fall', 900);
    console.log('  the setting on     :', JSON.stringify(again), 'lit:', againLit);
    if (!again.raf || !(againLit > 40)) {
      note('the rain did not begin again when the setting was turned on: '
        + JSON.stringify(again) + ' ' + againLit);
    }
    if (want) { await page.screenshot({ path: path.join(__dirname,
      'n1-rain-' + one.tag + '-5-rain.png') }); }
    if (faults.length) {
      trouble += 1;
      for (const fault of faults) { console.log('  FAULT: page error: ' + fault); }
    }
    if (notes.length) { trouble += 1; }
    await page.close();

    // A page opened where the system asks for few movements: no rain at all. The ask is then
    // taken away, and the movement has to come back of itself - no state is asked for, no theme
    // is changed and no setting is moved.
    const still = await browser.newPage();
    await still.setViewport({ width: 1280, height: 860 });
    const quiet = [];
    still.on('pageerror', (e) => quiet.push(String(e)));
    await still.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await still.evaluateOnNewDocument(LOOK);
    await still.evaluateOnNewDocument(function (given) {
      window.__state = given;
      window.__set = function (state) { window.__state = state; };
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(window.__state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, stateOf(false, 'rain', 1));
    await still.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(900);
    const hushed = await still.evaluate(() => window.__rain());
    const hushedLit = await litOf(still, 'fall', 500);
    console.log('  few movements      :', JSON.stringify(hushed), 'lit:', hushedLit);
    if (!(hushedLit < 1) || !hushed.reduce) {
      note('the rain moves although the system asks for few movements: '
        + JSON.stringify(hushed) + ' ' + hushedLit);
    }
    if (want) { await still.screenshot({ path: path.join(__dirname,
      'n1-rain-' + one.tag + '-6-quiet.png') }); }
    await still.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'no-preference' }]);
    await sleep(900);
    const wake = await still.evaluate(() => window.__rain());
    const wakeLit = await litOf(still, 'fall', 1200);
    console.log('  the ask taken off  :', JSON.stringify(wake), 'lit:', wakeLit);
    if (!wake.raf || !(wakeLit > 40)) {
      note('the rain did not come back when the system stopped asking for few movements: '
        + JSON.stringify(wake) + ' ' + wakeLit);
    }
    if (want) { await still.screenshot({ path: path.join(__dirname,
      'n1-rain-' + one.tag + '-7-woke.png') }); }
    if (quiet.length) {
      trouble += 1;
      for (const fault of quiet) { console.log('  FAULT: page error: ' + fault); }
    }
    await still.close();
  }
  await browser.close();
  console.log('\n' + (trouble ? trouble + ' face(s) in trouble' : 'ok: nothing in trouble')
    + (want ? ' (shots taken)' : ''));
  if (trouble) { process.exit(1); }
}

main().catch((e) => { console.error(e); process.exit(1); });
