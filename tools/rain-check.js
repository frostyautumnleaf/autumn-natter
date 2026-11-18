// The pixelated rain behind the words of the chat, looked at in both faces of the
// program with no engine behind it.
//
// Seven things are looked at, in each face:
//   * the canvas of the splash stands over the chat, catches no hand, and stands
//     under every menu, sheet and window of the program;
//   * nothing is drawn while the setting says nothing, whether the splash switch
//     stands on or off;
//   * the rain falls while the setting says rain, and it falls although no answer
//     is being written - which is where it differs from the leaves;
//   * the drops are drawn in squares: every square of the rain is a whole number
//     of RAIN_CELL across, and no colour of it stands between one square and the
//     next;
//   * the rain splashes where it meets the chat composer box and the blocks of the
//     chat - the top edge of each of them is stood where the box and the blocks
//     stand - and with the splash switch turned off nothing at all is drawn on the
//     canvas of the splash;
//   * the splash of two drops that meet the same edge at the same place are not the
//     same splash, for the water is thrown anew each time;
//   * the sheet of the settings marks the word the settings hold, and the line of
//     the splash stands only while the rain stands chosen.
//
//   node rain-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node rain-check.js
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

function said(id, role, body, extra) {
  const one = {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', tool_name: '', mono: false, failed: false,
    cancelled: false, stopped: false, can_continue: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
  };
  return Object.assign(one, extra || {});
}

// A chat with a thought and a run of a tool in it, so that the rain has the blocks
// of the chat to splash on as well as the chat composer box.
function stateOf(theme, background, splash, generating) {
  return {
    rev: 1, unchanged: false, theme: theme || 'light',
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A chat with rain behind it', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat with rain behind it', repo: '', when: 'Today 09:04',
      active: true, preview: 'Let it rain.' }],
    commands: [{ name: '/plan', words: 'Plan the work before any of it is done.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('1', 'You', 'Let it rain.'),
      said('2', 'Assistant', 'The rain is of squares, and so is the splash of it.', {
        reasoning: 'The water has to fall in squares, so the drop is a run of squares '
          + 'down one column, and the splash is thrown off the edge it meets.',
        reasoning_ms: 2400,
      }),
      said('3', 'Tool', 'rain: started\nrain: 441 mm in an hour', {
        tool: 'command', tool_name: 'command', mono: true,
      }),
      said('4', 'Assistant', 'It came down hard and square.', {
        reasoning: 'The chat composer box has a top edge, and so has every block.',
        reasoning_ms: 900,
      }),
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000, background: background || 'none',
      rain_splash: splash !== false },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: !!generating, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.', working: [],
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

// What is stood in before the page runs: the two bridges, and the reading of the
// two canvases the rain is drawn on.
const LOOK = `
// How many squares of a canvas the program has drawn into.
function painted(where) {
  var canvas = document.getElementById(where);
  if (!canvas || !canvas.width || !canvas.height) { return -1; }
  var ctx = canvas.getContext('2d');
  var given = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  var lit = 0;
  for (var i = 3; i < given.length; i += 4) { if (given[i] > 8) { lit += 1; } }
  return lit;
}
// The same, looked at over a stretch of time, for a splash is seen for a third of
// a second and may well have come and gone between one look and the next.
function paintedOver(where, forLong) {
  return new Promise(function (done) {
    var most = 0;
    var began = Date.now();
    var step = function () {
      most = Math.max(most, painted(where));
      if (Date.now() - began > forLong) { done(most); return; }
      setTimeout(step, 40);
    };
    step();
  });
}
// The squares of the canvas that hold a colour, so that a rain drop may be said to
// be drawn in squares: the far edge of every square of colour is a whole number of
// RAIN_CELL from the near edge of the canvas.
function squareEdges() {
  var canvas = document.getElementById('fall');
  var ctx = canvas.getContext('2d');
  var dpr = rain.dpr;
  var given = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  var cell = RAIN_CELL * dpr;
  var bad = 0, lit = 0;
  var run = 0, inOne = false, start = -1;
  for (var x = 0; x < canvas.width; x++) {
    var seen = false;
    for (var y = 0; y < canvas.height; y++) {
      if (given[(y * canvas.width + x) * 4 + 3] > 8) { seen = true; break; }
    }
    if (seen && !inOne) { inOne = true; start = x; lit += 1; }
    if (!seen && inOne) {
      inOne = false;
      // The head of a drop is one square wide, and its far edge is a whole number
      // of squares from the edge of the canvas. A drop that is longer than one
      // square wide, or a square that stands between two others, is the fault.
      if ((start % cell) !== 0) { bad += 1; }
    }
  }
  return { columns: lit, ragged: bad, cell: cell };
}
// The edges the rain is stood to splash on, as the page reads them.
function edges() {
  if (!rain.surfaces.length) { rain.surfaces = rainSurfaces(); }
  var stage = document.getElementById('stage');
  var s = stage.getBoundingClientRect();
  var box = document.getElementById('box').getBoundingClientRect();
  var want = Math.round(box.top - s.top);
  var tool = document.querySelector('#thread .tool');
  var think = document.querySelector('#thread .think');
  var near = function (y) {
    for (var i = 0; i < rain.surfaces.length; i++) {
      if (Math.abs(rain.surfaces[i].y - y) <= 1) { return true; }
    }
    return false;
  };
  return {
    at: rain.surfaces.length,
    box: near(want),
    tool: tool ? near(Math.round(tool.getBoundingClientRect().top - s.top)) : null,
    think: think ? near(Math.round(think.getBoundingClientRect().top - s.top)) : null,
    blocks: document.querySelectorAll('#thread .tool').length,
    thoughts: document.querySelectorAll('#thread .think').length,
  };
}
// Two splashes at one place, drawn by the page itself, and how unlike they are.
function unlike(x, y) {
  rain.splashes.length = 0;
  rainSpur(x, y);
  var first = JSON.stringify(rain.splashes[0]);
  rainSpur(x, y);
  var second = JSON.stringify(rain.splashes[1]);
  return { of: rain.splashes.length, same: first === second };
}
`;

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
    }, stateOf('light', 'none', true, false));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(500);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  FAULT: ' + what); };

    console.log('\n== ' + one.name);

    // The canvas of the splash: over the chat, under the menus, and catching
    // nothing.
    const stood = await page.evaluate(() => {
      const canvas = document.getElementById('splash');
      const main = document.getElementById('main');
      const thread = document.getElementById('thread');
      const box = document.getElementById('box');
      if (!canvas) { return { missing: true }; }
      const look = getComputedStyle(canvas);
      return {
        inMain: canvas.parentNode === main,
        overThread: Number(look.zIndex) > Number(getComputedStyle(thread).zIndex),
        overBox: Number(look.zIndex) >= Number(getComputedStyle(box).zIndex || 0),
        underMenu: Number(look.zIndex) < 40,
        clicks: look.pointerEvents,
        covers: Math.round(canvas.getBoundingClientRect().height)
          >= Math.round(box.getBoundingClientRect().bottom - main.getBoundingClientRect().top) - 24,
      };
    });
    if (stood.missing) {
      note('no canvas of the splash stands in the chat');
    } else {
      if (!stood.inMain) { note('the canvas of the splash does not stand in the chat'); }
      if (!stood.overThread) { note('the splash stands under the talk, where a solid block would hide it'); }
      if (!stood.underMenu) { note('the splash stands over the menus of the chat'); }
      if (stood.clicks !== 'none') { note('the canvas of the splash catches the hand'); }
      if (!stood.covers) { note('the canvas of the splash does not reach the chat composer box'); }
    }

    // Nothing at all is drawn while the setting says nothing.
    const dry = await page.evaluate(async () => {
      return { wet: await paintedOver('fall', 500), splash: await paintedOver('splash', 500) };
    });
    if (dry.wet > 0 || dry.splash > 0) {
      note('something is drawn with the setting at nothing: ' + JSON.stringify(dry));
    }

    // The blocks the rain is meant to splash on stand in the chat.
    const edges = await page.evaluate(() => {
      return { blocks: document.querySelectorAll('#thread .tool').length,
        thoughts: document.querySelectorAll('#thread .think').length };
    });
    if (!edges.blocks || !edges.thoughts) {
      note('the chat holds no block of a tool or no thought to splash on: ' + JSON.stringify(edges));
    }

    // The rain falls with no answer being written, and it splashes.
    await page.evaluate((given) => { window.__set(given); if (typeof poll === 'function') { poll(); } },
      stateOf('light', 'rain', true, false));
    await sleep(1200);
    const where = await page.evaluate(() => edges());
    if (where.at < 2) {
      note('the rain is stood to splash on too little: ' + JSON.stringify(where));
    } else {
      if (!where.box) { note('the top edge of the chat composer box is not among them'); }
      if (where.blocks && !where.tool) { note('the top edge of a block of the chat is not among them'); }
      if (where.thoughts && !where.think) { note('the top edge of a thought is not among them'); }
    }
    const wet = await page.evaluate(async () => ({
      drops: await paintedOver('fall', 900),
      splash: await paintedOver('splash', 900),
      squares: squareEdges(),
    }));
    if (wet.drops < 20) { note('the rain barely falls at all: ' + wet.drops + ' squares'); }
    if (wet.splash < 1) { note('the rain falls without splashing on anything'); }
    if (wet.squares.columns && wet.squares.ragged) {
      note(wet.squares.ragged + ' columns of the rain do not stand on the squares');
    }

    // The splash of two drops at one place are not the same splash.
    const unlike = await page.evaluate(() => unlike(220, 300));
    if (unlike.of !== 2) { note('the page did not draw two splashes to compare'); }
    if (unlike.same) { note('two splashes at one place are alike, so the water is not thrown anew'); }

    if (want) {
      await page.screenshot({ path: path.join(__dirname, 'rain-' + one.tag + '-light.png') });
    }

    // With the splash switch turned off the rain still falls, and nothing is drawn
    // on the canvas of the splash.
    await page.evaluate((given) => { window.__set(given); if (typeof poll === 'function') { poll(); } },
      stateOf('light', 'rain', false, false));
    await sleep(900);
    const past = await page.evaluate(async () => ({
      drops: await paintedOver('fall', 700),
      splash: await paintedOver('splash', 700),
      switch: chatting.rainSplash,
    }));
    if (past.switch) { note('the splash switch did not follow the state'); }
    if (past.drops < 20) { note('the rain stopped falling when the splash switch was turned off'); }
    if (past.splash > 0) {
      note('the rain splashes with the switch turned off: ' + past.splash + ' squares');
    }

    // The leaves still wait for an answer, whichever way the rain stands.
    await page.evaluate((given) => { window.__set(given); if (typeof poll === 'function') { poll(); } },
      stateOf('light', 'leaves', true, false));
    await sleep(700);
    const leaves = await page.evaluate(async () => ({
      drops: await paintedOver('fall', 400), splash: await paintedOver('splash', 200),
    }));
    if (leaves.drops > 0 || leaves.splash > 0) {
      note('the leaves fell with no answer being written: ' + JSON.stringify(leaves));
    }

    // The sheet of the settings.
    const sheet = await page.evaluate(() => {
      showBackgroundButtons('rain');
      var row = document.getElementById('fRainSplashRow');
      var out = {
        word: backgroundWord(),
        rain: document.getElementById('fBackgroundRain').classList.contains('on'),
        none: document.getElementById('fBackgroundNone').classList.contains('on'),
        leaves: document.getElementById('fBackgroundLeaves').classList.contains('on'),
        shown: row && !row.classList.contains('hidden'),
      };
      showBackgroundButtons('leaves');
      out.after = backgroundWord();
      out.hidden = document.getElementById('fRainSplashRow').classList.contains('hidden');
      showBackgroundButtons('none');
      out.noneWord = backgroundWord();
      return out;
    });
    if (sheet.word !== 'rain' || !sheet.rain || sheet.none || sheet.leaves) {
      note('the sheet does not stand at the rain: ' + JSON.stringify(sheet));
    }
    if (!sheet.shown) { note('the line of the splash is hidden while the rain stands chosen'); }
    if (sheet.after !== 'leaves' || !sheet.hidden) {
      note('the line of the splash stands with the leaves: ' + JSON.stringify(sheet));
    }
    if (sheet.noneWord !== 'none') { note('the sheet does not stand at nothing'); }

    if (want) {
      // The same rain on the dark skin, and on the grey one, to be looked at.
      await page.evaluate((given) => { window.__set(given); if (typeof poll === 'function') { poll(); } },
        stateOf('dark', 'rain', true, false));
      await sleep(1400);
      await page.screenshot({ path: path.join(__dirname, 'rain-' + one.tag + '-dark.png') });
    }

    if (faults.length) {
      trouble += 1;
      for (const fault of faults) { console.log('  FAULT: page error: ' + fault); }
    }
    if (notes.length) { trouble += 1; }
    await page.close();
  }
  await browser.close();
  console.log('\n' + (trouble ? trouble + ' face(s) in trouble' : 'ok: nothing in trouble') +
    (want ? ' (shots taken)' : ''));
  if (trouble) { process.exit(1); }
}

main().catch((e) => { console.error(e); process.exit(1); });
