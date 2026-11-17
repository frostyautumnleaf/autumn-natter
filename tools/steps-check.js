// The block of the steps of an answer: the room the two buttons of paging stand
// in, and what a step longer than the one before it, or longer than the chat, does
// to the room of the chat.
//
// The buttons of going back and going on stand under the words of the step, in the
// corners of the same area, and they used to lean on those words: 3px between the
// last line and the top of the button, with 12px of ground under the button. They
// now stand with the same room above them as below.
//
// A step with more lines than the step before it takes its room above, out of the
// answer, so the block keeps its bottom edge and the row of the step buttons stays
// where it was. A step longer than the chat itself has no room above to take, and
// holding the bottom edge then leaves the head of the block - the name of the list,
// the count, and the first lines of the step - above the chat, out of reach of any
// scroll. The chat therefore stands at the head of the block in that case, and the
// rest of the step is read by scrolling down. The same holds when a long block is
// drawn for the first time at the end of a chat that follows its end.
//
//   node steps-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node steps-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// The words of a step, in so many lines. A line of the step is a line of the
// block, so the height of a step is told by the lines it holds.
function lines(count) {
  var out = [];
  for (var n = 0; n < count; n++) { out.push('Say what changed, what the tests cost, and what is still open.'); }
  return out.join('\n');
}

function stateOf(steps) {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'The steps of the work', },
    chats: [{ id: 'c1', title: 'The steps of the work', repo: '', when: 'Today 09:04',
      active: true, preview: 'Show the plan.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Show the plan of the work as steps.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      // The answer the steps stand under. It is long on purpose: the block of the
      // steps must stand at the end of a chat that is longer than the window, for
      // that is the only chat where a step has room to take above it.
      { id: '2', role: 'Assistant', body: lines(26), reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:05' },
      { id: '3', role: 'Tool', body: 'The steps of the work stand under your answer.',
        tool: 'the steps of the work', tool_name: 'show_steps', steps: steps,
        mono: false, failed: false, when: '09:05' },
      { id: '4', role: 'Assistant', body: 'The steps of the work stand under this answer.',
        reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false,
        failed: false, when: '09:06' },
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
  };
}

// The block of the steps, the room of the chat, and where the parts of the block
// stand. Everything is read in the measures of the window, for the fault is a
// fault of the room and not of the words.
const LOOK = `(function () {
  var room = el.thread;
  var box = room.querySelector('.steps');
  if (!box) { return { none: true }; }
  var v = room.getBoundingClientRect();
  var head = box.querySelector('.stitle').getBoundingClientRect();
  var text = box.querySelector('.stext').getBoundingClientRect();
  var row = box.querySelector('.srow').getBoundingClientRect();
  var next = box.querySelector('.snext:not(.gone)');
  var back = box.querySelector('.sprev:not(.gone)');
  var nav = (next || back) ? (next || back).getBoundingClientRect() : null;
  return {
    at: box.dataset.at,
    viewTop: Math.round(v.top), viewBottom: Math.round(v.bottom),
    headTop: Math.round(head.top), textBottom: Math.round(text.bottom),
    rowTop: Math.round(row.top), rowBottom: Math.round(row.bottom),
    navTop: nav ? Math.round(nav.top) : null,
    navBottom: nav ? Math.round(nav.bottom) : null,
    gapTextNav: nav ? Math.round(nav.top - text.bottom) : null,
    gapNavRow: nav ? Math.round(row.top - nav.bottom) : null,
    scroll: [Math.round(room.scrollTop), Math.round(room.scrollHeight),
      Math.round(room.clientHeight)],
    stuck: room.scrollHeight - (room.scrollTop + room.clientHeight) < 4
  };
})()`;

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

  // The room of a short step, the room of a step of a dozen lines, and the room
  // of a step longer than the chat. The three are the cases the block is asked to
  // stand in the same chat, one after another.
  const shortFirst = {
    title: 'The plan for the parser',
    steps: [
      { label: 'Read the fault', content: 'Read the fault of the run and say which line of the parser gives it.' },
      { label: 'Write the parser', content: lines(12) },
      { label: 'Run the tests', content: lines(46) }
    ]
  };
  // The long step first, so the block is drawn at the end of the chat with more
  // lines in it than the chat has.
  const longFirst = {
    title: 'The plan for the parser',
    steps: [
      { label: 'Write the parser', content: lines(46) },
      { label: 'Read the fault', content: 'Read the fault of the run.' }
    ]
  };

  async function open(steps) {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', function (e) { errors.push(String(e)); });
    const state = stateOf(steps);
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
    await new Promise(function (r) { setTimeout(r, 500); });
    if (errors.length) { throw new Error('page faults: ' + errors.join(' | ')); }
    return page;
  }

  const page = await open(shortFirst);
  const look = function (p) { return p.evaluate(LOOK); };
  const click = async function (p, selector) {
    await p.evaluate(function (s) {
      var one = document.querySelector('.steps ' + s);
      if (one) { one.click(); }
    }, selector);
    await new Promise(function (r) { setTimeout(r, 320); });
  };

  const first = await look(page);
  if (first.none) { throw new Error('no block of the steps of the work stands in the chat'); }
  // The room the buttons of paging stand in: the same ground above them as below,
  // so neither the words of the step nor the line over the row of the steps is
  // touched.
  if (first.gapTextNav === null) { throw new Error('no button of paging stands over the step'); }
  if (first.gapTextNav < 10) {
    throw new Error('the button of paging leans on the words of the step: ' + first.gapTextNav + 'px');
  }
  if (first.gapNavRow < 10) {
    throw new Error('the button of paging leans on the line over the row of the steps: '
      + first.gapNavRow + 'px');
  }
  if (Math.abs(first.gapTextNav - first.gapNavRow) > 3) {
    throw new Error('the button of paging does not stand between even ground: '
      + first.gapTextNav + 'px above, ' + first.gapNavRow + 'px below');
  }

  // A step with more lines than the step before it takes its room above, out of
  // the answer: the bottom edge of the block, and the row of the step buttons with
  // it, stay where they stood.
  await click(page, '.snext');
  const grown = await look(page);
  if (grown.at !== '1') { throw new Error('the block of the steps did not go on to the second step'); }
  if (Math.abs(grown.rowBottom - first.rowBottom) > 2) {
    throw new Error('the row of the step buttons moved with the step: from ' + first.rowBottom
      + ' to ' + grown.rowBottom);
  }
  if (grown.rowBottom > grown.viewBottom) {
    throw new Error('the row of the step buttons went out of the chat: ' + grown.rowBottom
      + ' against ' + grown.viewBottom);
  }
  if (grown.headTop < grown.viewTop) {
    throw new Error('the head of the block went above the chat: ' + grown.headTop
      + ' against ' + grown.viewTop);
  }

  // A step longer than the chat has no room above to take. The chat stands at the
  // head of the block, so the name of the list and the first lines of the step are
  // read, and the rest of the step is come at by scrolling down.
  await click(page, '.snext');
  const tall = await look(page);
  if (tall.at !== '2') { throw new Error('the block of the steps did not go on to the long step'); }
  if (tall.headTop < tall.viewTop) {
    throw new Error('the head of a step longer than the chat stands above the chat: '
      + tall.headTop + ' against ' + tall.viewTop);
  }
  if (tall.rowBottom <= tall.viewBottom) {
    throw new Error('a step longer than the chat was taken in without the rest of it '
      + 'standing below the fold, which says nothing grew');
  }
  const rest = await page.evaluate(async function () {
    // The scroll of a reader going down into the long step: the chat stops
    // following its end the moment the hand of the user moves, and nothing of the
    // page puts them back at the head of the block.
    stick = false;
    el.thread.scrollTop = el.thread.scrollHeight;
    await new Promise(function (r) { setTimeout(r, 120); });
    var box = document.querySelector('.steps');
    var row = box.querySelector('.srow').getBoundingClientRect();
    return { rowBottom: Math.round(row.bottom),
      viewBottom: Math.round(el.thread.getBoundingClientRect().bottom) };
  });
  if (rest.rowBottom > rest.viewBottom) {
    throw new Error('the rest of a long step cannot be read by scrolling down: the row of '
      + 'the step buttons stands at ' + rest.rowBottom + ' against ' + rest.viewBottom);
  }
  await page.close();

  // The same block drawn for the first time with a step longer than the chat: the
  // chat follows its end, and the head of the block is kept in view rather than
  // the last lines of that one step.
  const fresh = await open(longFirst);
  const drawn = await look(fresh);
  if (drawn.none) { throw new Error('no block of the steps of the work stands in the chat'); }
  if (drawn.headTop < drawn.viewTop) {
    throw new Error('a block drawn with a step longer than the chat hides its head: '
      + drawn.headTop + ' against ' + drawn.viewTop);
  }
  await fresh.close();

  await browser.close();
  console.log('the buttons of paging stand between even ground under the words of the '
    + 'step; a step with more lines takes its room above and keeps the row of the '
    + 'step buttons where it stood; a step longer than the chat stands at the head of '
    + 'the block, the rest of it is read by scrolling down, and a block drawn with '
    + 'such a step shows its head too');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
