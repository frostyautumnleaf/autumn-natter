// The marker of going on with an answer, checked in both pages of the program with no engine
// behind it.
//
// Five items are checked, in each page:
// * the marker is under the answer the state reports may be continued, and pressing it posts
//    continue_message with the id of that answer;
// * it is there although that answer carries no marker of a stop, which is how an answer that
//    ended on its own end-of-sequence token looks;
// * it remains nowhere else: not under an older answer that was stopped, and not under a
//    message of the user;
// * it is sent of the chat while an answer is arriving, and returns when the chat is at rest
//    again - and it returns on the row that already is there, so the marker of the row takes
//    the state of going on into account;
// * a chat whose newest saying is a message of the user returns nothing.
//
// node continue-check.js AN_PAGE=web/page.html AN_FETCH=1 node continue-check.js
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

// The word the marker of going on has in its tooltip, so the marker is found by what it is for
// and not by where it is.
const GOING = 'Go on with this answer';

function said(id, role, body, stopped, canContinue) {
  return {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', mono: false, failed: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
    stopped: !!stopped, can_continue: !!canContinue,
  };
}

// A context of two questions and two answers. The older answer was stopped by the pointer on
// the stop button, and the state no longer proposes it: only the newest answer may be
// continued. The newest answer carries no marker of a stop, for the model came to its own end -
// on an end-of-sequence token, when it had a deal left to finish.
function stateOf(theme, rev, generating, goable) {
  const going = !!goable;
  return {
    rev: rev, unchanged: false, theme: theme || 'light',
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A chat with an answer truncated', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat with an answer truncated', repo: '', when: 'Today 09:04',
      active: true, preview: 'Finish the job.' }],
    commands: [{ name: '/plan', words: 'Plan the work before any of it is done.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('a', 'You', 'Finish the job.', false, false),
      said('b', 'Assistant', 'I began the job and', true, false),
      said('c', 'You', 'Go on with it then.', false, false),
      said('d', 'Assistant', 'I started the second part and then the model', false, going),
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    queue: null,
    generating: !!generating, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// A conversation that ends on a question: there is no answer to write on from.
function asking(theme, rev) {
  const given = stateOf(theme, rev, false, '');
  given.messages.push(said('e', 'You', 'And what came of it?', false, false));
  return given;
}

// What is put into the page before it runs: the two bridges, and a book of the actions the page
// requests, so the check can read what was sent instead of watching for an answer that no
// engine would send.
function bridges(given) {
  window.__state = given;
  window.__posts = [];
  var note = function (name, body) { window.__posts.push({ name: name, body: body }); };
  window.fetch = function (url, opts) {
    var where = String(url);
    if (where.indexOf('/api/state') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
    }
    if (where.indexOf('/api/') === 0) {
      var body = {};
      try { body = JSON.parse((opts && opts.body) || '{}'); } catch (e) { body = {}; }
      note(where.slice(5), body);
    }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(window.__state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { note((args && args.path) || '', (args && args.body) || {}); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The markers that are under each message of the chat, by the id of the message. A marker is
// known by the text of its tooltip.
function marksUnder(page) {
  return page.evaluate(function (going) {
    var rows = document.querySelectorAll('#thread .turn[data-mid]');
    var out = {};
    for (var at = 0; at < rows.length; at++) {
      var mid = rows[at].dataset.mid;
      var buttons = rows[at].querySelectorAll('.stamp .acts button.act');
      var titles = [];
      for (var n = 0; n < buttons.length; n++) {
        // The tooltip of a marker is kept in `data-tip`, and `title` is taken off the button,
        // so the text of the marker is reads from wherever they remain.
        titles.push(buttons[n].getAttribute('data-tip') || buttons[n].title
          || buttons[n].getAttribute('aria-label') || '');
      }
      var count = 0;
      for (var t = 0; t < titles.length; t++) {
        if (titles[t].indexOf(going) === 0) { count++; }
      }
      out[mid] = { all: titles, going: count };
    }
    return out;
  }, GOING);
}

// Press the marker of going on under one message, and say whether it was there.
function pressGoOn(page, id) {
  return page.evaluate(function (mid, going) {
    var row = document.querySelector('#thread .turn[data-mid="' + mid + '"]');
    if (!row) { return false; }
    var buttons = row.querySelectorAll('.stamp .acts button.act');
    for (var n = 0; n < buttons.length; n++) {
      var tip = buttons[n].getAttribute('data-tip') || buttons[n].title
        || buttons[n].getAttribute('aria-label') || '';
      if (tip.indexOf(going) === 0) { buttons[n].click(); return true; }
    }
    return false;
  }, id, GOING);
}

// Say what the state is now, so the page reads it again without being opened afresh. The row of
// the answer is already in the page when this happens. The look at the state is requested at
// once, for a chat at rest is read again only after the slower pace of the poll.
function replaceState(page, next) {
  return page.evaluate(function (given) {
    window.__state = given;
    if (typeof poll === 'function') { poll(); }
  }, next);
}

async function main() {
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
    await page.evaluateOnNewDocument(bridges, stateOf('light', 1, false, true));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(500);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    // Where the marker remains, and where it does not.
    const first = await marksUnder(page);
    const want = ['a', 'b', 'c', 'd'];
    for (const id of want) {
      if (!first[id]) { note('no row of the message ' + id + ' remains'); }
    }
    if (!first.d || first.d.going !== 1) {
      note('the newest answer does not carry the marker of going on: ' +
        JSON.stringify(first.d && first.d.all));
    }
    if (first.b && first.b.going !== 0) {
      note('an older answer under a marker of a stop still proposes to be continued');
    }
    if (first.a && first.a.going !== 0) {
      note('a message of the user proposes to be continued');
    }
    if (first.c && first.c.going !== 0) {
      note('the second question proposes to be continued');
    }
    // The marker of a stop is not what the marker of going on requests any more: the answer
    // that proposes it here has no such marker, and the answer that has it does not propose.
    const told = await page.evaluate(() => {
      const rows = (cur.messages) || [];
      const out = {};
      for (let at = 0; at < rows.length; at++) { out[rows[at].id] = !!rows[at].stopped; }
      return out;
    });
    if (told.d !== false) { note('the answer that proposes to go on was informed as stopped'); }
    if (told.b !== true) { note('the older answer lost its marker of a stop'); }

    // Pressing it requests the answer to be removed on from.
    const pressed = await pressGoOn(page, 'd');
    if (!pressed) { note('the marker of going on could not be pressed'); }
    await sleep(200);
    const posts = await page.evaluate(() => window.__posts);
    const asked = posts.filter((p) => p.name === 'continue_message');
    if (asked.length !== 1) {
      note('the marker did not post continue_message once: ' + JSON.stringify(posts));
    } else if (asked[0].body.id !== 'd') {
      note('continue_message was posted for another message: ' + JSON.stringify(asked[0]));
    }

    // While an answer is arriving, nothing is proposed.
    await replaceState(page, stateOf('light', 2, true, false));
    await sleep(600);
    const live = await marksUnder(page);
    if (live.d && live.d.going !== 0) {
      note('the marker was while an answer was arriving');
    }

    // And it returns on the row that is there already, when the chat is at rest again: the row
    // is not made afresh unless the state of going on is part of what the row is known by.
    await replaceState(page, stateOf('light', 3, false, true));
    await sleep(600);
    const rested = await marksUnder(page);
    if (!rested.d || rested.d.going !== 1) {
      note('the marker did not return when the chat was at rest: ' +
        JSON.stringify(rested.d && rested.d.all));
    }

    // A chat whose newest saying is a question returns nothing.
    await replaceState(page, asking('light', 4));
    await sleep(600);
    const asked2 = await marksUnder(page);
    for (const id of Object.keys(asked2)) {
      if (asked2[id].going !== 0) {
        note('the marker is in a chat that ends on a question, under ' + id);
      }
    }
    if (!asked2.e) { note('the question that ends the conversation has no row'); }

    for (const fault of faults) { note('a page fault: ' + fault); }
    if (notes.length || faults.length) { trouble++; }
    console.log((notes.length || faults.length ? 'FAILED ' : 'passed ') + one.name);
    await page.close();
  }
  await browser.close();
  if (trouble) { process.exit(1); }
}

main();
