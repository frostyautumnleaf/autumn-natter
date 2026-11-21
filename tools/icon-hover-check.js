// Every icon of the program animates when the pointer enters it and when the pointer leaves.
// This check walks every icon that is visible on the page, moves a real pointer onto it, and
// requires:
//   * on entry the animation starts (class mark-in) and the icon is drawn larger than at rest;
//   * on leave the settle animation starts (class mark-out) and the class is removed when the
//     animation ends, so nothing is left on the element;
//   * the control the icon stands in does not move or change size while the icon moves.
//
// Icons are found by the rule the page uses: an svg element, an element carrying the svg class,
// or the program image. Elements that are hidden, too big to be icons (illustrations), covered
// by something the pointer reaches first, or outside the viewport are skipped and counted. An
// icon in a disabled control must not animate, and that is asserted rather than skipped.
//
// The view is swept in several phases, because an icon of a menu that is shut is not in the
// page: the chat, the side bar open, each badge menu open, and each settings tab open.
//
// Both pages are checked: the window of the program and the page served by --remote.
//
//   node icon-hover-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node icon-hover-check.js
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

// A chat with the parts that carry icons: an answer that thought, tool calls with their own
// marks, a failed call, files handed over, a file offer, suggestions, the steps of a run, the
// queue of waiting messages, chapters and summarisations for the timelines beside the chat, and
// servers of the user's own for the badge of them.
function stateOf() {
  const said = (id, role, body, extra) => Object.assign({
    id: String(id), role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + (id % 10), attachments: []
  }, extra || {});
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [
      { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
      { id: 'm2', name: 'DeepSeek-R1-7B-Q4_K_M', kind: 'GGUF' }
    ],
    repository: { id: 'r1', name: 'Autumn Notes' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }, { id: 'r2', name: 'Kernel Ports' }],
    chat: { id: 'c1', title: 'Icons under the pointer' },
    chats: [
      { id: 'c1', title: 'Icons under the pointer', repo: 'r1', when: 'Today 09:44',
        active: true, preview: 'Every icon answers.' },
      { id: 'c2', title: 'A chat of no repository', repo: '', when: 'Today 08:10',
        active: false, preview: 'It answers too.' }
    ],
    home: false, greeting: 'Still here, still ready.', overview: false,
    carousel: { index: 1, count: 2, blank: false },
    messages: [
      said(1, 'You', 'Run the tests, and show me the plan you wrote first.', {
        attachments: [
          { id: 'att-1', name: 'parser-diagram.png', mime: 'image/png', image: true },
          { id: 'att-2', name: 'notes.txt', mime: 'text/plain', image: false }
        ]
      }),
      said(2, 'Assistant', 'The plan is in `notes/plan.md`. The tests pass, all 6 of them.\n\n'
        + '- The bar takes its height from the words beside it.\n'
        + '- A list that follows a paragraph keeps its place.',
        { reasoning: 'The request names two things: the plan and the tests. The plan is read from '
          + 'the file, the tests are run, and both answers are put in one message.',
          reasoning_ms: 8000 }),
      said(3, 'Tool', 'cargo test markdown', {
        tool_name: 'run_command', mono: true,
        tool: 'running 6 tests\ntest markdown::tests::list_after_paragraph ... ok\n'
          + 'all 6 tests passed'
      }),
      said(4, 'Tool', 'notes/plan.md', {
        tool_name: 'edit_file',
        tool: 'Edited notes/plan.md (2 replacements)\n'
          + '@@ -2,5 +2,5 @@\n the first line of the plan\n'
          + '-flush the paragraph\n'
          + '+close the paragraph, and open the list\n the last line of the plan'
      }),
      said(5, 'Tool', 'notes/todo.txt', {
        tool_name: 'create_file', failed: true,
        tool: "Error: 'notes/todo.txt' already exists. create_file makes a new file and never "
          + 'writes over one; use edit_file to change it.'
      }),
      said(6, 'Note', 'cannot reach the model: connection refused', { failed: true }),
      said(7, 'Tool', 'the plan of the parser', {
        tool_name: 'offer_suggestions',
        tool: '3 next steps are offered for the end of your answer: Run the tests of the parser, '
          + 'Show me the diff, Read the log of the run',
        suggestions: [
          { label: 'Run the tests of the parser', prompt: 'Run the tests of the parser.' },
          { label: 'Show me the diff' },
          { label: 'Read the log of the run', prompt: 'Read the log of the run.' }
        ]
      }),
      said(8, 'Tool', 'a-page-of-the-work.html', {
        tool_name: 'offer_files',
        tool: '1 file is offered to the user in a download block: a-page-of-the-work.html',
        downloads: [{ name: 'a-page-of-the-work', extension: 'html', kind: 'page',
          path: 'a-page-of-the-work.html', size: 4213 }]
      }),
      said(9, 'Assistant', 'The work is done. The page of the run is under this answer.')
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [
      { index: 1, title: 'The plan of the parser', from: 0, to: 3, when: 'Today 09:41',
        text: 'The plan was read and changed.' },
      { index: 2, title: 'The tests of the parser', from: 3, to: 6, when: 'Today 09:44',
        text: 'The tests were run and passed.' }
    ],
    handoffs: [
      { index: 1, title: 'Handoff of the first part', from: 0, to: 2, when: 'Today 09:40',
        text: 'The parser kept a paragraph open.' }
    ],
    compaction: { type: 'handoff', by: 'percent', percent: 85, reserve: 10240, timeline: true },
    live_tools: [],
    mcp: { python: 'python3', on: ['mcpdemo0001'], servers: [
      { id: 'mcpdemo0001', name: 'Bookshop', kind: 'stdio', icon: 7, shown: true, on: true,
        tools: ['look_up', 'reserve'], fault: '' },
      { id: 'mcpdemo0002', name: 'Clock', kind: 'internal', icon: 2, shown: true, on: false,
        tools: ['now'], fault: '' },
      { id: 'mcpdemo0003', name: 'Archive', kind: 'http', icon: 9, shown: false, on: false,
        tools: [], fault: 'The server did not answer' }
    ] },
    thinking: { mode: 'medium', available: true,
      modes: ['off', 'low', 'medium', 'high', 'xhigh'],
      preserve: true, preserve_available: true },
    chatting: { collapse_thinking: false, grow_thinking: true, tools_in_thinking: false,
      collapse_tools: false, highlight: true, chapters: true, paste_limit: 6000 },
    context: { used: '2,412', total: '8,192', ratio: 0.29 },
    queue: {
      chat: 'c1', held: false,
      items: [{ id: 1, text: 'A message waiting to go.', files: [] }],
      planned: [{ id: 9, text: 'A programmed message.', match: 'the work is done', files: [] }]
    },
    generating: false, busy: false, code: true, search: true, sandbox: false,
    sandbox_available: true,
    sandbox_note: 'bubblewrap works. A command sees only the working directory of the chat, '
      + 'and it has no network.',
    tools: { groups: [] },
    status: 'ready', dark: false,
    data_dir: '/home/user/.local/share/autumn-natter',
    contexts_dir: '/home/user/.local/share/autumn-natter/contexts'
  };
}

// ---------------------------------------------------------------- page side helpers
//
// The helpers run in the page. They use the page's own rules (MARK_ICON, markFind, isMark) so
// that what is checked is what the page does, and they tag the icons of the current DOM with an
// index so a later call can name the same element.
const HELPERS = `
window.__ic = {};

// The control an icon belongs to, for the report of what the icon may move.
function controlOf(el) {
  return el.closest('button, summary, a[href], [role="button"], .thead, .shead, .row, .line');
}

// The animations running on an icon by name.
function namesOf(el) {
  var out = [];
  var list = el.getAnimations ? el.getAnimations() : [];
  for (var n = 0; n < list.length; n++) { out.push(String(list[n].animationName || '')); }
  return out;
}

// Describe one icon for the report: its own classes and the control it belongs to, with the
// title of the control when it has one.
function describeIcon(el) {
  var control = el.closest('button, a[href], summary, [role="button"], [role="menuitem"], [title]');
  var name = control ? (control.getAttribute('title') || control.id || control.className) : '';
  var tag = el.tagName.toLowerCase();
  var cls = String(el.getAttribute('class') || '').split(' ')[0];
  return tag + (cls ? '.' + cls : '') + (name ? ' in ' + String(name).split(' ')[0] : '');
}

// Tag every icon that is in the page now, and return the count.
window.__ic.collect = function () {
  var all = document.querySelectorAll(MARK_ICON);
  var kept = 0;
  for (var n = 0; n < all.length; n++) {
    var el = all[n];
    var box = el.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) { continue; }
    el.setAttribute('data-ic', String(kept));
    kept = kept + 1;
  }
  return kept;
};

// Everything the report needs about one icon before the pointer moves: where it is, whether the
// pointer can reach it at all, whether a disabled control rules the animation out, and how big
// it is drawn.
window.__ic.at = function (index, block) {
  var el = document.querySelector('[data-ic="' + index + '"]');
  if (!el) { return { gone: true }; }
  var box = el.getBoundingClientRect();
  if (box.top < 0 || box.bottom > window.innerHeight) {
    // The thread of the chat scrolls smoothly, so the jump is asked for without animation:
    // otherwise the measure below is taken while the page is still moving.
    el.scrollIntoView({ block: block || 'center', behavior: 'instant' });
    box = el.getBoundingClientRect();
  }
  var x = box.left + box.width / 2;
  var y = box.top + box.height / 2;
  window.__ic.target = el;
  // Strictly: the pointer must come to rest on the icon itself or on a part of it. The point
  // may answer with an ancestor of the icon when the icon is clipped away, as the line of
  // markers a message carries is until the message is hovered, and that is not the icon.
  var hit = document.elementFromPoint(x, y);
  var reach = hit === el || el.contains(hit) || (hit && markFind(hit) === el);
  return {
    name: describeIcon(el),
    x: Math.round(x), y: Math.round(y),
    width: Math.round(box.width), height: Math.round(box.height),
    inView: box.top >= 0 && box.bottom <= window.innerHeight,
    covered: !reach,
    disabled: !!(el.closest && el.closest('[disabled], [aria-disabled="true"]')),
    illustration: !isMark(el),
    rest: getComputedStyle(el).transform,
    controlWidth: (function () {
      var c = controlOf(el);
      if (!c) { return 0; }
      return Math.round(c.getBoundingClientRect().width);
    })(),
    controlHeight: (function () {
      var c = controlOf(el);
      if (!c) { return 0; }
      return Math.round(c.getBoundingClientRect().height);
    })()
  };
};

// A point of the message an icon belongs to, for the line of markers a message carries opens
// only while the message is hovered. The mark in such a line cannot be reached until the line is
// open, so the pointer is put on the text of the message first, which is what a hand does.
window.__ic.opener = function (index) {
  var el = document.querySelector('[data-ic="' + index + '"]');
  if (!el) { return { gone: true }; }
  var box = el.getBoundingClientRect();
  var hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
  if (hit === el || el.contains(hit) || markFind(hit).indexOf(el) >= 0) { return null; }
  var turn = el.closest('.turn');
  if (!turn) { return null; }
  var texts = turn.querySelectorAll('.body, .plain, .words, .cmd-text, .out, p, li');
  for (var n = 0; n < texts.length; n++) {
    var text = texts[n];
    if (text.querySelector(MARK_ICON)) { continue; }
    var tb = text.getBoundingClientRect();
    if (tb.width < 20 || tb.bottom < 20 || tb.top > window.innerHeight - 20) { continue; }
    return { x: Math.round(tb.left + 8),
      y: Math.round(Math.min(Math.max(tb.top + tb.height / 2, 12), window.innerHeight - 12)) };
  }
  return null;
};

// Why no point answers for this icon: the places tried, what the page found at each, and what
// the page's rule answered there. For the report of a fault only.
window.__ic.why = function (index) {
  var el = document.querySelector('[data-ic="' + index + '"]');
  if (!el) { return { gone: true }; }
  var out = [];
  var c = controlOf(el);
  var places = [['icon', el.getBoundingClientRect()]];
  if (c) { places.push(['control', c.getBoundingClientRect()]); }
  for (var n = 0; n < places.length; n++) {
    var box = places[n][1];
    var x = Math.round(box.left + box.width / 2);
    var y = Math.round(box.top + box.height / 2);
    var hit = (x > 0 && y > 0 && x < window.innerWidth && y < window.innerHeight)
      ? document.elementFromPoint(x, y) : null;
    var m = hit ? markFind(hit) : [];
    out.push(places[n][0] + '@' + x + ',' + y + ' size=' + Math.round(box.width) + 'x'
      + Math.round(box.height) + ' hit=' + (hit ? hit.tagName + '.'
        + String(hit.getAttribute && hit.getAttribute('class') || '') : 'null')
        + ' marks=' + m.length);
  }
  return out.join(' ; ');
};

// A point from which the page picks this very icon: the middle of the icon, or the middle of the
// control it stands in, or the edge of that control. A mark under a pointer that answers for the
// control, as the mark of a field does, is reached that way.
window.__ic.vantage = function (index) {
  var el = document.querySelector('[data-ic="' + index + '"]');
  if (!el) { return { gone: true }; }
  var places = [];
  var box = el.getBoundingClientRect();
  places.push([box.left + box.width / 2, box.top + box.height / 2]);
  var c = controlOf(el);
  if (c) {
    var cb = c.getBoundingClientRect();
    places.push([cb.left + cb.width / 2, cb.top + cb.height / 2]);
    places.push([cb.left + 3, cb.top + cb.height / 2]);
    places.push([cb.left + cb.width - 3, cb.top + cb.height / 2]);
  }
  for (var n = 0; n < places.length; n++) {
    var x = Math.round(places[n][0]);
    var y = Math.round(places[n][1]);
    if (x < 2 || y < 2 || x > window.innerWidth - 2 || y > window.innerHeight - 2) { continue; }
    var hit = document.elementFromPoint(x, y);
    if (!hit) { continue; }
    if (hit === el || el.contains(hit) || markFind(hit).indexOf(el) >= 0) { return { x: x, y: y }; }
  }
  return null;
};

// Where the icon aimed at last is now. The hover of one item of the page may move the page -
// the line a message's markers stand in opens when the message is hovered, and pushes the row
// out - so the pointer is aimed a second time at the place the icon came to rest at.
window.__ic.reaim = function () {
  var el = window.__ic.target;
  if (!el || !el.isConnected) { return { gone: true }; }
  var box = el.getBoundingClientRect();
  if (box.width <= 0 || box.height <= 0) { return { gone: true }; }
  return {
    x: Math.round(box.left + box.width / 2),
    y: Math.round(box.top + box.height / 2),
    inView: box.top >= 0 && box.bottom <= window.innerHeight
  };
};

// The icon the page picks for a point, held for the calls that follow. The layout may move under
// the pointer - the line a message's markers stand in opens when the message is hovered - so the
// icon is named by what is under the point after the pointer has come to rest, which is what the
// page itself answered.
window.__ic.enter = function (x, y) {
  var hit = document.elementFromPoint(x, y);
  var marks = hit ? markFind(hit) : [];
  var target = window.__ic.target;
  var one = marks.indexOf(target) >= 0 ? target : (marks[0] || null);
  window.__ic.hold = marks;
  if (!one) { return { found: false, marks: 0 }; }
  var c = controlOf(one);
  var box = c ? c.getBoundingClientRect() : null;
  return {
    found: true,
    marks: marks.length,
    named: marks.indexOf(target) >= 0,
    name: describeIcon(one),
    in: one.classList.contains('mark-in'),
    out: one.classList.contains('mark-out'),
    transform: getComputedStyle(one).transform,
    animations: namesOf(one),
    width: box ? Math.round(box.width) : 0,
    height: box ? Math.round(box.height) : 0
  };
};

// The icon held from the last hover: its classes, and whether it is still in the page.
function held() {
  var one = window.__ic.hold;
  return one && one.isConnected ? one : null;
}

window.__ic.now = function () {
  var list = [];
  var held = window.__ic.hold || [];
  for (var n = 0; n < held.length; n++) {
    if (held[n] && held[n].isConnected) { list.push(held[n]); }
  }
  if (!list.length) { return { gone: true }; }
  var ins = 0;
  var outs = 0;
  for (var k = 0; k < list.length; k++) {
    if (list[k].classList.contains('mark-in')) { ins = ins + 1; }
    if (list[k].classList.contains('mark-out')) { outs = outs + 1; }
  }
  return {
    count: list.length, in: ins, out: outs,
    transform: getComputedStyle(list[0]).transform,
    animations: namesOf(list[0])
  };
};

// A point over plain text, where the page must start no animation at all. An item is plain when
// it carries no icon, the page's own rule finds nothing for it and for the element under the
// point, and it is big enough that a point six pixels in from its left edge lands on it.
window.__ic.neutral = function () {
  var wants = document.querySelectorAll('body *');
  for (var n = 0; n < wants.length && n < 4000; n++) {
    var el = wants[n];
    if (el.querySelector(MARK_ICON)) { continue; }
    if ((el.textContent || '').trim().length < 20) { continue; }
    var box = el.getBoundingClientRect();
    if (box.width < 30 || box.height < 8) { continue; }
    if (box.bottom < 20 || box.top > window.innerHeight - 20) { continue; }
    var x = Math.round(box.left + 6);
    var y = Math.round(Math.min(Math.max(box.top + box.height / 2, 12), window.innerHeight - 12));
    if (x < 2) { continue; }
    var hit = document.elementFromPoint(x, y);
    if (!hit) { continue; }
    // The point may belong to another icon: the mark that is held settles as soon as the
    // pointer is over anything else, which is all the check asks of it.
    if (window.__ic.target && markFind(hit).indexOf(window.__ic.target) >= 0) { continue; }
    if (!(hit === el || el.contains(hit) || hit.contains(el))) { continue; }
    return { x: x, y: y };
  }
  return null;
};

// How many icons of the page are in one of the two animation classes. The sweep looks at this
// after a hover over plain text: nothing may be animating.
window.__ic.waking = function () {
  return document.querySelectorAll('.mark-in, .mark-out').length;
};
`;

// Walk the icons of one view. Each icon gets the pointer, and the pointer then goes to plain
// text, so the entry and the leave of the animation are both seen. `counts` gathers what the
// walk found, and `notes` collects what is wrong.
async function sweep(page, label, neutral, notes, counts) {
  const total = await page.evaluate(() => window.__ic.collect());
  for (let index = 0; index < total; index++) {
    const scroll = async (block) => page.evaluate((a) => window.__ic.at(a.i, a.block),
      { i: index, block: block });
    let at = await scroll('center');
    if (at.gone) { notes.push(label + ': icon ' + index + ' went out of the page'); continue; }
    if (at.illustration) { counts.illustration++; continue; }
    if (!at.inView) { counts.offscreen++; continue; }

    let aim = await page.evaluate((i) => window.__ic.vantage(i), index);
    if (aim && aim.gone) { notes.push(label + ': icon ' + index + ' went out of the page'); continue; }
    if (!aim) {
      // The mark may be at the end of the conversation, where the line above the chat composer
      // lies over it: a mark near the top of the window is free of it.
      at = await scroll('start');
      aim = await page.evaluate((i) => window.__ic.vantage(i), index);
    }
    if (!aim) {
      // Nothing answers for the mark yet: it may be in a line that opens when the message it
      // belongs to is hovered, so the text of the message is touched first.
      const opener = await page.evaluate((i) => window.__ic.opener(i), index);
      if (opener) {
        await page.mouse.move(opener.x, opener.y);
        await sleep(80);
        counts.revealed++;
        aim = await page.evaluate((i) => window.__ic.vantage(i), index);
      }
    }
    if (!aim) {
      counts.covered++;
      if (process.env.AN_IC_DEBUG === '1') {
        const why = await page.evaluate((i) => window.__ic.why(i), index);
        console.log('    covered: ' + at.name + ' at ' + at.x + ',' + at.y + ' :: ' + why);
      }
      continue;
    }

    // Let any scroll settle before the pointer moves, so the point the pointer is aimed at is
    // the point the pointer finds.
    await sleep(60);
    let point = aim;
    await page.mouse.move(point.x, point.y);
    await sleep(60);
    let woke = await page.evaluate((q) => window.__ic.enter(q.x, q.y), point);
    if (woke.found && !woke.named) {
      // The page answered for another mark, so the pointer was taken somewhere else on the way:
      // the mark may have moved, as a mark in a line that opens under the pointer does. Aim at
      // the place the mark came to and try once more.
      const again = await page.evaluate((i) => window.__ic.vantage(i), index);
      if (again && !again.gone && (again.x !== point.x || again.y !== point.y)) {
        point = again;
        await page.mouse.move(point.x, point.y);
        await sleep(60);
        woke = await page.evaluate((q) => window.__ic.enter(q.x, q.y), point);
      }
    }
    if (!woke.found || !woke.named) {
      if (at.disabled && !woke.found) { counts.disabled++; continue; }
      counts.covered++;
      if (process.env.AN_IC_DEBUG === '1') {
        const why = await page.evaluate((i) => window.__ic.why(i), index);
        console.log('    missed: ' + at.name + ' :: ' + why);
      }
      continue;
    }
    if (at.disabled) {
      notes.push(label + ': ' + woke.name + ' animates although its control is disabled');
      continue;
    }
    if (!woke.in) {
      notes.push(label + ': ' + woke.name + ' at ' + point.x + ',' + point.y
        + ' does not start an animation on hover'
        + (woke.marks ? ' (' + woke.marks + ' marks answered)' : ''));
    } else if (woke.animations.indexOf('mark-in') < 0 && woke.transform === at.rest) {
      notes.push(label + ': ' + woke.name + ' has the class but does not move ('
        + woke.transform + ')');
    } else {
      counts.animated++;
    }
    // An icon may move itself, but it may not push the control it stands in: the width and the
    // height of that control are the same while the icon is hovered as at rest. Where the icon
    // is revealed by the hover itself, as the line of a message's markers is, the position of
    // the control changes and that is what the page intends, so only the size is compared.
    if (at.controlWidth && (woke.width !== at.controlWidth
      || woke.height !== at.controlHeight)) {
      notes.push(label + ': ' + woke.name + ' changed the size of the control it stands in: '
        + at.controlWidth + 'x' + at.controlHeight + ' -> ' + woke.width + 'x' + woke.height);
    }

    // The pointer goes to plain text: the settle animation must start, and it must leave
    // nothing behind when it ends.
    const away = await page.evaluate(() => window.__ic.neutral());
    if (!away) { notes.push(label + ': no place of plain text for ' + woke.name); continue; }
    await page.mouse.move(away.x, away.y);
    const settling = await page.evaluate(() => window.__ic.now());
    if (settling.gone) { continue; }
    if (settling.out !== settling.count) {
      notes.push(label + ': ' + woke.name + ' was left with ' + settling.out + ' of '
        + settling.count + ' marks settling');
    } else {
      counts.settled += settling.count;
    }
    await sleep(420);
    const rest = await page.evaluate(() => window.__ic.now());
    if (!rest.gone && (rest.in || rest.out)) {
      notes.push(label + ': ' + woke.name + ' kept ' + (rest.in ? 'mark-in' : 'mark-out')
        + ' after the animation ended');
    }
  }
}

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const state = stateOf();
  for (const one of PAGES) {
    const notes = [];
    const counts = { animated: 0, settled: 0, skipped: 0, offscreen: 0, covered: 0,
      illustration: 0, disabled: 0, revealed: 0 };
    const faults = [];
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 900 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument((data) => {
      // The engine reports a state that has not changed, and the page then keeps what it drew.
      // The stub does the same: only the first answer is drawn, so the icons the check is
      // looking at are not replaced under it.
      var served = false;
      const once = function () {
        const out = JSON.parse(JSON.stringify(data));
        out.unchanged = served;
        served = true;
        return out;
      };
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(once()); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(once()); }
            if (command === 'get_config') { return Promise.resolve({}); }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, state);
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(500);
    if (faults.length) { notes.push('page faults: ' + faults.join(' | ')); }

    await page.evaluate(HELPERS);
    const neutral = await page.evaluate(() => window.__ic.neutral());
    if (!neutral) {
      notes.push('no place of plain text was found where no animation may start');
    }
    const move = async (x, y) => { await page.mouse.move(x, y); await sleep(70); };

    // Call a function of the page by name. A name the page does not have is reported, so a
    // phase that cannot be opened is a fault of the check rather than a crash.
    const call = async (fn, ...args) => {
      const source = '([' + fn + '][0])(' + args.map((a) => JSON.stringify(a)).join(',') + ')';
      const fault = await page.evaluate((text) => {
        try { new Function(text)(); return false; } catch (e) { return String(e); }
      }, source);
      if (fault !== false) { throw new Error(fault); }
    };
    const phases = [
      { name: 'chat', open: async () => {} },
      { name: 'side bar', open: async () => {
        await page.evaluate(() => document.body.classList.add('sideopen'));
        await sleep(400);
      } },
      { name: 'model settings', open: async () => {
        await call('openSettings', 'model');
        await sleep(400);
      }, close: async () => { await call('closeSettings'); await sleep(300); } },
      { name: 'program settings', open: async () => {
        await call('openSettings', 'program');
        await sleep(400);
      }, close: async () => { await call('closeSettings'); await sleep(300); } },
      { name: 'mcp settings', open: async () => {
        await call('openSettings', 'mcp');
        await sleep(400);
      }, close: async () => { await call('closeSettings'); await sleep(300); } },
      { name: 'tools menu', open: async () => {
        await page.evaluate(() => document.getElementById('code').click());
        await sleep(300);
      }, close: async () => { await page.evaluate(() => document.body.click()); await sleep(250); } },
      { name: 'thinking menu', open: async () => {
        await page.evaluate(() => document.getElementById('think').click());
        await sleep(300);
      }, close: async () => { await page.evaluate(() => document.body.click()); await sleep(250); } },
      { name: 'mcp menu', open: async () => {
        await page.evaluate(() => document.getElementById('mcp').click());
        await sleep(300);
      }, close: async () => { await page.evaluate(() => document.body.click()); await sleep(250); } },
    ];

    for (const phase of phases) {
      let blocked = null;
      try { await phase.open(); } catch (e) { blocked = String(e); }
      if (blocked) {
        notes.push(one.name + ' ' + phase.name + ': could not be opened: ' + blocked);
        if (phase.close) { try { await phase.close(); } catch (e) { void e; } }
        continue;
      }
      const before = counts.animated;
      await sweep(page, one.name + ' ' + phase.name, neutral, notes, counts);
      // The pointer goes to plain text: nothing may be animating there, and what animated must
      // settle back to the size it was drawn at.
      if (neutral) {
        await move(neutral.x, neutral.y);
        await sleep(400);
        const waking = await page.evaluate(() => window.__ic.waking());
        if (waking) {
          notes.push(one.name + ' ' + phase.name + ': ' + waking
            + ' icons still animating over plain text');
        }
      }
      if (phase.close) { await phase.close(); }
      process.stdout.write('  ' + one.name + ' ' + phase.name + ': '
        + (counts.animated - before) + ' icons animated\n');
    }

    // The program image in the top bar is an icon of the program and must animate as one.
    const mark = await page.evaluate(() => {
      const one = document.querySelector('img.mark');
      if (!one) { return { gone: true }; }
      one.scrollIntoView();
      const box = one.getBoundingClientRect();
      return { name: 'img.mark', x: Math.round(box.left + box.width / 2),
        y: Math.round(box.top + box.height / 2) };
    });
    if (mark.gone) {
      notes.push(one.name + ': the program image is not in the page');
    } else {
      await move(mark.x, mark.y);
      const woke = await page.evaluate(() => {
        const one = document.querySelector('img.mark');
        return { in: one.classList.contains('mark-in'), transform: getComputedStyle(one).transform };
      });
      if (!woke.in) { notes.push(one.name + ': the program image does not animate'); }
      if (woke.transform === 'none') { notes.push(one.name + ': the program image does not move'); }
      if (neutral) {
        await move(neutral.x, neutral.y);
        const settling = await page.evaluate(() => document.querySelector('img.mark')
          .classList.contains('mark-out'));
        if (!settling) { notes.push(one.name + ': the program image does not settle back'); }
      }
    }

    await page.close();
    console.log('  ' + one.name + ': animated ' + counts.animated + ', settled ' + counts.settled
      + ', disabled ' + counts.disabled + ', illustrations ' + counts.illustration
      + ', off screen ' + counts.offscreen + ', covered ' + counts.covered
      + ', revealed by hover ' + counts.revealed);
    if (!notes.length) { console.log('  ok ' + one.name); }
    else {
      notes.slice(0, 60).forEach((n) => console.log('  ERROR: ' + n));
      if (notes.length > 60) { console.log('  ... ' + (notes.length - 60) + ' more'); }
      trouble += notes.length;
    }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('every icon of the program animates on hover in and on hover out, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
