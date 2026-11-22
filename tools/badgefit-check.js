// The row of badges under the text of the chat composer never lays a badge over another badge.
//
// The badges carry the whole words of what they switch, and the box they stand in is narrower than
// the window and changes with the lines at the sides of the chat, with a new chat in front of the
// user, and with the width the chat is set to. The row is measured against the box and wears the
// face that fits it: the three columns of the design, the two sides bending under themselves while
// the sandbox stays above the middle; or, where a column no longer holds the widest badge, the
// parts taking their place in one line that runs over as many lines as it needs. This check walks
// the breadth of the window down to a width no one chats at, and asks at every step that no badge
// lies over another, that no badge is painted outside the box, that the sandbox is above the
// middle of the box whenever the row keeps its columns, that the words of the badges are still
// there to read, that the row is never two lines where one line holds it, and that every
// drop-down opened from a badge of the row is kept within the window. A word that grows with the
// state - the count of the sources of retrieval - is grown too, for the row is measured again
// when the words are written over.
//
// Both pages are checked: the window of the program and the page served by --remote carry the
// same rules.
//
//   node badgefit-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node badgefit-check.js
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

// The breadths of the window the row is looked at over: from a window wider than most screens to
// one no one chats at. The box of the composer is narrower than all of them.
const WIDTHS = [1400, 1180, 1000, 900, 820, 760, 700, 640, 580, 520, 460];

// The state of the engine, as the record of web::state. `home` puts a new chat in front of the
// user, which makes the box of the composer a breadth of its own. The engine always sends a chat
// along, even of no id and no title, so the state of a new chat carries an empty one.
function stateOf(home) {
  return {
    rev: 1, unchanged: false, theme: 'dark', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-9B', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-9B', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: home
      ? { id: '', title: 'No chat', planning: false, planned: false }
      : { id: 'c1', title: 'Badges in a narrow box', planning: false, planned: false },
    chats: home ? [] : [{ id: 'c1', title: 'Badges in a narrow box', repo: '', when: 'Today 09:04',
      active: true, preview: 'Badges.' }],
    home: home, greeting: 'Still here, still ready.', overview: false,
    messages: home ? [] : [
      { id: '1', role: 'You', body: 'Badges.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [{ name: 'notes', on: true, tools: 3 },
      { name: 'a server with a longer name than most', on: false, tools: 12 }] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    rag: { enabled: true, files: 3, chunks: 40, pending: 0, ready: true },
    chatting: { collapse_thinking: true, collapse_tools: false, highlight: true, chapters: true,
      paste_limit: 6000, background: 'none', rain_splash: true, cursor_digits: false },
    context: { used: '4,304', total: '8,192', ratio: 0.5 },
    generating: false, busy: false, code: true, sandbox: false, sandbox_available: true,
    sandbox_note: 'bubblewrap works.', status: 'ready', dark: true,
    data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  window.__state = state;
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(state); } }); }
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

// The row, its parts, every badge of it, and the box the row stands in. `spill` is how far past
// the edge of the box the widest badge is painted, and `cross` names the first pair of badges that
// lie over one another.
function read() {
  const row = document.getElementById('badges');
  const place = (one) => {
    const r = one.getBoundingClientRect();
    return { id: one.id || one.className, left: r.left, right: r.right, top: r.top,
      bottom: r.bottom, width: r.width, height: r.height };
  };
  const cross = (a, b) => a.left < b.right - 1 && b.left < a.right - 1
    && a.top < b.bottom - 1 && b.top < a.bottom - 1;
  const rowBox = row.getBoundingClientRect();
  const partBox = document.querySelector('.dock-row').getBoundingClientRect();
  const partStyle = getComputedStyle(document.querySelector('.dock-row'));
  const from = partBox.left + parseFloat(partStyle.paddingLeft);
  const until = partBox.right - parseFloat(partStyle.paddingRight);
  const badges = [];
  const nodes = row.querySelectorAll('.badge, .send');
  for (let n = 0; n < nodes.length; n++) { badges.push(place(nodes[n])); }
  let overlapped = '';
  let spill = 0;
  let spilling = '';
  for (let n = 0; n < badges.length; n++) {
    const one = badges[n];
    if (one.right - until > spill) { spill = one.right - until; spilling = one.id; }
    if (from - one.left > spill) { spill = from - one.left; spilling = one.id; }
    for (let m = n + 1; m < badges.length; m++) {
      if (!overlapped && cross(one, badges[m])) { overlapped = one.id + ' over ' + badges[m].id; }
    }
  }
  const sandbox = place(document.getElementById('sandbox'));
  // The breadth the badges want when they are all laid on one line. Where that fits the box, the
  // row has no need of a second line, and a second line costs the chat a line of its own: the
  // sheet of the questions of planning mode is read down to the button of send.
  const worn = row.className;
  row.classList.add('measure');
  let wants = Math.ceil(row.scrollWidth);
  let tallest = 0;
  const parts = row.children;
  for (let n = 0; n < parts.length; n++) {
    const one = parts[n].getBoundingClientRect().height;
    if (one > tallest) { tallest = one; }
  }
  row.className = worn;
  return {
    face: window.badgesFace,
    wants: wants,
    tallest: Math.round(tallest),
    stacked: row.classList.contains('stack'),
    display: getComputedStyle(row).display,
    height: Math.round(rowBox.height),
    room: row.clientWidth,
    from: Math.round(from), until: Math.round(until),
    spill: Math.round(spill),
    spilling: spilling,
    overlapped: overlapped,
    shown: badges.length,
    // The middle of the sandbox badge against the middle of the box.
    offCentre: Math.round((sandbox.left + sandbox.right) / 2 - (rowBox.left + rowBox.width / 2)),
    thinking: (document.getElementById('thinktext').getBoundingClientRect().width > 40),
    ragWord: document.getElementById('ragtext').textContent,
  };
}

async function main() {
  const browser = await puppeteer.launch({
    headless: 'shell', executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    for (const home of [false, true]) {
      const state = stateOf(home);
      const page = await browser.newPage();
      const faults = [];
      page.on('pageerror', (e) => faults.push(String(e)));
      await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
      await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(state) + ');');
      const kind = home ? 'a new chat' : 'a chat open';
      for (const width of WIDTHS) {
        await page.setViewport({ width: width, height: 820 });
        await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
        await sleep(420);
        const got = await page.evaluate(read);
        const said = one.name + ', ' + kind + ', a window ' + width + ' wide';
        if (got.shown !== 8) { throw new Error(said + ': not every badge is in the row: ' + got.shown); }
        if (got.overlapped) { throw new Error(said + ': ' + got.overlapped); }
        if (got.spill > 1) { throw new Error(said + ': ' + got.spilling + ' is painted ' + got.spill
          + ' past the edge of the box in the face of ' + got.face); }
        if (!got.stacked && Math.abs(got.offCentre) > 2) {
          throw new Error(said + ': the sandbox is ' + got.offCentre
            + ' off the middle of the box in the face of ' + got.face);
        }
        if (!got.thinking) { throw new Error(said + ': the word of the thinking badge is gone'); }
        if (width === 1400 && !home && got.height > 35) {
          throw new Error(said + ': the row is ' + got.height + ' tall where it has the whole box');
        }
        // A row that has the breadth it wants on one line is never two lines tall, whichever of
        // the two faces it wears.
        if (got.wants <= got.room + 1 && got.height > got.tallest + 2) {
          throw new Error(said + ': the row is ' + got.height + ' tall, ' + got.wants
            + ' of badges in a box ' + got.room + ' wide, on the face of ' + got.face);
        }
        // A box opened from one of the badges is kept within the window. The row of badges comes to
        // the edge of it when it bends, and the sandbox badge is laid last of the line, so a box
        // that grew outwards from the badge would be painted beyond the edge. A line longer than
        // any the box holds now is added to it, for the box is grown by the words of its lines.
        const boxes = await page.evaluate(function () {
          var out = [];
          var seen = [
            ['the sandbox', window.openSandboxMenu, window.closeSandboxMenu, window.el.sandboxmenu],
            ['the thinking', window.openThinkMenu, window.closeThinkMenu, window.el.thinkmenu],
            ['the tools', window.openToolMenu, window.closeToolMenu, window.el.codemenu],
            ['the servers of MCP', window.openMcpMenu, window.closeMcpMenu, window.el.mcpmenu],
            ['the ways a file comes in', window.openPlusMenu, window.closePlusMenu,
              window.el.plusmenu],
          ];
          for (var n = 0; n < seen.length; n++) {
            var name = seen[n][0];
            var menu = seen[n][3];
            if (!menu) { continue; }
            seen[n][1]();
            var first = menu.querySelector('button');
            if (first) {
              var extra = first.cloneNode(true);
              var word = extra.querySelector('.word');
              if (word) {
                word.textContent = 'Allow wget and fetch and everything else that a command of '
                  + 'this chat may want to run, and a word more or two besides';
              }
              menu.appendChild(extra);
            }
            var b = menu.getBoundingClientRect();
            var lines = menu.querySelectorAll('button').length;
            // A box with nothing in it is not a box the layout is asked about: the menu of the
            // servers of MCP is empty of a chat to which no server was added.
            if (lines > 0) {
              out.push({ name: name, lines: lines,
                left: Math.round(b.left), right: Math.round(b.right),
                until: Math.round(window.innerWidth) });
            }
            seen[n][2]();
          }
          return out;
        });
        for (const box of boxes) {
          if (box.left < -1 || box.right > box.until + 1) {
            throw new Error(said + ': ' + box.name + ' menu is from ' + box.left + 'px to '
              + box.right + 'px of ' + box.until + 'px');
          }
        }
      }
      if (faults.length) { throw new Error(one.name + ': page faults: ' + faults.join(' | ')); }
      // A word that grows with the state. The count of the sources of retrieval is written over
      // the badge of retrieval, and a longer word wants more breadth than the shorter one did. The
      // row is measured again when the words are written over, and it may not lay a badge over
      // another to make the longer word fit, nor lose the middle of the box.
      await page.setViewport({ width: 900, height: 820 });
      await sleep(300);
      const before = await page.evaluate(read);
      await page.evaluate(function () {
        window.__state.rag = { enabled: true, files: 12840, chunks: 999999, pending: 12,
          ready: true, vision: true };
        window.drawBadges(window.__state);
      });
      await sleep(300);
      const grown = await page.evaluate(read);
      const grew = one.name + ', ' + kind + ', a window 900 wide with the word of retrieval grown';
      if (grown.ragWord.indexOf('12840') < 0) {
        throw new Error(grew + ': the word of the badge of retrieval was not written again: '
          + before.ragWord + ' to ' + grown.ragWord);
      }
      if (grown.overlapped) { throw new Error(grew + ': ' + grown.overlapped); }
      if (grown.spill > 1) { throw new Error(grew + ': a badge is painted ' + grown.spill
        + ' past the edge of the box'); }
      if (!grown.stacked && Math.abs(grown.offCentre) > 2) {
        throw new Error(grew + ': the sandbox is ' + grown.offCentre
          + ' off the middle of the box in the face of ' + grown.face);
      }
      await page.close();
    }
  }
  await browser.close();
  console.log('no badge lies over another, none is painted outside the box, the sandbox keeps the'
    + ' middle of the box while the row keeps its columns, the words of the badges are still there'
    + ' to read at every breadth, and every drop-down of the row is kept within the window');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
