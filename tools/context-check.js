// Check that the meter of the context moves while an answer comes in, does not
// wait for the answer to stop, and rolls up to its new number instead of
// jumping to it.
//
// The page is loaded as it stands, with a bridge that hands out a list of
// states one after another, the way the engine does during an answer: the
// number of the talk grows, and the words of the answer grow with it. The
// numbers the page shows are then read back. A meter that only moves when the
// answer is over shows one number for the whole run, and that is the fault. A
// meter that is set straight from the state shows only the numbers it is
// given, and never one of the roll between them. The last state goes back, for
// a chat that has been tidied, and there the number must fall at once.
//
//   node context-check.js                  # the window page
//   AN_PAGE=../web/page.html AN_FETCH=1 node context-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// The numbers of the meter are written with a comma in the place of a
// thousand, the way util::count writes them.
function count(value) {
  var digits = String(value);
  var out = '';
  while (digits.length > 3) {
    out = ',' + digits.slice(digits.length - 3) + out;
    digits = digits.slice(0, digits.length - 3);
  }
  return digits + out;
}

// One message of the talk.
function said(id, role, body) {
  return { id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + id };
}

// The numbers of one look. The words the talk takes grow a bit at a time, as
// they do while the model writes, and they go over a thousand so the comma in
// the place of a thousand is looked at too. The last step goes back: a chat
// that has been tidied has fewer words in it than it had, and the number of
// the meter must fall there and then, not walk down.
const STEPS = [
  { used: 0, live: '' },
  { used: 306, live: '' },
  { used: 416, live: '' },
  { used: 431, live: 'The Danube' },
  { used: 986, live: 'The Danube, the Rhine' },
  { used: 1042, live: 'The Danube, the Rhine and the Vistula,' },
  { used: 1099, live: 'The Danube, the Rhine and the Vistula, all of them' },
  { used: 1156, live: 'The Danube, the Rhine and the Vistula, all of them long.' },
  { used: 402, live: 'The Danube, the Rhine and the Vistula, all of them long.' },
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
    context: { used: count(step.used), total: count(total),
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
  // of the page: one new state every three looks, so a poll always has
  // something new to draw, and each number the engine sends has time to be
  // stood on before the next one is asked for.
  await page.evaluateOnNewDocument(function (states, fetchMode) {
    var asked = 0;
    var held = function () {
      var at = Math.min(states.length - 1, Math.floor(asked / 3));
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
  const read = [];
  const wrong = [];
  for (let n = 0; n < 64; n++) {
    const now = await page.evaluate(function () {
      var meter = document.getElementById('ctxtext');
      var body = document.getElementById('col');
      var line = meter ? meter.textContent : '';
      var room = document.getElementById('ctxpct');
      return { meter: line, head: Number(String(line).split(' ')[0].replace(/[^0-9]/g, '')),
        pct: room ? room.textContent : '', thread: body ? body.textContent : '' };
    });
    if (!seen.length || seen[seen.length - 1] !== now.meter) { seen.push(now.meter); }
    read.push(now.head);
    // The mark of the hundredth stands outside the number that is rolled, so
    // the two of them must always make one number and one mark.
    if (!/^[0-9]+%$/.test(now.pct) && wrong.indexOf(now.pct) < 0) { wrong.push(now.pct); }
    if (!words.length || words[words.length - 1] !== now.thread) { words.push(now.thread); }
    await new Promise(function (r) { setTimeout(r, 110); });
  }
  await browser.close();

  const last = STEPS[STEPS.length - 1];
  const before = STEPS[STEPS.length - 2];
  const faults = [];
  if (seen.length < 4) {
    faults.push('the meter moved ' + seen.length + ' times only, so it does not follow the answer');
  }
  if (!seen[seen.length - 1] || seen[seen.length - 1].indexOf(count(last.used)) < 0) {
    faults.push('the meter ends on ' + JSON.stringify(seen[seen.length - 1])
      + ', and not on ' + count(last.used));
  }
  if (seen[0] && seen[0].indexOf(count(last.used)) === 0) {
    faults.push('the meter stands on the last number from the first look');
  }

  // Every number the engine sent has to be stood on in the end, so that a
  // number of the roll on its way up is not mistaken for the meter.
  STEPS.forEach(function (step) {
    if (read.indexOf(step.used) < 0) {
      faults.push('the meter never stood on ' + count(step.used));
    }
  });

  if (wrong.length) {
    faults.push('the hundredths of the meter read ' + JSON.stringify(wrong)
      + ', and one number wants one mark of the hundredth');
  }

  // The numbers between the ones the engine sent belong to the roll: a meter
  // that is set straight from the state only ever shows the numbers it is
  // given, and the reader sees it jump.
  const sent = {};
  STEPS.forEach(function (step) { sent[step.used] = true; });
  const rolled = read.filter(function (n) { return n > 0 && !sent[n]; });
  if (rolled.length < 2) {
    faults.push('the number was seen between the numbers of the engine ' + rolled.length
      + ' times only, so it jumps instead of rolling up');
  }

  // A number that goes back is taken where it stands. Nothing of the room
  // between the two may be shown on the way down, for the talk was never that
  // length.
  let up = -1;
  let down = -1;
  for (let n = 0; n < read.length; n++) { if (read[n] === before.used) { up = n; } }
  for (let n = up + 1; n < read.length; n++) { if (read[n] === last.used) { down = n; break; } }
  if (up < 0 || down < 0) {
    faults.push('the fall of the meter from ' + count(before.used) + ' to '
      + count(last.used) + ' was not seen at all');
  } else {
    const slid = read.slice(up + 1, down)
      .filter(function (n) { return n < before.used && n > last.used; });
    if (slid.length) {
      faults.push('the meter walked down through ' + JSON.stringify(slid)
        + ', and a number going back should be taken at once');
    }
  }

  console.log('the meter read, in order: ' + JSON.stringify(seen));
  console.log('the numbers between them, of the roll: ' + JSON.stringify(rolled));
  if (faults.length) {
    console.log('FAULTS\n' + faults.join('\n'));
    process.exit(1);
  }
  console.log('the meter of the context moved ' + (seen.length - 1)
    + ' times while the answer came in, on ' + rolled.length
    + ' numbers of the roll, and ended on ' + count(last.used));
})();
