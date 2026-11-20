// The hover animation of the icons, checked in both pages with no engine behind them: an icon
// animates as the pointer arrives and settles as it leaves, and nothing else on the page moves
// or changes size while that happens.
//
//   node hover-face-check.js
//   AN_PAGE=../web/page.html node hover-face-check.js   # one page only
//
// Six items are checked:
//   * an icon animates - the class that animates it is on it, and it is drawn larger than at
//     rest - when the control it stands in is hovered and when the icon itself is hovered;
//   * the program icon in the top bar, which is an image rather than one of the icons the
//     buttons carry, animates as well;
//   * leaving an icon is answered: the settle class is on it, and once that animation ends no
//     class is left and the icon is as it was drawn;
//   * hovering the text of a control animates the icon of that control, so text and icon answer
//     as one;
//   * plain text starts no animation;
//   * with reduced motion requested there is no animation at all.
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

// How an icon stands now: animating, settling or at rest, and how large it is drawn. `box` is
// the box of the control it stands in, so that the movement of an icon is not mistaken for a
// movement of the page.
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
    // The transform of an animating icon is a matrix. Its first two numbers give the width it
    // is drawn at, which is what is compared here.
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

// The places the check puts the pointer on. Each stands for a type of control in the program: a
// badge of the top bar, a button of the top bar, a field of the top bar with its icon at the
// left, and the program icon itself. The chat drawn for the check holds the blocks of a working
// conversation: a message of the user, an answer with its thinking, and a tool call. The headers
// of the thinking panel and of the tool block are open, so their icons answer to a hover over
// their text and not only to a hover over the icon itself.
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

// A point over the text of a clickable item, away from the icon inside it.
function wordPlace(sel) {
  var one = document.querySelector(sel);
  if (!one) { return { gone: true }; }
  var box = one.getBoundingClientRect();
  return { x: Math.round(box.right - 8), y: Math.round(box.top + box.height / 2),
    mark: (one.querySelector('.svg') || {}).className !== undefined ? 'yes' : 'no' };
}

const PLACES = [
  { sel: '#code', what: 'the tools badge' },
  { sel: '#themebtn', what: 'the theme icon' },
  { sel: 'label.field > .svg', what: 'the model field icon' },
  { sel: 'img.mark', what: 'the program icon' },
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
      if (!woke.woke) { note(place.what + ' did not animate on hover'); }
      if (!(woke.scale > 1.02)) { note(place.what + ' has the class but does not move: ' + woke.scale); }
      if (rest.box && woke.box && String(rest.box) !== String(woke.box)) {
        note(place.what + ' moved the control it stands in: ' + rest.box + ' -> ' + woke.box);
      }
      // The pointer leaves: the icon settles and is left as it was drawn.
      await page.mouse.move(6, 6);
      await sleep(40);
      const settling = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (!settling.set) { note(place.what + ' was left without a settle animation'); }
      await sleep(420);
      const back = await page.evaluate((sel) => window.__look(sel), place.sel);
      if (back.woke || back.set) { note(place.what + ' kept an animation class after the animation ended'); }
      if (Math.abs(back.scale - 1) > 0.02) { note(place.what + ' did not return to its resting size: ' + back.scale); }
      console.log('  ' + one.name + ' ' + place.what + ': ' + JSON.stringify({
        woke: woke.woke, scale: woke.scale, set: settling.set, back: back.scale,
      }));
    }

    // Text with no icon in it starts no animation.
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
      note('no place of plain text was found to put the pointer on');
    } else {
      await page.mouse.move(idle.x, idle.y);
      await sleep(80);
      const woken = await page.evaluate(() => document.querySelectorAll('.mark-in').length);
      if (woken) { note('plain text started ' + woken + ' icon animations'); }
      await page.mouse.move(6, 6);
      await sleep(400);
    }

    // The blocks of a working chat: the header of a tool call and the header of the thinking
    // panel answer to a hover over their text, because hovering the text of a header is
    // hovering the header.
    const chat = await page.evaluate(() => {
      document.querySelector('#draft').value = '';
      return window.__drawChat ? window.__drawChat() : { skipped: true };
    });
    if (!chat.skipped) {
      console.log('  ' + one.name + ' blocks of the chat: ' + JSON.stringify(chat));
      if (!chat.heads) {
        note('the chat drew no header to put the pointer on');
      } else {
        const head = await page.evaluate(() => window.__wordPlace('.turn .thead'));
        if (head.gone) {
          note('the header of the tool block is not on the page');
        } else {
          await page.mouse.move(head.x, head.y);
          await sleep(70);
          const woke = await page.evaluate(() => {
            var one = document.querySelector('.turn .thead .svg');
            return { woke: one.classList.contains('mark-in'), marks: document.querySelectorAll('.mark-in').length };
          });
          if (!woke.woke) { note('the header of the tool block did not animate its icon: ' + JSON.stringify(woke)); }
          await page.mouse.move(6, 6);
          await sleep(400);
        }
        // Text with nothing clickable in it starts no animation in its block.
        const said = await page.evaluate(() => window.__wordPlace('.turn .said, .turn .words, .bubble'));
        console.log('  ' + one.name + ' text of a block: ' + JSON.stringify(said));
        if (!said.gone) {
          await page.mouse.move(said.x, said.y);
          await sleep(80);
          const woken = await page.evaluate(() => document.querySelectorAll('.mark-in').length);
          if (woken) { note('text of a block started ' + woken + ' icon animations'); }
          await page.mouse.move(6, 6);
          await sleep(400);
        }
      }
    }

    // With reduced motion requested, neither animation runs.
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
      note('the icons still animate with reduced motion requested: ' + JSON.stringify(quiet));
    }

    if (faults.length) { note('page faults: ' + faults.slice(0, 2).join(' | ')); }
    await page.close();
    trouble += notes.length;
  }
  await browser.close();
  console.log(trouble ? '\n' + trouble + ' errors' : '\nthe hover animation is correct in both pages');
  return trouble;
}

main().then((n) => process.exit(n ? 1 : 0), (e) => { console.error(e); process.exit(2); });
