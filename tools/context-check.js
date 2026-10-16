// Check that the meter of the context moves while an answer comes in, and does
// not wait for the answer to stop.
//
// The page is loaded as it stands, with a bridge that hands out a list of
// states one after another, the way the engine does during an answer: the
// number of the talk grows, and the words of the answer grow with it. The
// numbers the page shows are then read back. A meter that only moves when the
// answer is over shows one number for the whole run, and that is the fault.
//
//   node context-check.js                  # the window page
//   AN_PAGE=../web/page.html AN_FETCH=1 node context-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// One message of the talk.
function said(id, role, body) {
  return { id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + id };
}

// The numbers of one look. The words the talk takes grow a bit at a time, as
// they do while the model writes.
const STEPS = [
  { used: 0, live: '' },
  { used: 306, live: '' },
  { used: 416, live: '' },
  { used: 431, live: 'The Danube' },
  { used: 440, live: 'The Danube, the Rhine' },
  { used: 452, live: 'The Danube, the Rhine and the Vistula,' },
  { used: 461, live: 'The Danube, the Rhine and the Vistula, all of them' },
  { used: 470, live: 'The Danube, the Rhine and the Vistula, all of them long.' },
];

// The state of the engine, as the record of web::state. The total of the room
// stands at 2048, so the numbers of the steps are the numbers of a real answer
// of the testing model.
function stateOf(step, dark) {
  const total = 2048;
  return {
    rev: step.n + 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-9B-Q4_K_M', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-9B-Q4_K_M', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'Rivers of Europe' },
    chats: [{ id: 'c1', title: 'Rivers of Europe', repo: '', when: 'Today 09:04',
      active: true, preview: 'Three rivers' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    carousel: { index: 0, count: 1, blank: false },
    messages: [said(1, 'You', 'Name three rivers of Europe in one short line.')],
    live: step.live, reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [],
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true },
    context: { used: String(step.used), total: String(total),
      ratio: Math.min(1, step.used / total) },
    generating: true, busy: false, code: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'thinking', dark: !!dark
  };
}

const STATES = STEPS.map(function (step, n) { return stateOf(Object.assign({ n: n }, step)); });

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();

  // The bridge hands out the states one after another, on the pace of the poll
  // of the page: one new state every two looks, so a poll always has something
  // new to draw.
  await page.evaluateOnNewDocument(function (states, fetchMode) {
    var asked = 0;
    var held = function () {
      var at = Math.min(states.length - 1, Math.floor(asked / 2));
      return JSON.parse(JSON.stringify(states[at]));
    };
    if (fetchMode) {
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          asked++;
          return Promise.resolve({ json: function () { return Promise.resolve(held()); } });
        }
        asked++;
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      return;
    }
    window.__TAURI__ = {
      core: {
        invoke: function (command) {
          if (command === 'get_state') {
            asked++;
            return Promise.resolve(held());
          }
          if (command === 'get_config') { return Promise.resolve({}); }
          return Promise.resolve({ ok: true });
        }
      }
    };
  }, STATES, USE_FETCH);

  await page.goto('file://' + path.resolve(PAGE));

  // Read the meter of the page as it stands, again and again, while the answer
  // comes in.
  const seen = [];
  const words = [];
  for (let n = 0; n < 40; n++) {
    const now = await page.evaluate(function () {
      var meter = document.getElementById('ctxtext');
      var body = document.getElementById('col');
      return { meter: meter ? meter.textContent : '', thread: body ? body.textContent : '' };
    });
    if (!seen.length || seen[seen.length - 1] !== now.meter) { seen.push(now.meter); }
    if (!words.length || words[words.length - 1] !== now.thread) { words.push(now.thread); }
    await new Promise(function (r) { setTimeout(r, 120); });
  }
  await browser.close();

  const last = String(STEPS[STEPS.length - 1].used);
  const faults = [];
  if (seen.length < 4) {
    faults.push('the meter moved ' + seen.length + ' times only, so it does not follow the answer');
  }
  if (!seen[seen.length - 1] || seen[seen.length - 1].indexOf(last) < 0) {
    faults.push('the meter ends on ' + JSON.stringify(seen[seen.length - 1])
      + ', and not on ' + last);
  }
  if (seen[0] && seen[0].indexOf(last) === 0) {
    faults.push('the meter stands on the last number from the first look');
  }
  console.log('the meter read, in order: ' + JSON.stringify(seen));
  if (faults.length) {
    console.log('FAULTS\n' + faults.join('\n'));
    process.exit(1);
  }
  console.log('the meter of the context moved ' + (seen.length - 1)
    + ' times while the answer came in, and ended on ' + last);
})();
