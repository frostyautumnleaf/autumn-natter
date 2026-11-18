// The mark of the program in every skin, looked at in both faces with no engine
// behind it.
//
// Each face is opened in each of the four skins, over a chat and on the first
// screen. Four things are looked at:
//   * the mark of the top bar and the mark above the chat box of the first screen
//     both stand, and are drawn from the file of the program's own mark;
//   * neither is put through a colour filter, so the leaf keeps the colours it is
//     drawn with - which is what the two skins of grey used to take from it;
//   * the skin the page is drawn in is the one the state asked for, so a filter
//     that came back could not be hidden behind another skin;
//   * the mark of the top bar is to be seen over a chat, and the mark above the
//     chat box on the first screen.
//
//   node mark-colour-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node mark-colour-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const SKINS = ['light', 'dark', 'silver', 'corporate'];
const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A chat with a leaf over it', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat with a leaf over it', repo: '', when: 'Today 09:04',
      active: true, preview: 'The leaf of the program.' }],
    commands: [{ name: '/plan', words: 'Plan the work before any of it is done.' }],
    // The first screen is where the mark of the program stands above the chat box,
    // so the look at it is taken with a chat open and then with none.
    home: false, greeting: 'Good morning.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'The leaf of the program.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04', downloads: [],
        displayed: [], suggestions: [] },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    queue: null,
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: theme === 'dark' || theme === 'corporate',
    data_dir: '/tmp/autumn-natter',
  };
}

function bridges(given) {
  window.__state = given;
  window.fetch = function (url) {
    var where = String(url);
    if (where.indexOf('/api/state') === 0) {
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
}

// What the two marks are like: whether they stand, what they are drawn from, and
// what is done to their colours.
function looked(page) {
  return page.evaluate(function () {
    var out = { skin: document.body.className, marks: [] };
    var found = document.querySelectorAll('img.mark');
    for (var n = 0; n < found.length; n++) {
      var one = found[n];
      var style = getComputedStyle(one);
      var box = one.getBoundingClientRect();
      out.marks.push({
        inBar: !!one.closest('.brand'),
        shown: box.width > 4 && box.height > 4,
        // A file of the mark, whichever way the page reaches it.
        leaf: /autumn-natter\.svg$/.test(one.getAttribute('src') || ''),
        filter: style.filter === 'none' ? 'none' : style.filter,
      });
    }
    return out;
  });
}

// The skin the page is drawn in. The pale skin is drawn with no class of its own,
// so a body that names none of the others is in it.
function skinOf(bodyClass) {
  const named = ['silver', 'corporate', 'dark'];
  for (const n of named) {
    if ((' ' + bodyClass + ' ').indexOf(' ' + n + ' ') >= 0) { return n; }
  }
  return 'light';
}

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    for (const skin of SKINS) {
      for (const screen of ['chat', 'first']) {
      const page = await browser.newPage();
      await page.setViewport({ width: 1280, height: 900 });
      const faults = [];
      page.on('pageerror', (e) => faults.push(String(e)));
      const given = stateOf(skin);
      given.home = screen === 'first';
      await page.evaluateOnNewDocument(bridges, given);
      await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
      await sleep(500);
      const notes = [];
      const note = (what) => { notes.push(what); console.log('  FAULT: ' + what); };

      const first = await looked(page);
      if (skinOf(first.skin) !== skin) {
        note('the skin asked for was ' + skin + ' and the page is drawn in ' + skinOf(first.skin));
      }
      if (first.marks.length !== 2) {
        note('two marks of the program should stand and ' + first.marks.length + ' were found');
      }
      const inBar = first.marks.filter((m) => m.inBar);
      if (inBar.length !== 1) { note('the mark of the top bar does not stand alone'); }
      for (const mark of first.marks) {
        if (!mark.leaf) { note('a mark of the program is not drawn from the file of the mark'); }
        if (mark.filter !== 'none') {
          note('a mark of the program is put through a colour filter in the ' + skin
            + ' skin: ' + mark.filter);
        }
      }

      // The mark above the chat box of the first screen has to be seen on that
      // screen; the mark of the top bar is seen on both.
      const hello = await page.evaluate(() => {
        const box = document.querySelector('.hello img.mark');
        if (!box) { return { stands: false, shown: false, filter: '' }; }
        const style = getComputedStyle(box);
        return { stands: true, shown: style.width !== '0px' && box.getBoundingClientRect().width > 8,
          filter: style.filter === 'none' ? 'none' : style.filter };
      });
      if (screen === 'first') {
        if (!hello.stands) {
          note('no mark of the program stands above the chat box of the first screen');
        } else if (!hello.shown) {
          note('the mark above the chat box of the first screen is not to be seen');
        } else if (hello.filter !== 'none') {
          note('the mark above the chat box is put through a colour filter in the '
            + skin + ' skin: ' + hello.filter);
        }
      } else if (inBar.length !== 1 || !inBar[0].shown) {
        note('the mark of the top bar is not to be seen over a chat');
      }

      for (const fault of faults) { note('a page fault: ' + fault); }
      if (notes.length || faults.length) { trouble++; }
      console.log((notes.length || faults.length ? 'FAILED ' : 'passed ')
        + one.name + ' in the ' + skin + ' skin, on the ' + screen + ' screen');
      await page.close();
      }
    }
  }
  await browser.close();
  if (trouble) { process.exit(1); }
}

main();
