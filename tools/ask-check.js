// The card that shows a question from the model. It sits above the chat composer box and must
// have exactly the same width and left edge as the box: the two read as one column, and a card
// that sticks out on either side looks like a layout fault. Both the box and the card have sharp
// corners.
//
//   node ask-check.js
//   AN_PAGE=../web/page.html AN_FETCH=1 node ask-check.js
const path = require('path');
const { execSync } = require('child_process');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

async function main() {
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
        || process.env.HOME + '/.cache/puppeteer') + '" '
        + '-name chrome-headless-shell -type f | head -1').toString().trim();
    } catch (e) { shell = ''; }
  }
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', function (e) { errors.push(String(e)); });

  // The state the engine sends, in the shape of web::state, with a question from the model set
  // in it.
  const state = {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A token was never given' },
    chats: [{ id: 'c1', title: 'A token was never given', repo: '', when: 'Today 09:04',
      active: true, preview: 'It needs a token.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Put the page online.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
    ],
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
    ask: { id: 'q1', question: 'Which file should the work go into?',
      critical: false, left: 30, choices: ['The first one', 'The second one'] },
  };
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

  await page.setViewport({ width: 1100, height: 800 });
  await page.goto('file://' + path.resolve(PAGE));
  await new Promise(function (r) { setTimeout(r, 400); });

  // The card for the question in the state appears above the composer on its own. The appearance
  // animates with a scale transform, so the measurement is taken after it settles and reads the
  // layout rather than the painted box.
  await new Promise(function (r) { setTimeout(r, 400); });
  const shown = await page.evaluate(function () {
    var ask = document.getElementById('ask');
    var box = document.getElementById('box');
    // Read the layout, not the paint: the card scales up when it appears, and a paint measure
    // taken mid-animation reads smaller than the card actually is.
    return {
      hidden: ask.classList.contains('hidden'),
      title: document.getElementById('asktitle').textContent,
      askWidth: ask.offsetWidth, boxWidth: box.offsetWidth,
      askLeft: ask.offsetLeft, boxLeft: box.offsetLeft,
      above: ask.offsetTop + ask.offsetHeight <= box.offsetTop,
      askRadius: getComputedStyle(ask).borderTopLeftRadius,
      boxRadius: getComputedStyle(box).borderTopLeftRadius,
      askParent: ask.parentElement.id,
    };
  });

  const weighty = await page.evaluate(function () {
    drawAsk({ ask: { id: 'q2', question: 'The token was never given.', critical: true } });
    return document.getElementById('ask').classList.contains('weighty');
  });

  await browser.close();

  if (errors.length) { throw new Error('page faults: ' + errors.join(' | ')); }
  if (shown.hidden) { throw new Error('the question card did not appear'); }
  if (shown.askWidth !== shown.boxWidth || shown.askLeft !== shown.boxLeft) {
    throw new Error('the question card is ' + shown.askWidth + ' wide at left '
      + shown.askLeft + ', but the composer box is ' + shown.boxWidth + ' wide at left '
      + shown.boxLeft);
  }
  if (!shown.above) { throw new Error('the question card is not above the composer box'); }
  if (shown.askRadius !== '0px' || shown.boxRadius !== '0px') {
    throw new Error('the corners are not sharp: card ' + shown.askRadius
      + ', box ' + shown.boxRadius);
  }
  if (shown.askParent !== 'dock') { throw new Error('the card is inside ' + shown.askParent); }
  if (!weighty) { throw new Error('a critical question did not get the weighty class'); }
  console.log('the question card sits above the composer box at the same width, with sharp '
    + 'corners, and a critical question is marked as critical');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
