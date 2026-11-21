// The result of a command that printed a large file is in the chat whole. The program used to
// cut such a result to a few thousand characters, the start of it and the end of it with a line
// between saying the middle was gone; that text is gone with the cut, and what a model reads of
// a file is what the command printed. The check is that the page holds all of it, that the
// result scrolls inside its own block instead of stretching the chat, and that no word of a cut
// is left anywhere on the page.
//
// Both pages are checked: the window of the program and the page served by --remote.
//
//   node output-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node output-check.js
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

// How many lines the command of the scene printed. Four thousand lines is near 150 kB of text:
// what a model brings back when it reads a large file with one command, and many times what the
// cut of the result kept.
const LINES = 4000;

// The whole of the result, as the program hands it over: every line of it, in order.
function printed() {
  let text = '';
  for (let n = 1; n <= LINES; n++) { text += `line ${n} of the file the command read\n`; }
  return text;
}

// The state of the engine, as the record of web::state: one chat, and the last item of it a run
// of the terminal tool whose result is the whole of a large file.
function stateOf() {
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'The whole of a large file' },
    chats: [{ id: 'c1', title: 'The whole of a large file', repo: '', when: 'Today 09:04',
      active: true, preview: 'cat notes/big.md' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Read notes/big.md to me.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Tool', body: 'cat notes/big.md', reasoning: '', reasoning_ms: 0,
        tool: printed(), mono: true, failed: false, tool_name: 'run_terminal',
        tool_args: '{"command":"cat notes/big.md"}', when: '09:05', attachments: [] },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '19,304', total: '8,192', ratio: 1.0 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  window.fetch = function (url) {
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

// The block of the run, the result it holds, and what the page does with them. The block is the
// last of the runs, which is the command of the scene.
function read() {
  const runs = document.querySelectorAll('.tool');
  const last = runs[runs.length - 1];
  if (!last) { return { found: false }; }
  const out = last.querySelector('.out');
  const panel = last.getBoundingClientRect();
  const thread = document.querySelector('.thread');
  const text = out ? out.textContent : '';
  return {
    found: true,
    runs: runs.length,
    characters: text.length,
    begins: text.startsWith('line 1 of the file the command read\n'),
    ends: text.endsWith('line ' + (window.__LINES || 4000) + ' of the file the command read\n'),
    lines: text ? text.replace(/\n$/, '').split('\n').length : 0,
    cut: /truncat/i.test(document.body.innerText),
    scrollable: out ? out.scrollHeight > out.clientHeight : false,
    outHeight: out ? Math.round(out.clientHeight) : 0,
    panelHeight: Math.round(panel.height),
    windowHeight: window.innerHeight,
    threadHeight: thread ? Math.round(thread.scrollHeight) : 0,
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

    const seen = await page.evaluate(read);
    if (!seen.found) {
      note('no block of a run is in the chat, so the result was not read');
    } else {
      // The whole of the result is in the page: the first line and the last of them, and every
      // line between by count.
      if (seen.lines !== LINES) {
        note(`the result holds ${seen.lines} lines of the ${LINES} the command printed`);
      }
      if (!seen.begins) {
        note('the result does not begin with the first line the command printed');
      }
      if (!seen.ends) {
        note(`the result does not end with line ${LINES}, the last of what the command printed`
          + ` (${seen.characters} characters are in the block)`);
      }
      if (seen.cut) {
        note('the page still carries the words of a cut result');
      }
      // The result is scrolled inside its block: the block is not stretched to the whole of the
      // text, or a chat that read one large file would be a chat of one enormous block.
      if (!seen.scrollable) {
        note(`the result of ${seen.characters} characters is not scrollable in its block`);
      }
      if (seen.panelHeight > seen.windowHeight) {
        note(`the block of the run is ${seen.panelHeight} high against a window of `
          + `${seen.windowHeight}: it should scroll its result, not grow`);
      }
      // The block is one part of the chat: the conversation is not the length of the result.
      if (seen.threadHeight > seen.panelHeight * 3 + 2000) {
        note(`the chat is ${seen.threadHeight} high for one block of ${seen.panelHeight}`);
      }
    }

    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log(`the whole of a ${LINES}-line result is in the chat, scrolled inside its block,`
    + ' with no word of a cut, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
