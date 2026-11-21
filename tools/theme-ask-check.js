// A theme the user chose is not moved back by a state asked for before the choice.
//
// The colours of the page come from the state, which is read again with every poll. The poll on
// its way when a theme is clicked carries the theme the user had left, and it can land after the
// click: the page goes back into the old colours for a moment and is moved to the new ones again
// when the next state comes. The page therefore holds a chosen theme against the state until the
// program agrees to it (themeFromState, artFromState, askTheme).
//
// This check drives that with a state request that answers late, which is what the flicker needs
// in the first place. It first checks it twice over: the same late answer is run against the page
// with the holding taken away, and the flicker must be seen, so the check is known to be looking
// at the right thing. It then checks the holding itself - the chosen theme stays, the hold is
// given up when the state agrees, and is given up at last after ASK_AGED - for the theme and for
// its pixel-art form.
//
//   node theme-ask-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node theme-ask-check.js
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

// How long a request for the state waits before answering. Longer than the moment between the
// click and the request, shorter than the polling of the page, so an answer that was asked for
// before the click always comes after it.
const LATE = 260;

// One chat, so the page has a thread to draw and a theme to draw it in.
function stateOf(theme, pixel) {
  const messages = [];
  for (let n = 1; n <= 4; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'The ' + n + 'st thing I asked about.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
      when: '09:0' + n });
    messages.push({ id: 'a' + n, role: 'Assistant', body: 'And the answer I gave.',
      reasoning: '', reasoning_ms: 0, when: '09:0' + n });
  }
  return {
    rev: 1, unchanged: false, theme: theme, theme_pixel: !!pixel,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A theme is chosen once' },
    chats: [{ id: 'c1', title: 'A theme is chosen once', repo: '', when: 'Today 09:04',
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
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

// The two transports of the two pages, held to the same rules: the state answers out of a
// snapshot of the moment the request came in, after LATE milliseconds, and a write of the theme
// is taken at once - which is what the program does, and why the next state agrees.
function stubs(state) {
  window.__state = state;
  window.__asks = 0;
  window.__late = 0;
  window.__writes = [];

  // One answer for both transports. `raw` is how the window bridge is answered: with the thing
  // itself, and not with the shape of an HTTP answer.
  const answer = function (name, body, raw) {
    const plain = function (made) {
      return raw ? made : { json: function () { return Promise.resolve(made); } };
    };
    if (name === 'state') {
      window.__asks++;
      const snapshot = JSON.parse(JSON.stringify(window.__state));
      snapshot.rev = window.__asks;
      snapshot.unchanged = false;
      return new Promise(function (resolve) {
        setTimeout(function () { resolve(plain(snapshot)); }, window.__late || 0);
      });
    }
    if (name === 'theme') {
      window.__writes.push(body);
      if (body && body.theme) { window.__state.theme = body.theme; }
      if (body && body.pixel !== undefined) { window.__state.theme_pixel = !!body.pixel; }
    }
    return Promise.resolve(plain({ ok: true }));
  };

  window.fetch = function (url, opts) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '').split('?')[0];
    let body = {};
    try { body = JSON.parse((opts && opts.body) || '{}'); } catch (e) { body = {}; }
    return answer(name, body, false);
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return answer('state', {}, true); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { return answer(String((args && args.path) || ''), (args && args.body) || {}, true); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// Every value the page drew the theme in, from the moment the watcher is put on. The class of the
// body is what the theme is drawn out of, so it is what a flicker shows in.
const WATCH = function () {
  window.__seen = [];
  const body = document.body;
  window.__watcher = new MutationObserver(function () {
    window.__seen.push({ silver: body.classList.contains('silver'),
      dark: body.classList.contains('dark'),
      corporate: body.classList.contains('corporate'),
      neon: body.classList.contains('neon'),
      pixel: body.classList.contains('pixel') });
  });
  window.__watcher.observe(body, { attributes: true, attributeFilter: ['class'] });
};

// The theme a record of the watcher was drawn in.
const wordOf = (one) => {
  for (const word of ['dark', 'silver', 'corporate', 'neon']) { if (one[word]) { return word; } }
  return 'light';
};

async function run(page, file, name) {
  await page.setViewport({ width: 1280, height: 860 });
  await page.evaluateOnNewDocument(stubs, stateOf('dark', false));
  await page.goto('file://' + file, { waitUntil: 'domcontentloaded' });
  await sleep(700);

  const started = await page.evaluate(function () {
    return { word: window.themeWord, dark: document.body.classList.contains('dark') };
  });
  if (started.word !== 'dark' || !started.dark) {
    throw new Error(name + ': the page did not start in the theme of its state: '
      + JSON.stringify(started));
  }

  // The hold, and the state that was asked for before the choice: the theme of the page does not
  // move, and the hold stands until the state agrees with it.
  const held = await page.evaluate(function () {
    window.__stale = JSON.parse(JSON.stringify(window.__state));
    window.chooseTheme('silver');
    const chosen = document.body.classList.contains('silver');
    window.draw(window.__stale);            // the answer that was on its way
    const after = document.body.classList.contains('silver');
    const ask = window.themeAsk.theme;
    window.draw(window.__state);            // and the state that agrees at last
    return { chosen: chosen, after: after, ask: ask, held: window.themeAsk.theme,
      word: window.themeWord, silver: document.body.classList.contains('silver') };
  });
  if (!held.chosen) { throw new Error(name + ': the theme clicked did not go on at once'); }
  if (!held.after) { throw new Error(name + ': a state asked for before the choice moved the theme back: ' + JSON.stringify(held)); }
  if (held.ask !== 'silver') { throw new Error(name + ': the choice was not held: ' + JSON.stringify(held)); }
  if (held.held !== '') { throw new Error(name + ': the hold was not given up when the state agreed: ' + JSON.stringify(held)); }
  if (!held.silver || held.word !== 'silver') {
    throw new Error(name + ': the page is not in the theme it agreed to: ' + JSON.stringify(held));
  }

  // A hold the program never agreed to is given up after its time, so the page follows the
  // program at last rather than keeping a theme nobody wrote.
  const aged = await page.evaluate(function () {
    window.themeAsk.theme = 'neon';
    window.themeAsk.at = -1000000;          // older than ASK_AGED, whatever clock is used
    const stale = JSON.parse(JSON.stringify(window.__state));
    stale.theme = 'corporate';
    window.draw(stale);
    return { word: window.themeWord, ask: window.themeAsk.theme,
      corporate: document.body.classList.contains('corporate') };
  });
  if (aged.ask !== '' || aged.word !== 'corporate' || !aged.corporate) {
    throw new Error(name + ': an aged hold was not given up to the state: ' + JSON.stringify(aged));
  }

  // The pixel-art form of the theme travels with it and is held the same way.
  const art = await page.evaluate(function () {
    window.__state.theme_pixel = false;
    window.chooseThemeArt(true);
    const chosen = document.body.classList.contains('pixel');
    const stale = JSON.parse(JSON.stringify(window.__state));
    stale.theme_pixel = false;
    window.draw(stale);
    const after = document.body.classList.contains('pixel');
    const ask = window.themeAsk.art;
    window.draw(window.__state);
    return { chosen: chosen, after: after, ask: ask, held: window.themeAsk.art,
      pixel: document.body.classList.contains('pixel') };
  });
  if (!art.chosen) { throw new Error(name + ': the pixel-art form did not go on at once'); }
  if (!art.after) { throw new Error(name + ': a state asked for before the choice took the form back: ' + JSON.stringify(art)); }
  if (art.ask !== true) { throw new Error(name + ': the form was not held: ' + JSON.stringify(art)); }
  if (art.held !== null) { throw new Error(name + ': the hold of the form was not given up: ' + JSON.stringify(art)); }
  if (!art.pixel) { throw new Error(name + ': the page is not in the form it agreed to: ' + JSON.stringify(art)); }

  // The flicker itself, driven end to end: the state answers LATE ms after it was asked, so an
  // answer of the old theme is on its way across when the theme is clicked.
  //
  // First with the holding taken away, which is how the page used to be. The old theme must be
  // seen again, or this part of the check is looking at nothing.
  const through = await flick(page, true);
  if (!(through.moved && through.back)) {
    throw new Error(name + ': the check did not reproduce the flicker with the holding taken away, so it proves nothing: '
      + JSON.stringify(through));
  }
  const fixed = await flick(page, false);
  if (fixed.back) {
    throw new Error(name + ': the chosen theme moved back while a state of the old one was on its way: '
      + JSON.stringify(fixed));
  }
  if (!fixed.settled || fixed.word !== fixed.wanted) {
    throw new Error(name + ': the chosen theme did not hold to the end: ' + JSON.stringify(fixed));
  }
  return { name: name, through: through.moved, held: fixed.wanted };
}

// One going of the theme marker with the state answering late. `blind` takes the holding of the
// page away, so the old behaviour can be measured against the new.
async function flick(page, blind) {
  await page.evaluate(function (late, takeAway) {
    window.__late = late;
    if (takeAway) {
      // The page as it was: the state is the theme, whatever was just clicked.
      if (!window.__realTheme) { window.__realTheme = window.themeFromState; }
      window.themeFromState = function (s) { return (s && s.theme) || 'light'; };
    } else if (window.__realTheme) {
      window.themeFromState = window.__realTheme;
      window.__realTheme = null;
    }
    window.themeAsk.theme = '';
    window.themeAsk.art = null;
  }, LATE, blind);
  await sleep(700);

  const before = await page.evaluate(function () {
    return { word: window.themeWord, asks: window.__asks };
  });
  await page.evaluate(WATCH);
  // Click once a request for the state is on its way, so the answer of the old theme is certain
  // to land after the click.
  let waited = 0;
  while (waited < 1600) {
    const asks = await page.evaluate(function () { return window.__asks; });
    if (asks > before.asks) { break; }
    await sleep(20);
    waited += 20;
  }
  await page.click('#themebtn');
  await sleep(1400);
  const seen = await page.evaluate(function () {
    window.__watcher.disconnect();
    return { seen: window.__seen, word: window.themeWord };
  });
  const wanted = await page.evaluate(function (word) {
    return window.THEMES[(window.THEMES.indexOf(word) + 1) % window.THEMES.length];
  }, before.word);
  const after = seen.seen.filter(function (one) { return wordOf(one) !== before.word; });
  const back = seen.seen.some(function (one) {
    return wordOf(one) === before.word;
  }) && after.length > 0;
  return { from: before.word, wanted: wanted, moved: after.length > 0, back: back,
    word: seen.word, settled: seen.word === wanted, steps: seen.seen.length };
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
      console.log('ok', report.name, '(flicker seen without the hold:', report.through +
        ', theme held:', report.held + ')');
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
  console.log('a theme chosen holds until the program agrees to it');
})();
