// The block of the messages waiting to go is as wide as the chat composer it sits above, to the
// pixel: the same left edge and the same width, with the list open or shut and with programmed
// messages in their own section of it. A block one side narrower than the box reads as a mistake
// of the layout, and the check fails on that.
//
// Both pages are checked: the window of the program and the page served by --remote.
//
//   node queue-width-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node queue-width-check.js
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

// The queue is kept by the program, and the page draws what it is told about: the messages of
// the chat, the ones the user held back, and the programmed messages of their own section.
function stateOf() {
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A queue the same width as the box' },
    chats: [{ id: 'c1', title: 'A queue the same width as the box', repo: '', when: 'Today 09:04',
      active: true, preview: 'The same width.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Queue them up.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
    ],
    live: 'An answer still being written, so the queue has a reason to be in the chat.',
    reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: true, busy: true, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'writing', dark: false, data_dir: '/tmp/autumn-natter',
    queue: {
      chat: 'c1', held: false,
      items: [
        { id: 1, text: 'The oldest message, which is sent first.', files: [] },
        { id: 2, text: 'A second message, with a file hung on it.', files: ['notes-for-the-chat.md'] }
      ],
      planned: [
        { id: 9, text: 'A programmed message, held for an answer that says its text.',
          match: 'the work is done', files: [] }
      ]
    },
  };
}

function stubs(state) {
  window.__actions = [];
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(state); } }); }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { window.__actions.push({ name: String((args && args.path) || '') }); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The two blocks, as they stand in the page.
function read() {
  const box = document.querySelector('.box').getBoundingClientRect();
  const queue = document.getElementById('queue').getBoundingClientRect();
  const head = document.getElementById('queuehead').getBoundingClientRect();
  const open = document.getElementById('queue').classList.contains('open');
  const list = document.getElementById('queuelist').getBoundingClientRect();
  return {
    shown: !document.getElementById('queue').classList.contains('hidden'),
    open: open,
    boxLeft: Math.round(box.left), boxWidth: Math.round(box.width),
    queueLeft: Math.round(queue.left), queueWidth: Math.round(queue.width),
    headLeft: Math.round(head.left), headWidth: Math.round(head.width),
    listLeft: open ? Math.round(list.left) : null,
    listWidth: open ? Math.round(list.width) : null,
    rows: document.querySelectorAll('#queuelist .qrow').length,
  };
}

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };
    const faults = [];
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 820 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(450);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }

    const shut = await page.evaluate(read);
    if (!shut.shown) {
      note('the queue of the waiting messages is not in the chat, so its width was not read');
    } else {
      if (shut.queueLeft !== shut.boxLeft) {
        note('the queue starts at ' + shut.queueLeft + ' and the chat composer at '
          + shut.boxLeft + ': their left edges differ');
      }
      if (shut.queueWidth !== shut.boxWidth) {
        note('the queue is ' + shut.queueWidth + ' wide and the chat composer '
          + shut.boxWidth + ': they are not the same width');
      }
      if (shut.headLeft !== shut.boxLeft || shut.headWidth !== shut.boxWidth) {
        note('the header of the queue is ' + shut.headWidth + ' wide at ' + shut.headLeft
          + ', against the composer of ' + shut.boxWidth + ' at ' + shut.boxLeft);
      }
    }

    // The list opens under the header, and it is the width of the block it belongs to.
    await page.evaluate(function () { document.getElementById('queuehead').click(); });
    await sleep(250);
    const open = await page.evaluate(read);
    if (!open.open) {
      note('the list of the waiting messages did not open');
    } else {
      if (open.listLeft !== open.boxLeft || open.listWidth !== open.boxWidth) {
        note('the open list is ' + open.listWidth + ' wide at ' + open.listLeft
          + ', against the composer of ' + open.boxWidth + ' at ' + open.boxLeft);
      }
      if (open.queueWidth !== open.boxWidth) {
        note('with its list open the queue is ' + open.queueWidth + ' wide, not ' + open.boxWidth);
      }
    }

    // The width is kept when the window is narrow, where the blocks are cut by the space they
    // have rather than by a margin of their own.
    await page.setViewport({ width: 720, height: 820 });
    await sleep(350);
    const narrow = await page.evaluate(read);
    if (!narrow.shown) {
      note('in a narrow window the queue went out of the chat');
    } else if (narrow.queueLeft !== narrow.boxLeft || narrow.queueWidth !== narrow.boxWidth) {
      note('in a narrow window the queue is ' + narrow.queueWidth + ' wide at ' + narrow.queueLeft
        + ', against the composer of ' + narrow.boxWidth + ' at ' + narrow.boxLeft);
    }

    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the queue of the waiting messages is the width of the chat composer, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
