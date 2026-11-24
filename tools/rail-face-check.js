// The two lines beside the chat, checked in both pages with no engine behind
// them: the drawing is driven manually, so the window face and the browser face
// are read the same way.
//
//   node rail-face-check.js
//   AN_PAGE=../web/page.html node rail-face-check.js   # one page only
//
// Seven items are checked, for each of the two lines:
//   * a line with space to spare keeps the distance between its nodes and shows
//     all of them, with no marker at either end - it looks as it did before the
//     line became a window;
//   * a line with more nodes than the space is tall for is cut to the space, keeps
//     the same distance between its nodes, and remains centred in the sandbox directory;
//   * it shows the end of the line where the newest node remains, and the marker
//     for the newer end is removed while the other one is there;
//   * the rows the window does not show are hidden, so a card of one of them
//     never lies over the chat, and nothing that is shown hangs out of the
//     window;
//   * a wheel over the line slides the line, and not the chat behind it;
//   * a press of a marker moves the window by part of a screenful;
//   * while the user is looking at another part of the line, a node arriving
//     does not drag them back, and when the look has expired the line returns to
//     the newest node of itself.
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

// What each remaining line is made of. Every line is drawn from a conversation with this
// many nodes in it, first with few enough to fit and then with far too many.
const SCRIPT = `
window.__talk = function (many) {
  var messages = [], handoffs = [];
  for (var n = 1; n <= many; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'Question ' + n + ' with a long tail of words', when: '12:0' + (n % 10), attachments: [] });
    messages.push({ id: 'm' + n, role: 'Assistant', body: 'Answer ' + n, when: '12:0' + (n % 10), attachments: [] });
    handoffs.push({ id: 'h' + n, number: n, state: 'done', when: '2026-10-21, 12:0' + (n % 10),
      before: 9000 + n, after: 900, snippet: 'The talk about number ' + n });
  }
  return { messages: messages, handoffs: handoffs, compaction: {} };
};

// One line, drawn the way the page draws it.
window.__draw = function (which, many) {
  var state = window.__talk(many);
  cur = { messages: state.messages, handoffs: state.handoffs, compaction: {} };
  chatting.timeline = true; chatting.userline = true;
  if (which === 'timeline') { drawTimeline(state); }
  if (which === 'userline') { drawUserLine(state); }
};

// The line as it stands: how far the window has slid, and what it shows. The
// track glides over 170 ms, so this is only read once the glide is over.
window.__read = function (which) {
  var box = document.getElementById(which);
  var nodes = Array.from(box.querySelectorAll('.railtrack > *'));
  var top = nodes.map(function (n) { return n.getBoundingClientRect().top; });
  var marks = {
    up: box.querySelector('.railmark.up'),
    down: box.querySelector('.railmark.down')
  };
  var seen = function (m) {
    var style = getComputedStyle(m);
    return style.display !== 'none' && style.visibility !== 'hidden';
  };
  var here = box.getBoundingClientRect();
  var shown = nodes.filter(function (n) { return !n.classList.contains('out'); });
  var inWindow = shown.every(function (n) {
    var r = n.getBoundingClientRect();
    return r.top >= here.top - 1 && r.bottom <= here.bottom + 1;
  });
  return {
    many: nodes.length,
    gap: top.length > 1 ? Math.round((top[1] - top[0]) * 10) / 10 : 0,
    height: Math.round(here.height),
    centred: Math.abs((here.top + here.bottom) / 2 - (document.getElementById('stage').getBoundingClientRect().top
      + document.getElementById('stage').getBoundingClientRect().bottom) / 2) <= 2,
    more: box.classList.contains('railmore'),
    up: seen(marks.up), down: seen(marks.down),
    hidden: nodes.length - shown.length,
    inWindow: inWindow,
    off: Math.round(box.__off || 0), max: Math.round(box.__max || 0),
    last: shown.length ? Math.round(shown[shown.length - 1].getBoundingClientRect().bottom) : 0,
    bottom: Math.round(here.bottom)
  };
};

// A wheel of the user's own, sent to the line as a real one would come.
window.__wheel = function (which, deltaY) {
  var box = document.getElementById(which);
  box.dispatchEvent(new WheelEvent('wheel', { deltaY: deltaY, bubbles: true, cancelable: true }));
};

window.__mark = function (which, place) {
  document.getElementById(which).querySelector('.railmark.' + place).click();
};
`;

const LINES = ['timeline', 'userline'];

// How tall a line of three nodes is, and how tall the window of a line that overflows is. The
// first is the height of three nodes and the two distances between them; the second is the
// space of the stage with the margin the line keeps at either end taken off.
const SHORT_HEIGHT = { timeline: 75, userline: 51 };
const EDGE = 26;

// The distance from the top of one node to the next, and the height of a node.
const ROW = { timeline: [32, 11], userline: [24, 3] };

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1440, height: 900 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(SCRIPT);
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    // The appearance of the user's own is held for five seconds in the program. The check will
    // not wait that long, so it is compressed here and put back after.
    await page.evaluate(() => { window.RAIL_LOOK = 350; });
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    for (const which of LINES) {
      await page.evaluate((w, m) => window.__draw(w, m), which, 3);
      await sleep(220);
      const short = await page.evaluate((w) => window.__read(w), which);
      if (short.many !== 3) { note(which + ': three nodes drew ' + short.many); }
      if (short.more) { note(which + ': a line with room was still made a window'); }
      if (short.up || short.down) { note(which + ': a mark stands when there is nothing to slide'); }
      if (!(short.gap > 20)) { note(which + ': the distance between nodes changed: ' + short.gap); }
      if (!(short.height === SHORT_HEIGHT[which])) {
        note(which + ': the line is ' + short.height + ' tall, not ' + SHORT_HEIGHT[which]);
      }
      if (!short.centred) { note(which + ': the line is not centred in the room'); }
      if (short.hidden !== 0) { note(which + ': rows were hidden though the line fits'); }

      await page.evaluate((w, m) => window.__draw(w, m), which, 60);
      await sleep(220);
      const long = await page.evaluate((w) => window.__read(w), which);
      const room = await page.evaluate(() => document.getElementById('stage').clientHeight);
      // The window holds a whole number of rows of nodes, so no node is cut in half along the
      // edge of it.
      const stride = ROW[which][0], nodeHigh = ROW[which][1];
      const windowHeight = (() => {
        const rows = Math.floor((room - EDGE * 2 + stride) / stride);
        return (rows - 1) * stride + nodeHigh;
      })();
      if (long.many !== 60) { note(which + ': sixty nodes drew ' + long.many); }
      if (!long.more) { note(which + ': a line with no room was not made a window'); }
      if (Math.abs(long.height - windowHeight) > 2) {
        note(which + ': the window is ' + long.height + ' in a room of ' + room
          + ', which should leave it ' + windowHeight);
      }
      if (Math.abs(long.gap - short.gap) > 0.5) {
        note(which + ': the distance between nodes shrank from ' + short.gap + ' to ' + long.gap);
      }
      if (!long.centred) { note(which + ': the window is not centred in the room'); }
      if (!long.up) { note(which + ': no mark for the older part of the line'); }
      if (long.down) { note(which + ': the mark for the newer part stands at the end of the line'); }
      if (long.hidden <= 0) { note(which + ': nothing was hidden though the line overflows'); }
      if (!long.inWindow) { note(which + ': a shown row hangs out of the window'); }
      if (long.off < long.max) { note(which + ': the line does not stand at the newest node'); }
      if (long.last > long.bottom + 1) { note(which + ': the newest node is below the window'); }

      // The wheel of the user slides the line, and the markers answer to it.
      await page.evaluate((w) => window.__wheel(w, -240), which);
      await sleep(220);
      const looked = await page.evaluate((w) => window.__read(w), which);
      if (!(looked.off > 0 && looked.off < looked.max)) {
        note(which + ': a wheel over the line left it at ' + looked.off + ' of ' + looked.max);
      }
      if (!looked.down) { note(which + ': no mark for the newer part after a scroll back'); }
      if (!looked.inWindow) { note(which + ': a shown row hangs out after a scroll'); }

      // While the look lasts, a node arriving leaves the window where it is; when it has
      // expired, the line returns to the newest node of itself.
      const held = await page.evaluate((w) => {
        window.__draw(w, 61);
        return window.__read(w);
      }, which);
      if (Math.abs(held.off - looked.off) > 1) {
        note(which + ': a new node dragged the user back to the end of the line');
      }
      await sleep(500);
      const back = await page.evaluate((w) => window.__read(w), which);
      if (back.off < back.max) { note(which + ': the line did not come back to the newest node'); }
      if (back.down) { note(which + ': the mark for the newer part stands at the end again'); }

      // A press of a marker moves the window the other way.
      const turned = await page.evaluate((w) => {
        window.__wheel(w, 1000000);
        var before = window.__read(w);
        window.__mark(w, 'up');
        var after = window.__read(w);
        window.__mark(w, 'down');
        window.__mark(w, 'down');
        window.__mark(w, 'down');
        var end = window.__read(w);
        return { before: before, after: after, end: end };
      }, which);
      if (!(turned.after.off < turned.before.off)) {
        note(which + ': the mark for the older part did not move the window');
      }
      if (!(turned.end.off >= turned.end.max - 1)) {
        note(which + ': the mark for the newer part did not come back to the end');
      }
      if (turned.end.down) { note(which + ': the mark for the newer part stands at the end of the line'); }

      console.log('  ' + one.name + ' ' + which + ': ' + JSON.stringify({
        shortGap: short.gap, longGap: long.gap, height: long.height, room: room,
        hidden: long.hidden, off: long.off, max: long.max,
      }));
    }

    if (faults.length) { note('the face faulted: ' + faults.slice(0, 2).join(' | ')); }
    await page.close();
    trouble += notes.length;
  }
  await browser.close();
  console.log(trouble ? '\n' + trouble + ' errors' : '\nnothing amiss in either face');
  return trouble;
}

main().then((n) => process.exit(n ? 1 : 0), (e) => { console.error(e); process.exit(2); });
