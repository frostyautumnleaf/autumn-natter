// The chat is measured against the place it is in, and the side bar is between the two: hide the
// side bar and the chat may be wider, show it again and the chat must come back to the width that
// fits. The side bar slides with a transition, so a measure taken in the same breath as the class
// was changed reads the place before the slide - which is what left the column too wide after the
// side bar was hidden and shown again, until the side bar was dragged or the window resized.
// Both pages are checked: the window of the program and the page served by --remote.
//
//   node side-unhide-check.js
//   node side-unhide-check.js shots      # frames of the three states, for the eye
//   AN_PAGE=web/page.html AN_FETCH=1 node side-unhide-check.js
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'web', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const shots = process.argv[2] === 'shots';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The column is wider than the place it has with the side bar open, so the width the chat is
// given is decided by the place and not by the setting: what the check reads is the decision.
function stateOf() {
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A chat of that repository' },
    chats: [{ id: 'c1', title: 'A chat of that repository', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'How wide is the chat now?' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'How wide is the chat now?', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Model', body: 'It depends on the space the side bar leaves it.',
        reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false,
        when: '09:05' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true, chapters: true,
      paste_limit: 6000, width: 1200, userline: false, seamless: false, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  // A program that has nothing new to say answers that nothing changed, and the page draws
  // nothing on such an answer: asking for the place the chat is in is not a change the state
  // speaks of, so a page that redrew itself at every ask would measure again by accident.
  window.__asked = 0;
  var nextState = function () {
    window.__asked += 1;
    var out = {};
    for (var key in state) { out[key] = state[key]; }
    out.rev = window.__asked;
    out.unchanged = window.__asked > 1;
    return out;
  };
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(nextState()); } }); }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(nextState()); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { window.__actions.push({ name: String((args && args.path) || '') }); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The place the chat is in, the width the column is given, and the width it is painted at.
function read() {
  const area = document.getElementById('area');
  const col = document.getElementById('col');
  return {
    open: document.body.classList.contains('sideopen'),
    area: area ? area.clientWidth : 0,
    colw: (getComputedStyle(document.documentElement).getPropertyValue('--colw') || '').trim(),
    col: col ? Math.round(col.getBoundingClientRect().width) : 0,
  };
}

// The measures the report says come right: the window is made a pixel wider and a pixel narrower
// again, which asks the page for the place the chat is in.
async function resizeNudge(page, wide, high, named) {
  await page.setViewport({ width: wide + 1, height: high });
  await sleep(120);
  await page.setViewport({ width: wide, height: high });
  await sleep(320);
  if (shots && named) {
    await page.screenshot({ path: path.join(__dirname, 'n2-side-' + named + '.png') });
  }
  return page.evaluate(read);
}

// One click of the marker of the top bar, read while the side bar is still on its way and again
// once it has come to rest.
async function clickAndRead(page, where, named) {
  await page.click('#sidebtn');
  await sleep(60);
  const moving = await page.evaluate(read);
  await sleep(520);
  if (shots) {
    await page.screenshot({ path: path.join(__dirname, 'n2-side-' + named + '.png') });
  }
  const settled = await page.evaluate(read);
  return { way: where, moving: moving, settled: settled };
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
    await page.setViewport({ width: 1280, height: 860 });
    page.on('pageerror', (e) => faults.push(String(e)));

    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(500);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }

    const atFirst = await page.evaluate(read);
    if (shots) {
      await page.screenshot({ path: path.join(__dirname, 'n2-side-' + one.tag + '-1-open.png') });
    }
    if (!atFirst.open) {
      note('the side bar is not open at the first, so hide and show cannot be asked of it');
    } else {
      // The width the same layout comes to when the window is resized by hand, which the report
      // names as the only thing that makes the page measure again.
      const openByHand = await resizeNudge(page, 1280, 860, one.tag + '-2-open-by-hand');
      const hid = await clickAndRead(page, 'hide', one.tag + '-3-hidden');
      const hidByHand = await resizeNudge(page, 1280, 860, one.tag + '-4-hidden-by-hand');
      const shown = await clickAndRead(page, 'show', one.tag + '-5-back');
      const shownByHand = await resizeNudge(page, 1280, 860, one.tag + '-6-back-by-hand');

      if (atFirst.col !== openByHand.col) {
        note('the column is ' + atFirst.col + 'px when the page has been drawn and ' + openByHand.col
          + 'px once the window is resized by hand, so the place the chat is in was never measured '
          + 'at the first');
      }
      if (hid.settled.area <= atFirst.area) {
        note('the place the chat is in did not grow when the side bar was hidden: ' + atFirst.area
          + 'px then ' + hid.settled.area + 'px');
      }
      if (hid.settled.col <= openByHand.col) {
        note('hiding the side bar left the column at ' + hid.settled.col + 'px, when with the bar '
          + 'open it is ' + openByHand.col + 'px and the place the chat has is wider');
      }
      // The two reports of one layout: what the slide leaves behind, and what a resize of the
      // window comes to. The same in both directions, for the slide is the same in the other.
      if (hid.settled.col !== hidByHand.col) {
        note('hiding the side bar leaves the column ' + hid.settled.col + 'px wide, when the same '
          + 'layout reached by resizing the window is ' + hidByHand.col + 'px');
      }
      if (shown.settled.col !== shownByHand.col) {
        note('showing the side bar leaves the column ' + shown.settled.col + 'px wide, when the '
          + 'same layout reached by resizing the window is ' + shownByHand.col
          + 'px: the place the chat is in was not measured again after the slide');
      }
      if (shown.settled.area !== shownByHand.area) {
        note('the place the chat is in is ' + shown.settled.area + 'px after the side bar came '
          + 'back and ' + shownByHand.area + 'px after a resize');
      }
      if (shownByHand.col !== openByHand.col) {
        note('the side bar open gives the column ' + openByHand.col + 'px at the first and '
          + shownByHand.col + 'px after it went and came back');
      }
      console.log('  ' + one.name + ': ' + openByHand.col + 'px open, ' + hidByHand.col
        + 'px hidden, ' + shownByHand.col + 'px back again; the slide left '
        + hid.settled.col + 'px and ' + shown.settled.col + 'px');
    }
    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the width of the chat follows the side bar both ways, in both pages'
    + (shots ? ' (shots taken)' : ''));
}

main().catch((e) => { console.error(e); process.exit(1); });
