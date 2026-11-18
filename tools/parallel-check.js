// Several chats answered at once, looked at in both faces of the program with no
// engine behind it.
//
// Five things are looked at, in each face:
//   * the mark of a new chat stands, and the side bar lets another chat be
//     opened, while an answer is being written: the program answers as many chats
//     at once as the model was started with room for;
//   * every chat that is being answered wears a beating mark in the side bar,
//     whether the window looks at it or not, and an idle chat wears none;
//   * the mark goes out of the bar when the answer is over, which means the bar is
//     made anew when the list of chats at work moves;
//   * opening another chat, and starting a new one, while an answer comes in post
//     what they always posted, and nothing else;
//   * the words the program says when a model setting is changed during an answer
//     are shown on the line of state, and the settings still went out.
//
//   node parallel-check.js
//   node parallel-check.js shots
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    {
      name: 'the window', tag: 'window',
      file: path.join(__dirname, '..', 'frontend', 'index.html')
    },
    {
      name: 'the browser page', tag: 'page',
      file: path.join(__dirname, '..', 'web', 'page.html')
    }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The words the program is asked to say when a model setting is moved while an
// answer is on the way. The settings go out all the same, and hold on the next
// turn of the chat.
const TAKES_EFFECT = 'The changed model settings will take effect on a new turn';

function said(id, role, body) {
  return {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', mono: false, failed: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
    stopped: false, can_continue: false,
  };
}

// The chats of the side bar stand in `window.__lists`, which the bridges below
// hand to the page: two chats belong to no repository and one stands in the
// repository of the machine. The open one is `c1`, which is also the chat that is
// answered in the states below.

// The state of the window. `working` holds the ids of the chats that are being
// answered, and `generating` says whether the chat the window looks at is one of
// them. `status` is what the program said last.
function stateOf(theme, rev, generating, working, status) {
  return {
    rev: rev, unchanged: false, theme: theme || 'light',
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'The chat being answered', planning: false, planned: false },
    chats: [],
    commands: [{ name: '/plan', words: 'Plan the work before any of it is done.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('a', 'You', 'Finish the job.'),
      said('b', 'Assistant', 'I began the job and read the files.'),
    ],
    live: generating ? 'The work goes on here' : '', reasoning: '', reasoning_ms: 0,
    reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    queue: null,
    generating: !!generating, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    // The chats the model is at work on. More than one of them may stand here.
    working: working || [],
    status: status || 'ready', dark: false, data_dir: '/tmp/autumn-natter',
    structure_rev: 1,
  };
}

// What is put into the page before it runs: the two bridges, and a book of the
// actions the page asks for, so the check can read what went out.
function bridges(given) {
  window.__state = given;
  window.__posts = [];
  var note = function (name, body) { window.__posts.push({ name: name, body: body }); };
  window.fetch = function (url, opts) {
    var where = String(url);
    if (where.indexOf('/api/state') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
    }
    if (where.indexOf('/api/lists') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(window.__lists); } });
    }
    if (where.indexOf('/api/') === 0) {
      var body = {};
      try { body = JSON.parse((opts && opts.body) || '{}'); } catch (e) { body = {}; }
      note(where.slice(5), body);
    }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__lists = {
    repositories: [{
      id: 'r1', name: 'autumn-natter', pinned: false,
      chats: [{ id: 'c3', title: 'A chat of the repository', repo: 'r1', when: 'Today 09:03',
        pinned: false, archived: false, active: false, messages: 12,
        preview: 'The last words of A chat of the repository' }],
    }],
    loose: [
      { id: 'c1', title: 'The chat being answered', repo: '', when: 'Today 09:01',
        pinned: false, archived: false, active: true, messages: 8,
        preview: 'The last words of The chat being answered' },
      { id: 'c2', title: 'A chat at rest', repo: '', when: 'Today 09:02',
        pinned: false, archived: false, active: false, messages: 3,
        preview: 'The last words of A chat at rest' },
    ],
    archived: [],
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(window.__state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') {
          var where = (args && args.path) || '';
          if (where === 'lists') { return Promise.resolve(window.__lists); }
          note(where, (args && args.body) || {});
        }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The rows of the side bar, by the id of the chat, with the marks they wear.
function rows(page) {
  return page.evaluate(function () {
    var out = {};
    var found = document.querySelectorAll('#sideBody .srow[data-id]');
    for (var n = 0; n < found.length; n++) {
      var row = found[n];
      out[row.dataset.id] = {
        work: row.classList.contains('work'),
        on: row.classList.contains('on'),
        title: row.getAttribute('data-tip') || row.getAttribute('title') || '',
        visible: row.offsetParent !== null,
      };
    }
    return out;
  });
}

// Whether a button of the window stands, as the user sees it.
function shown(page, id) {
  return page.evaluate(function (one) {
    var node = document.getElementById(one);
    if (!node) { return 'missing'; }
    var style = window.getComputedStyle(node);
    return (style.display === 'none' || style.visibility === 'hidden') ? 'hidden' : 'shown';
  }, id);
}

// Press the row of one chat in the side bar, and say whether it was there.
function pressRow(page, id) {
  return page.evaluate(function (one) {
    var row = document.querySelector('#sideBody .srow[data-id="' + one + '"]');
    if (!row) { return false; }
    row.click();
    return true;
  }, id);
}

// Press one button of the window by its id.
function press(page, id) {
  return page.evaluate(function (one) {
    var node = document.getElementById(one);
    if (!node) { return false; }
    node.click();
    return true;
  }, id);
}

// What the page asked the program for since the look began.
function asked(page) {
  return page.evaluate(function () { return window.__posts; });
}

// Say what the state is now, and make the page look at it.
function replaceState(page, next) {
  return page.evaluate(function (given) {
    window.__state = given;
    if (typeof poll === 'function') { poll(); }
  }, next);
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
    await page.setViewport({ width: 1280, height: 900 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    const busy = stateOf('light', 1, true, ['c1', 'c3'], 'The answer is being written');
    await page.evaluateOnNewDocument(bridges, busy);
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(600);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  FAULT: ' + what); };

    console.log('\n== ' + one.name);

    // A new chat is started, and another chat opened, while an answer comes in.
    if (await shown(page, 'newchat') !== 'shown') {
      note('the mark of a new chat is hidden while an answer comes in');
    }
    if (await shown(page, 'sideNew') !== 'shown') {
      note('the mark of a new chat in the side bar is hidden while an answer comes in');
    }

    const marked = await rows(page);
    for (const id of ['c1', 'c2', 'c3']) {
      if (!marked[id]) { note('no row of the chat ' + id + ' stands in the bar'); }
    }
    if (marked.c1 && !marked.c1.work) { note('the chat being answered wears no mark'); }
    if (marked.c3 && !marked.c3.work) { note('a chat answered out of sight wears no mark'); }
    if (marked.c2 && marked.c2.work) { note('a chat at rest wears the mark of an answer'); }
    if (marked.c3 && marked.c3.title.indexOf('being answered') < 0) {
      note('the mark of a chat at work says nothing of itself: "' + marked.c3.title + '"');
    }

    // The look of the bar while two chats are answered at once.
    await page.screenshot({ path: path.join(__dirname, 'parallel-' + (one.tag || 'face') + '.png') });

    // Opening another chat, and starting a new one, while the answer comes in.
    {
      const okRow = await pressRow(page, 'c2');
      if (!okRow) { note('the row of the idle chat could not be pressed'); }
      const okNew = await press(page, 'newchat');
      if (!okNew) { note('the mark of a new chat could not be pressed'); }
      await sleep(200);
      const gone = await asked(page);
      const names = gone.map((p) => p.name);
      if (names.indexOf('open_chat') < 0) { note('opening another chat asked for nothing'); }
      const opened = gone.filter((p) => p.name === 'open_chat')[0];
      if (opened && opened.body.id !== 'c2') {
        note('opening the idle chat named the wrong chat: ' + JSON.stringify(opened.body));
      }
      if (names.indexOf('new_chat') < 0) { note('starting a new chat asked for nothing'); }
    }

    // A model setting moved during an answer: the words of the program are the
    // ones asked for, and the setting still went out.
    await replaceState(page, stateOf('light', 2, true, ['c1', 'c3'], TAKES_EFFECT));
    await sleep(250);
    const line = await page.evaluate(function () {
      var node = document.querySelector('.status');
      return node ? node.textContent : '';
    });
    if (line !== TAKES_EFFECT) {
      note('the line of state says something else: "' + line + '"');
    }

    // The answers are over. The bar is made anew, and no chat wears a mark.
    await replaceState(page, stateOf('light', 3, false, [], 'ready'));
    await sleep(300);
    const rest = await rows(page);
    for (const id of ['c1', 'c2', 'c3']) {
      if (rest[id] && rest[id].work) { note('the mark of ' + id + ' stayed on after the answer'); }
    }

    if (await shown(page, 'newchat') !== 'shown') {
      note('the mark of a new chat does not stand with the chat at rest');
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
    (want ? ' (shots asked for)' : ''));
  if (trouble) { process.exit(1); }
}

main().catch((e) => { console.error(e); process.exit(1); });
