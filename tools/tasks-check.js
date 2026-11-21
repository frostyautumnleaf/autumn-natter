// The eight tools of the tasks suite, as the chat shows them, checked in both pages of the
// program with no engine behind it.
//
// The list of tasks belongs to the working directory of the chat, and every tool of the suite
// names the task it works on by its index, so the header of a block carries the words of its tool
// and the index or the name of the task - and `edit_task` and `append_task`, which change what a
// task says, are named the same way. Four things are checked, in each page:
// * the eight names of the suite are shapes of the page, each with a marker of its own, and the
//   two tools added last - `edit_task` and `append_task` - are not left without one;
// * the header of a finished run is the words of the shape with the item the engine sent after
//   them, so a block reads "Changed a task: <index>" and not the result of the call;
// * the read of the whole list is pointed at no task, so its header names the list;
// * the line of the badge that switches the suite on speaks of changing a task and of adding to
//   one, as well as of making, viewing, completing, cancelling and deleting.
//
// node tasks-check.js
// AN_PAGE=web/page.html node tasks-check.js
'use strict';
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'face', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'page', file: path.join(__dirname, '..', 'web', 'page.html') },
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The eight names of the suite, the item the engine sends for each, and the words the shape of
// the tool puts in front of them once the run is over.
const INDEX = 'b3f1c2a4-7d5e-4a1b-9c8f-2e6d5a4b3c1d';
const NAMED = [
  { name: 'create_task', item: 'Read the reader', done: 'Made a task: Read the reader' },
  { name: 'view_tasks', item: 'the list of tasks', done: 'Read the list of tasks' },
  { name: 'view_task', item: INDEX, done: 'Read a task: ' + INDEX },
  { name: 'edit_task', item: INDEX, done: 'Changed a task: ' + INDEX },
  { name: 'append_task', item: INDEX, done: 'Added to a task: ' + INDEX },
  { name: 'complete_task', item: INDEX, done: 'Completed a task: ' + INDEX },
  { name: 'cancel_task', item: INDEX, done: 'Cancelled a task: ' + INDEX },
  { name: 'delete_task', item: INDEX, done: 'Deleted a task: ' + INDEX },
];

// The two tools of the user's own saying, and the words the line of the badge must speak.
const BADGE = ['change the name', 'add to its content', 'complete a task', 'cancel or delete'];

function said(id, role, body) {
  return {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, attachments: [],
    tool: '', tool_name: '', mono: false, failed: false, cancelled: false, stopped: false,
    can_continue: false, when: '09:0' + id, downloads: [], displayed: [], suggestions: [],
  };
}

// One run of a tool of the suite. The engine keeps the item it was pointed at as the first line
// of the body of the message, and the result of the call as the text under the label of the
// result.
function run(id, one, result) {
  return {
    id: id, role: 'Tool', body: one.item, reasoning: '', reasoning_ms: 0, attachments: [],
    tool: result, tool_name: one.name, mono: true, failed: false, cancelled: false,
    stopped: false, can_continue: false, when: '09:0' + id, downloads: [], displayed: [],
    suggestions: [],
  };
}

function stateOf() {
  const messages = [said('1', 'You', 'Change the third task and add the new requirement to it.')];
  NAMED.forEach(function (one, n) {
    messages.push(run(String(2 + n), one, one.name === 'view_tasks'
      ? '1 tasks for this working directory: 1 open, 0 completed, 0 cancelled'
      : 'Task ' + INDEX + ' is changed.'));
  });
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A list of tasks', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A list of tasks', repo: '', when: 'Today 09:04',
      active: true, preview: 'Change the third task.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: false, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, search: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    rag: { enabled: false, files: 0, chunks: 0, pending: 0, ready: false, vision: false },
    queue: { chat: 'c1', held: false, items: [], planned: [] },
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__held = state;
  const answer = function (body) {
    return Promise.resolve({ json: function () { return Promise.resolve(body); } });
  };
  window.fetch = function (url) {
    if (String(url).indexOf('state') >= 0) { return answer(window.__held); }
    return answer({ ok: true });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command) {
        if (command === 'get_state') { return Promise.resolve(window.__held); }
        return Promise.resolve({});
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The blocks of the runs, in the order they stand in the chat.
const LOOK = function () {
  var out = [];
  var panels = document.querySelectorAll('.tool');
  for (var n = 0; n < panels.length; n++) {
    var panel = panels[n];
    var head = panel.querySelector('.thead .cmd');
    var drawn = panel.querySelector('.thead svg path');
    out.push({
      head: head ? String(head.textContent || '').replace(/\s+/g, ' ').trim() : '',
      mark: drawn ? String(drawn.getAttribute('d') || '') : '',
    });
  }
  return { blocks: out, shapes: window.TOOL_SHAPES || {} };
};

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

    // The line of the badge, as the page speaks it.
    const words = fs.readFileSync(one.file, 'utf8');
    const at = words.indexOf("'Tasks suite',");
    if (at < 0) {
      note('the page holds no line of the badge for the tasks suite');
    } else {
      const said = words.slice(at, at + 1200);
      BADGE.forEach(function (wanted) {
        if (said.indexOf(wanted) < 0) {
          note('the line of the badge says nothing of "' + wanted + '": ' + said.slice(0, 240));
        }
      });
    }

    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 900 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString()
      + '; window.__LOOK = ' + LOOK.toString() + ';');
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(600);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }

    const made = await page.evaluate('window.__LOOK()');
    if (made.blocks.length < NAMED.length) {
      note('the chat holds ' + made.blocks.length + ' blocks of calls, of ' + NAMED.length
        + ' that were asked for');
    } else {
      NAMED.forEach(function (named, n) {
        const block = made.blocks[n];
        const shape = made.shapes[named.name];
        if (!shape) {
          note('the page has no shape for ' + named.name);
          return;
        }
        if (!shape.mark) { note('the shape of ' + named.name + ' has no marker'); }
        if (block.mark !== shape.mark) {
          note('the marker of the block of ' + named.name + ' is not the marker of its shape');
        }
        if (block.head !== named.done) {
          note('the header of ' + named.name + ' says "' + block.head + '" against "'
            + named.done + '"');
        }
      });
      // The two tools that change what a task says are known by a marker of their own, and not
      // by the marker of the tool that only reads one.
      const marks = {};
      NAMED.forEach(function (named) {
        const shape = made.shapes[named.name];
        if (!shape) { return; }
        if (marks[shape.mark]) {
          note('the shape of ' + named.name + ' shares its marker with ' + marks[shape.mark]);
        }
        marks[shape.mark] = named.name;
      });
    }
    await page.evaluate(function () {
      var thread = document.getElementById('thread');
      if (thread) { thread.scrollTop = 0; }
    });
    await sleep(180);
    await page.screenshot({ path: path.join(__dirname, 'tasks-' + one.tag + '-1-runs.png') });
    await page.evaluate(function () {
      var thread = document.getElementById('thread');
      if (thread) { thread.scrollTop = 700; }
    });
    await sleep(180);
    await page.screenshot({ path: path.join(__dirname, 'tasks-' + one.tag + '-2-changed.png') });
    if (faults.length) { note('page faults at the end: ' + faults.join(' | ')); }
    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
    await page.close();
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the eight tools of the tasks suite are shapes of both pages, each with a marker of '
    + 'its own, every block of a run is headed by the words of its tool and the task it was '
    + 'pointed at, the read of the list names the list, and the line of the badge speaks of '
    + 'changing a task and of adding to one');
}

main().catch((e) => { console.error(e); process.exit(1); });
