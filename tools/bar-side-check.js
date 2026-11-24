// The side bar and the top bar are one piece: both are painted with the surface colour, the seam
// between them is gone, and the corner where the hairline of the bar meets the hairline of the
// side bar is rounded. The check reads the two backgrounds, the hairline that starts under the
// bar, and the corner that joins them, and asks that the pixel-art form of a theme, which sets
// every radius to nothing, leaves the corner square instead.
//
// Both pages are checked: the window of the program and the page served by --remote carry the
// same rules.
//
//   node bar-side-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node bar-side-check.js
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

// The page asks for its state on its own. The theme travels with it, as it does from the
// program, so the pixel-art form can be asked for as well.
function stateOf() {
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A chat of that repository' },
    chats: [{ id: 'c1', title: 'A chat of that repository', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'One piece, not two.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'One piece, not two.', reasoning: '', reasoning_ms: 0,
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

// The two pieces, their backgrounds, and the two parts that make the corner. `hairTop` is where
// the hairline of the side bar begins, counted from the top of the side bar.
function read() {
  const bar = document.querySelector('.bar');
  const side = document.getElementById('side');
  const before = getComputedStyle(side, '::before');
  const after = getComputedStyle(side, '::after');
  const radius = Number(getComputedStyle(side).getPropertyValue('--r-m').replace('px', '').trim());
  return {
    open: document.body.classList.contains('sideopen'),
    barBack: getComputedStyle(bar).backgroundColor,
    sideBack: getComputedStyle(side).backgroundColor,
    sideBorder: getComputedStyle(side).borderRightWidth,
    cornerImage: before.backgroundImage,
    cornerWidth: before.width,
    hairTop: after.top,
    hairWidth: after.width,
    barBottom: Math.round(bar.getBoundingClientRect().bottom),
    radius: radius,
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
    await page.setViewport({ width: 1280, height: 860 });
    page.on('pageerror', (e) => faults.push(String(e)));

    // The program opens the side bar from what it remembers, so the class is put on here by
    // hand, as the button of the top bar puts it on.
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(400);
    await page.evaluate(function () { document.body.classList.add('sideopen'); });
    await sleep(250);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }
    const plain = await page.evaluate(read);

    if (!plain.open) { note('the side bar is not open, so nothing of it was read'); }
    if (plain.barBack !== plain.sideBack) {
      note('the side bar is "' + plain.sideBack + '" and the top bar is "' + plain.barBack
        + '": they do not blend');
    }
    if (plain.sideBorder !== '0px') {
      note('the side bar still carries its own right border of ' + plain.sideBorder
        + ', so a seam is drawn against the top bar');
    }
    if (plain.cornerImage.indexOf('radial-gradient') < 0) {
      note('no rounded corner is drawn where the side bar meets the top bar: "'
        + plain.cornerImage + '"');
    }
    if (plain.cornerWidth !== plain.radius + 'px') {
      note('the corner is ' + plain.cornerWidth + ' wide, not the radius of ' + plain.radius + 'px');
    }
    const wants = plain.barBottom + plain.radius - 1;
    if (Math.round(parseFloat(plain.hairTop)) !== wants) {
      note('the hairline of the side bar begins ' + plain.hairTop + ' from the top of the side '
        + 'bar, not after the corner of ' + wants + 'px');
    }
    if (plain.hairWidth !== '1px') {
      note('the hairline of the side bar is ' + plain.hairWidth + ' wide');
    }

    // The pixel-art form sets every radius to nothing, so the corner must come back square and
    // the hairline must start at the bottom of the bar.
    const pixel = await page.evaluate(function () {
      document.body.classList.add('pixel');
      return new Promise(function (done) {
        setTimeout(function () {
          const side = document.getElementById('side');
          const bar = document.querySelector('.bar');
          done({
            radius: Number(getComputedStyle(side).getPropertyValue('--r-m').replace('px', '').trim()),
            cornerWidth: getComputedStyle(side, '::before').width,
            hairTop: getComputedStyle(side, '::after').top,
            barBottom: Math.round(bar.getBoundingClientRect().bottom),
            sideBack: getComputedStyle(side).backgroundColor,
            barBack: getComputedStyle(bar).backgroundColor,
          });
        }, 250);
      });
    });
    if (pixel.radius !== 0) { note('the pixel-art form did not take the radius away: ' + pixel.radius); }
    if (pixel.cornerWidth !== '0px') {
      note('the corner is ' + pixel.cornerWidth + ' wide in the pixel-art form, not square');
    }
    if (Math.round(parseFloat(pixel.hairTop)) !== pixel.barBottom - 1) {
      note('in the pixel-art form the hairline begins at ' + pixel.hairTop
        + ' instead of at the bottom of the bar');
    }
    if (pixel.sideBack !== pixel.barBack) {
      note('the two blend in the colour forms and not in the pixel-art form: "' + pixel.sideBack
        + '" against "' + pixel.barBack + '"');
    }

    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the side bar and the top bar are one piece, with a rounded corner, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
