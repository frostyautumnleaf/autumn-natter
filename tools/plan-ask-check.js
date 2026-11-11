// Check the questionnaire of planning mode: the sheet of questions the model put
// with `ask_plan`. It stands in the talk itself, at its end, and the model is
// waited for the answers with no clock on it - so the sheet must be where the user
// can read it, and not under the answer that asked. A sheet that comes new into
// the talk is brought to the top of the chat; one that stands already is left
// where the reader left it, for the state is read again and again and a sheet that
// jumped on every read could not be answered.
//
//   node plan-ask-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node plan-ask-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// So many questions of the plan, with three answers each but the last, which the
// user is to write.
function questions(count) {
  var items = [];
  for (var n = 0; n < count; n++) {
    items.push({
      question: 'Which of the roads stands for question ' + (n + 1) + '?',
      choices: ['the first road', 'the second road', 'the third road'],
      many: n % 2 === 0
    });
  }
  items.push({ question: 'What is the name of the plan?', choices: [], many: false });
  return items;
}

function stateOf(count) {
  var messages = [];
  // A talk long enough that the end of it stands out of the chat: the questions
  // must be brought back into view rather than left below the fold.
  for (var n = 0; n < 8; n++) {
    messages.push({
      id: String(n + 1), role: n % 2 ? 'Assistant' : 'You',
      body: 'The work of the parser, the tests of it, the notes of the run, and what '
        + 'was learned on the way. Line the second, line the third, line the fourth.',
      reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false,
      failed: false, when: '09:0' + (n % 6)
    });
  }
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'The plan of the parser', },
    chats: [{ id: 'c1', title: 'The plan of the parser', repo: '', when: 'Today 09:04',
      active: true, preview: 'Plan the work.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
    // The questionnaire the model put with `ask_plan`, and is waiting on.
    ask: { id: 7, question: '', choices: [], critical: false, left: null, seconds: 0,
      plan: true, items: questions(count) }
  };
}

async function main() {
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = require('child_process').execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
        || process.env.HOME + '/.cache/puppeteer') + '" '
        + '-name chrome-headless-shell -type f | head -1').toString().trim();
    } catch (e) { shell = ''; }
  }
  const browser = await puppeteer.launch({
    headless: 'shell', executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });

  // The sheet of the questions, the room of the chat, and the marks of the answers.
  const LOOK = `(function () {
    var room = el.thread;
    var box = document.getElementById('planask');
    if (!box) { return { none: true }; }
    var b = box.getBoundingClientRect();
    var v = room.getBoundingClientRect();
    var send = box.querySelector('.pqsend');
    var s = send ? send.getBoundingClientRect() : null;
    return {
      ask: box.dataset.ask,
      questions: box.querySelectorAll('.pq').length,
      boxes: box.querySelectorAll('.pqopt input').length,
      fields: box.querySelectorAll('.pqwrite').length,
      top: Math.round(b.top), bottom: Math.round(b.bottom),
      viewTop: Math.round(v.top), viewBottom: Math.round(v.bottom),
      sendBottom: s ? Math.round(s.bottom) : null,
      scrollTop: Math.round(room.scrollTop)
    };
  })()`;

  async function open(count) {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', function (e) { errors.push(String(e)); });
    const state = stateOf(count);
    if (USE_FETCH) {
      await page.evaluateOnNewDocument(function (state) {
        window.fetch = function (url) {
          if (String(url).indexOf('/api/state') === 0) {
            return Promise.resolve({ json: function () { return Promise.resolve(state); } });
          }
          return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
        };
      }, state);
    } else {
      await page.evaluateOnNewDocument(function (state) {
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
      }, state);
    }
    await page.setViewport({ width: 1100, height: 760 });
    await page.goto('file://' + path.resolve(PAGE));
    await new Promise(function (r) { setTimeout(r, 600); });
    if (errors.length) { throw new Error('page faults: ' + errors.join(' | ')); }
    return page;
  }

  const faults = [];

  // A sheet of questions that the chat can hold: it is brought whole into view,
  // the button of send with it, so the answers can be marked and sent without the
  // reader hunting for the end of it.
  const shortPage = await open(3);
  const shortSheet = await shortPage.evaluate(LOOK);
  if (shortSheet.none) {
    throw new Error('the questionnaire of planning mode does not stand in the talk');
  }
  if (shortSheet.ask !== '7') {
    faults.push('the sheet does not stand for the ask of the state: ' + shortSheet.ask);
  }
  if (shortSheet.questions !== 4 || shortSheet.fields !== 1) {
    faults.push('the questions of the sheet are wrong: ' + shortSheet.questions
      + ' questions, ' + shortSheet.fields + ' field to write in');
  }
  if (shortSheet.top < shortSheet.viewTop) {
    faults.push('the head of the sheet stands above the chat: ' + shortSheet.top
      + ' against ' + shortSheet.viewTop);
  }
  if (shortSheet.sendBottom === null || shortSheet.sendBottom > shortSheet.viewBottom) {
    faults.push('the button of send is out of the chat: ' + shortSheet.sendBottom
      + ' against ' + shortSheet.viewBottom);
  }
  // A reader who went up in the talk is left there: the state is read again and
  // again, and the sheet that stands already is not moved about.
  const stayed = await shortPage.evaluate(async function () {
    stick = false;
    el.thread.scrollTop = 0;
    await new Promise(function (r) { setTimeout(r, 1400); });
    return Math.round(el.thread.scrollTop);
  });
  if (stayed !== 0) {
    faults.push('the talk was dragged down under a reader who went up in it: scrollTop '
      + stayed);
  }
  await shortPage.close();

  // A sheet longer than the chat: its head stands in view, so the first question
  // is the first thing read, and the rest of it is come at by scrolling down.
  const longPage = await open(12);
  const longSheet = await longPage.evaluate(LOOK);
  if (longSheet.none) { throw new Error('the long questionnaire is gone'); }
  if (longSheet.top < longSheet.viewTop) {
    faults.push('the head of a sheet longer than the chat stands above the chat: '
      + longSheet.top + ' against ' + longSheet.viewTop);
  }
  if (longSheet.bottom <= longSheet.viewBottom) {
    faults.push('a sheet longer than the chat was taken in whole, which says nothing grew');
  }
  const rest = await longPage.evaluate(async function () {
    stick = false;
    el.thread.scrollTop = el.thread.scrollHeight;
    await new Promise(function (r) { setTimeout(r, 200); });
    var box = document.getElementById('planask');
    var send = box.querySelector('.pqsend').getBoundingClientRect();
    var v = el.thread.getBoundingClientRect();
    return { sendBottom: Math.round(send.bottom), viewBottom: Math.round(v.bottom) };
  });
  if (rest.sendBottom > rest.viewBottom) {
    faults.push('the end of a long sheet cannot be reached by scrolling down: '
      + rest.sendBottom + ' against ' + rest.viewBottom);
  }
  await longPage.close();

  await browser.close();
  const face = process.env.AN_PAGE ? 'the page of the remote mode' : 'the window page';
  if (!faults.length) {
    console.log('the questionnaire of planning mode stands in the talk with its head in '
      + 'view, its button of send within reach, a sheet longer than the chat gone down '
      + 'into by scrolling, and a reader who went up in the talk left there - on ' + face);
    return;
  }
  console.log(`faults on ${face}:`);
  faults.forEach(function (f) { console.log(' - ' + f); });
  process.exitCode = 1;
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
