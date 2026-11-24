// The two choices in the top bar - the model that answers, and the repository the chats of the
// window belong to - are boxes with a marker at the left, the choice filling the rest of it, and
// the arrow of the list at the right. The rule that sets the marker at the left was written for
// the marker of the box and took the arrow with it, and a box with both of its edges named keeps
// the left one: the arrow was drawn over the marker, which is how the two choices came to have no
// arrow at all. This tool measures the two boxes as each of the six skins paints them, on both
// faces, and asks that the arrow stands at the right edge of its box, clear of the marker and of
// the chip of the file type, and that it keeps still under the pointer - the arrow of a choice
// says that there is a list, it is not itself a thing to hold (task N3).
//
//   node topbar-arrow-check.js          # the reading of both faces
//   node topbar-arrow-check.js shots    # the same, with a frame of the top bar of every skin
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'web', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const THEMES = ['light', 'dark', 'silver', 'corporate', 'neon', 'glass'];
const FIELDS = ['.bar .field.tagged', '.bar .field:not(.tagged)'];
const shell = process.env.AN_SHELL_BIN || '';
const shots = process.argv[2] === 'shots';
const pixel = process.env.AN_PIXEL === '1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme, theme_pixel: pixel,
    model: { id: 'm1', name: 'Qwen3.5-9B-Q4_K_M-of-the-long-name', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-9B-Q4_K_M-of-the-long-name', kind: 'GGUF' }],
    repository: { id: 'r1', name: 'Autumn Notes, the long one' },
    repositories: [{ id: 'r1', name: 'Autumn Notes, the long one' }],
    chat: { id: 'c1', title: 'A chat of that repository' },
    chats: [{ id: 'c1', title: 'A chat of that repository', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'Where is the arrow of the choice?' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Where is the arrow of the choice?', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Model', body: 'At the right edge of its box, where it was meant to stand.',
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
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

function stubs(state) {
  window.__actions = [];
  var copy = Object.assign({}, state);
  // The page reads the theme out of the state it asks for again and again, so a skin is put on
  // by changing what the state says and asking the page to look at it once.
  window.__skin = function (word, pix) {
    copy.theme = word;
    copy.theme_pixel = !!pix;
    if (window.showTheme) { window.showTheme(word); }
    if (window.showPixelArt) { window.showPixelArt(!!pix); }
  };
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(copy); } }); }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(copy); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { window.__actions.push({ name: String((args && args.path) || '') }); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// One box of the top bar, read as it stands. The places are given from the two edges of the box,
// and are all that is wanted: that the marker stands at the left, the arrow at the right, the
// chip of the file type between the arrow and the word of the choice, and nothing over anything.
function readField(which) {
  const box = document.querySelector(which);
  if (!box) { return { missing: true }; }
  const wide = box.getBoundingClientRect();
  const at = function (node) {
    const r = node.getBoundingClientRect();
    const seen = getComputedStyle(node);
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return {
      left: Math.round((r.left - wide.left) * 10) / 10,
      right: Math.round((wide.right - r.right) * 10) / 10,
      width: Math.round(r.width * 10) / 10,
      height: Math.round(r.height * 10) / 10,
      stroke: seen.stroke, size: seen.width + ' x ' + seen.height,
      display: seen.display, visibility: seen.visibility, opacity: seen.opacity,
      transform: seen.transform,
      // The marks over the choice let the pointer through, so a point in the middle of the arrow
      // answers with the box of the choice: a click on the arrow opens the list.
      opens: !!(hit && hit.closest(which)),
    };
  };
  const arrow = box.querySelector('.arrow');
  const mark = box.querySelector(':scope > .svg:not(.arrow)');
  const tag = box.querySelector('.tag');
  const select = box.querySelector('select');
  if (!arrow || !mark || !select) { return { missing: true, why: 'no arrow, no marker or no choice' }; }
  const out = {
    missing: false,
    name: ((select.options[select.selectedIndex] || {}).text) || '',
    wide: Math.round(wide.width * 10) / 10,
    arrow: at(arrow), mark: at(mark), tag: tag ? at(tag) : null,
  };
  return out;
}

// The skin of the page, as the page itself puts it on the body.
async function skinOf(page, theme) {
  await page.evaluate(function (which, pix) {
    window.__skin(which, pix);
  }, theme, pixel);
  await page.mouse.move(2, 2);
  await sleep(240);
}

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const faults = [];
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 860 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf('light')) + ');');
    await page.goto(process.env.AN_PAGE_URL || ('file://' + one.file), { waitUntil: 'domcontentloaded' });
    await sleep(600);
    if (faults.length) { trouble++; console.log('  ERROR: ' + one.name + ' page faults: ' + faults.join(' | ')); }

    for (const theme of THEMES) {
      await skinOf(page, theme);
      const tag = one.tag + '-' + theme + (pixel ? '-pixel' : '');
      for (const which of FIELDS) {
        const at = await page.evaluate(readField, which);
        if (at.missing) {
          trouble++;
          console.log('  ERROR: ' + tag + ' has no box of the top bar at ' + which);
          continue;
        }
        const bad = [];
        if (at.arrow.display === 'none' || at.arrow.visibility === 'hidden'
          || Number(at.arrow.opacity) === 0) {
          bad.push('the arrow is not drawn at all (' + at.arrow.display + '/'
            + at.arrow.visibility + '/' + at.arrow.opacity + ')');
        }
        // The arrow stands at the right edge of the box, with the nine pixels of air the marker
        // has at the left.
        if (Math.abs(at.arrow.right - 9) > 1) {
          bad.push('the arrow stands ' + at.arrow.right + 'px from the right edge of the box,'
            + ' where nine were asked');
        }
        if (at.arrow.left < at.wide / 2) {
          bad.push('the arrow stands at ' + at.arrow.left + 'px from the left of a box '
            + at.wide + 'px wide, which is not the right edge of it');
        }
        // It is clear of the marker at the left and of the chip of the file type.
        const markerEnd = at.mark.left + at.mark.width;
        if (at.arrow.left - markerEnd < 8) {
          bad.push('the arrow comes within ' + Math.round((at.arrow.left - markerEnd) * 10) / 10
            + 'px of the marker of the choice');
        }
        // The chip of the file type stands in the box too, and its right edge is what the arrow
        // must keep clear of: the arrow is the last thing in the box, the chip the one before it.
        if (at.tag) {
          const chipEnd = at.wide - at.tag.right;
          if (at.arrow.left < chipEnd) {
            bad.push('the arrow stands over the chip of the file type, whose right edge is at '
              + Math.round(chipEnd * 10) / 10 + 'px of a box ' + at.wide + 'px wide');
          } else if (at.arrow.left - chipEnd > 14) {
            bad.push('the arrow stands ' + Math.round((at.arrow.left - chipEnd) * 10) / 10
              + 'px away from the chip of the file type, which is no air but a gap');
          }
        }
        if (!at.arrow.opens) {
          bad.push('a click on the arrow is not a click on the choice');
        }
        if (bad.length) {
          trouble++;
          console.log('  ERROR: ' + tag + ' ' + which + ' [' + at.name + ']: ' + bad.join('; '));
        }
        console.log('  ' + tag + ' ' + which + ' [' + at.name + '] box ' + at.wide + 'px: arrow at '
          + at.arrow.left + 'px from the left, ' + at.arrow.right + 'px from the right, '
          + at.arrow.size + ' ' + at.arrow.stroke + ' opacity ' + at.arrow.opacity
          + '; marker at ' + at.mark.left + 'px wide ' + at.mark.width + 'px; chip '
          + (at.tag ? at.tag.right + 'px from the right' : 'none'));
      }
      // The arrow of a choice says that there is a list: it is not itself a thing to hold, so it
      // keeps still while the box answers the pointer.
      const still = await page.evaluate(async function (which) {
        const box = document.querySelector(which);
        const arrow = box ? box.querySelector('.arrow') : null;
        if (!arrow) { return { missing: true }; }
        const before = getComputedStyle(arrow).transform;
        box.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
        box.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
        await new Promise(function (r) { setTimeout(r, 260); });
        return { before: before, after: getComputedStyle(arrow).transform };
      }, FIELDS[0]);
      await page.hover(FIELDS[0]);
      await sleep(320);
      const hovered = await page.evaluate(function (which) {
        const arrow = document.querySelector(which + ' .arrow');
        return arrow ? getComputedStyle(arrow).transform : 'gone';
      }, FIELDS[0]);
      if (!still.missing && hovered !== still.before) {
        trouble++;
        console.log('  ERROR: ' + tag + ' the arrow of the choice moves under the pointer ('
          + still.before + ' then ' + hovered + ')');
      }
      if (shots) {
        const bar = await page.$('.bar');
        const where = 'n11-' + tag;
        if (bar) { await bar.screenshot({ path: path.join(__dirname, where + '.png') }); }
        await page.screenshot({ path: path.join(__dirname, where + '-whole.png') });
      }
    }
    await page.close();
  }
  await browser.close();
  console.log((pixel ? 'pixel form; ' : '') + 'failures: ' + trouble);
}

main().catch(function (e) { console.log('FAIL', String(e)); process.exit(1); });
