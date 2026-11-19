// The markers of the page under a pointer, checked in both pages with no engine
// behind them: the markers start as the pointer comes to them, and settle as it goes,
// and nothing about the page moves or changes size while that happens.
//
//   node hover-face-check.js
//   AN_PAGE=../web/page.html node hover-face-check.js   # one page only
//
// Six items are checked:
//   * a marker starts - the class that animates it is on it, and the marker is drawn
//     bigger than it was - when the control it is in is hovered, and when the
//     marker itself is hovered;
//   * the program icon in the top bar, which is an image and not a marker of the
//     type the buttons carry, starts as well;
//   * going away from a marker is answered: the settling class is on it, and once
//     that is over no class is left and the marker remains as it was drawn;
//   * a control with markers of its own starts the first of them, so the text of the
//     control and the marker of the control answer as one;
//   * a stretch of plain text starts nothing;
//   * with the motion of the system turned down there is no animation at all.
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

// What a marker of the page looks like now: started or settling or neither, and how big it is
// drawn. `box` is the box of the control it is in, so that a motion of the marker is not
// mistaken for a motion of the page.
const LOOK = `
window.__look = function (sel) {
  var one = document.querySelector(sel);
  if (!one) { return { gone: true }; }
  // A place named by its control is looked at through the mark of that control.
  if (!one.classList.contains('svg') && !one.matches('img.mark')) {
    one = one.querySelector('.svg, img.mark') || one;
  }
  var style = getComputedStyle(one);
  var host = one.closest('button, label, .bar, .hello') || one.parentElement;
  var box = host ? host.getBoundingClientRect() : null;
  var scale = 1;
  if (style.transform !== 'none') {
    // The transform of a mark that is being moved is a matrix; the first two
    // numbers say how wide it is drawn, which is what is looked at here.
    var nums = style.transform.slice(style.transform.indexOf('(') + 1).split(',');
    var a = parseFloat(nums[0]) || 0;
    var b = parseFloat(nums[1]) || 0;
    scale = Math.round(Math.sqrt(a * a + b * b) * 100) / 100;
  }
  return {
    woke: one.classList.contains('mark-in'),
    set: one.classList.contains('mark-out'),
    scale: scale,
    animation: style.animationName,
    box: box ? [Math.round(box.left), Math.round(box.top), Math.round(box.width),
        Math.round(box.height)] : null,
  };
};
`;

// The markers the check puts a pointer on. Each means a type of the program: a badge of the top
// bar, a marker of a button of the top bar, a field of the top bar whose marker hangs at its
// left, the program icon itself, and text of the page that hold no marker at all. A chat with
// the blocks of a working conversation in it: a saying of the user, an answer with a train of
// thought, and a run of a tool. The headers of the thought and of the run are pressed open, so
// their markers answer to a hand over their text and not only to a hand over the marker itself.
function drawChat() {
  cur = { chatting: { tools: true, thinking: true, grow: false },
    chat: { id: 'chat-marks', title: 'Marks', home: false },
    messages: [
      { id: 'u1', role: 'You', body: 'Run the tests, and think about them first.',
        when: '11:04', when_full: 'today at 11:04', tool: '', mono: false, failed: false,
        reasoning: '', reasoning_ms: 0, attachments: [] },
      { id: 'm1', role: 'Assistant', body: 'The tests pass, all of them.', when: '11:05',
        when_full: 'today at 11:05', tool: '', mono: false, failed: false,
        reasoning: 'Because the conversation requested the tests, they are run and read.',
        reasoning_ms: 1200, attachments: [] },
      { id: 't1', role: 'Tool', body: 'cargo test\n\ntest result: ok', tool: 'ran in 3 s',
        tool_name: 'run_command', mono: true, when: '11:06', when_full: 'today at 11:06',
        reasoning: '', reasoning_ms: 0, failed: false, attachments: [] } ],
    chapters: [], handoffs: [], compaction: {}, generating: false, live: '', reasoning: '',
    live_tools: [] };
  readChatting(cur);
  drawThread(cur);
  return { heads: document.querySelectorAll('.thead').length,
    marks: document.querySelectorAll('.turn .svg').length };
}

// The text of an item the pointer may press, away from the marker that is in it.
function wordPlace(sel) {
  var one = document.querySelector(sel);
  if (!one) { return { gone: true }; }
  var box = one.getBoundingClientRect();
  return { x: Math.round(box.right - 8), y: Math.round(box.top + box.height / 2),
    mark: (one.querySelector('.svg') || {}).className !== undefined ? 'yes' : 'no' };
}

const PLACES = [
  { sel: '#code', what: 'the badge of the tools' },
  { sel: '#themebtn', what: 'the marker of the theme' },
  { sel: 'label.field > .svg', what: 'the marker of the field of the model' },
  { sel: 'img.mark', what: 'the mark of the program' },
];

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 900 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(LOOK);
    await page.evaluateOnNewDocument('window.__drawChat = ' + drawChat.toString()
      + '\nwindow.__wordPlace = ' + wordPlace.toString());
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(220);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    for (const place of PLACES) {
      const rest = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (rest.gone) { note(place.what + ' is not on the page'); continue; }
      const where = await page.evaluate((sel) => {
        const box = document.querySelector(sel).getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
      }, place.sel);
      await page.mouse.move(where.x, where.y);
      await sleep(60);
      const woke = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (!woke.woke) { note(place.what + ' did not start under the pointer'); }
      if (!(woke.scale > 1.02)) { note(place.what + ' woke without moving: ' + woke.scale); }
      if (rest.box && woke.box && String(rest.box) !== String(woke.box)) {
        note(place.what + ' moved the item it is in: ' + rest.box + ' -> ' + woke.box);
      }
      // The pointer goes: the marker settles, and is left as it was drawn.
      await page.mouse.move(6, 6);
      await sleep(40);
      const settling = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (!settling.set) { note(place.what + ' was left without settling back'); }
      await sleep(420);
      const back = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (back.woke || back.set) { note(place.what + ' kept a class after the going'); }
      if (Math.abs(back.scale - 1) > 0.02) { note(place.what + ' never came back to size: ' + back.scale); }
      console.log('  ' + one.name + ' ' + place.what + ': ' + JSON.stringify({
        woke: woke.woke, scale: woke.scale, set: settling.set, back: back.scale,
      }));
    }

    // Text of the page that hold no marker start nothing at all.
    const idle = await page.evaluate(() => {
      var words = null;
      var all = document.querySelectorAll('.bar span, .hello .greet, .hello .where');
      for (var n = 0; n < all.length; n++) {
        if (!all[n].querySelector('.svg, img.mark') && (all[n].textContent || '').trim().length > 3) {
          words = all[n]; break;
        }
      }
      return words ? { found: true, x: words.getBoundingClientRect().left + 4,
        y: words.getBoundingClientRect().top + 2 } : { found: false };
    });
    if (!idle.found) {
      note('no stretch of plain text was found to put a pointer on');
    } else {
      await page.mouse.move(idle.x, idle.y);
      await sleep(80);
      const woken = await page.evaluate(() => document.querySelectorAll('.mark-in').length);
      if (woken) { note('plain words woke ' + woken + ' markers'); }
      await page.mouse.move(6, 6);
      await sleep(400);
    }

    // The blocks of a working chat: the header of a run of a tool and the header of a train of
    // thought answer to a hand over their text, for a hand over the text of a header is a hand
    // over the header.
    const chat = await page.evaluate(() => {
      document.querySelector('#draft').value = '';
      return window.__drawChat ? window.__drawChat() : { skipped: true };
    });
    if (!chat.skipped) {
      console.log('  ' + one.name + ' chat of blocks: ' + JSON.stringify(chat));
      if (!chat.heads) {
        note('the chat of blocks drew no header to put a pointer on');
      } else {
        const head = await page.evaluate(() => window.__wordPlace('.turn .thead'));
        if (head.gone) {
          note('the head of the run is not on the page');
        } else {
          await page.mouse.move(head.x, head.y);
          await sleep(70);
          const woke = await page.evaluate(() => {
            var one = document.querySelector('.turn .thead .svg');
            return { woke: one.classList.contains('mark-in'), marks: document.querySelectorAll('.mark-in').length };
          });
          if (!woke.woke) { note('the header of a run did not start its marker: ' + JSON.stringify(woke)); }
          await page.mouse.move(6, 6);
          await sleep(400);
        }
        // Text that hold no pressable item start nothing of their block.
        const said = await page.evaluate(() => window.__wordPlace('.turn .said, .turn .words, .bubble'));
        console.log('  ' + one.name + ' words of a block: ' + JSON.stringify(said));
        if (!said.gone) {
          await page.mouse.move(said.x, said.y);
          await sleep(80);
          const woken = await page.evaluate(() => document.querySelectorAll('.mark-in').length);
          if (woken) { note('text of a block woke ' + woken + ' markers'); }
          await page.mouse.move(6, 6);
          await sleep(400);
        }
      }
    }

    // The motion of the system turned down: the markers are left remaining.
    await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
    await sleep(60);
    const quiet = await page.evaluate(() => {
      var one = document.querySelector('#code .svg');
      one.classList.add('mark-in');
      var name = getComputedStyle(one).animationName;
      one.classList.remove('mark-in');
      one.classList.add('mark-out');
      var out = getComputedStyle(one).animationName;
      one.classList.remove('mark-out');
      return { name: name, out: out };
    });
    if (quiet.name !== 'none' || quiet.out !== 'none') {
      note('the markers still animate with the motion turned down: ' + JSON.stringify(quiet));
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
