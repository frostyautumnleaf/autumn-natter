// Check that the writing is followed while it is written, in a block of code and
// in the block of a call. The page is loaded as it stands, with no bridge and no
// engine: the writer of the live answer is driven by hand, and the wheel of the
// mouse is a wheel of the browser.
//
// Four things are looked at, in both faces:
//
//   * a block of code that is being written is kept at its end, where the writing
//     stands, once it is too long for the room it has;
//   * a wheel turned over such a block, which the block could not take because it
//     stood at its end already, does not stop the following of it. That used to
//     mark the block as gone into, and the writing was followed no more for the
//     rest of the answer;
//   * a block the user puts a place of their own in is held where the user put it,
//     and bringing it back down to its end takes the following up again;
//   * the block of a call being written is followed in the same way, and a scroll
//     the program asked for is never read as the hand of the user.
//
// The writing only ever grows, as it does in an answer: a block that is made
// again is a block that has come in, and the place of the user is not in it.
//
//     node scroll-check.js                            both faces
//     AN_PAGE=../web/page.html node scroll-check.js      one face
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

// A code fence long enough to go past the room a block of code has, and a command
// long enough to go past the room the block of a call has.
const CODE = '```rust\n' + Array.from({ length: 150 },
  (_, i) => 'fn line' + i + '() -> u32 { ' + i + ' }').join('\n') + '\n```';
const COMMAND = '/bin/sh -c "' + Array.from({ length: 260 },
  (_, i) => 'echo row' + i + ';').join(' ') + '"';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The hand of the user in a block that scrolls: the block is put so many points
// above its end, and says so, which is what a drag of its scroll bar is.
function goAbove(back) {
  const one = window.__block;
  const most = one.scrollHeight - one.clientHeight;
  one.scrollTop = Math.max(0, most - Math.abs(back));
  one.dispatchEvent(new Event('scroll'));
  return Math.round(one.scrollTop);
}

// The user comes back down to the end of the block.
function goToEnd() {
  const one = window.__block;
  one.scrollTop = one.scrollHeight;
  one.dispatchEvent(new Event('scroll'));
  return Math.round(one.scrollTop);
}

// How far the end of the block still is, and whether the block is held.
function readBlock(held) {
  const one = window.__block;
  return {
    room: Math.round(one.scrollHeight - (one.scrollTop + one.clientHeight)),
    at: Math.round(one.scrollTop),
    held: held === 'looked' ? one.dataset.looked === '1'
      : !!(window.__writing() && window.__writing().pinned)
  };
}

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  let faults = 0;
  for (const spec of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 760 });
    page.on('pageerror', (e) => { faults++; console.log(spec.name + ' PAGE ERROR: ' + e.message); });
    await page.goto('file://' + spec.file);
    const say = (m) => { faults++; console.log(spec.name + ': ' + m); };

    // The two writings, and the block each of them is writing into.
    const start = await page.evaluate(({ CODE, COMMAND }) => {
      const host = document.createElement('div');
      host.className = 'turn t-ai';
      // The turn goes inside the chat, for the chat covers the page: a wheel
      // turned over a block has to come to the block and not to the chat.
      document.getElementById('thread').appendChild(host);
      window.__host = host;
      window.__code = CODE;
      window.__command = COMMAND;
      window.__writing = () => (ink.tools.length ? ink.tools[0].writing : null);
      window.__putCode = (upTo) => {
        const m = { body: CODE.slice(0, upTo), reasoning: '', reasoning_ms: 0,
          reasoning_done: false, tools: [] };
        if (!window.__lived) { fillLive(host, m); window.__lived = true; }
        else { feedInk(m); }
        ink.shown = ink.said.length;
        writeSaid();
        // A block that came in is made again, so the block is looked for again
        // after every look of the writing.
        window.__block = host.querySelector('pre.code');
      };
      window.__putCall = (upTo) => {
        const list = [{ name: 'run_terminal', command: COMMAND.slice(0, upTo), purpose: '' }];
        const m = { body: '', reasoning: '', reasoning_ms: 0, reasoning_done: false, tools: list };
        if (!window.__livedCall) { fillLive(host, m); window.__livedCall = true; }
        else { feedInk(m); }
        feedWrite(list);
        window.__block = host.querySelector('.tool.write .cmd-text');
      };
      window.__putCode(CODE.length * 0.15 | 0);
      window.__putCode(CODE.length * 0.5 | 0);
      const one = window.__block;
      return {
        made: !!one,
        scrollable: one ? one.scrollHeight > one.clientHeight : false,
        room: one ? Math.round(one.scrollHeight - (one.scrollTop + one.clientHeight)) : -1,
        looked: one ? one.dataset.looked === '1' : null
      };
    }, { CODE, COMMAND });
    if (!start.made) { say('no block of code was made'); await page.close(); continue; }
    if (!start.scrollable) { say('the block of code never went past the room it has'); await page.close(); continue; }
    if (start.room > 4) { say('the block of code being written stands ' + start.room + ' from its end'); }
    if (start.looked) { say('the block of code was held with nothing of the user in it'); }

    // A wheel turned over the block while it stands at its end: the block can take
    // nothing of it, so the chat moves and nothing of the block is in it.
    const box = await page.evaluate(() => {
      window.__block.scrollIntoView({ block: 'end' });
      const r = window.__block.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    });
    await sleep(40);
    await page.mouse.move(box.x, box.y);
    await page.mouse.wheel({ deltaY: 240 });
    await sleep(60);
    let step = await page.evaluate(() => ({
      looked: window.__block.dataset.looked === '1'
    }));
    if (step.looked) {
      say('a wheel the block could not take held the block, which stops the following');
    }
    step = await page.evaluate(() => {
      window.__putCode(window.__code.length * 0.7 | 0);
      return Math.round(window.__block.scrollHeight -
        (window.__block.scrollTop + window.__block.clientHeight));
    });
    if (step > 4) { say('the block of code stopped following the writing after a wheel'); }

    // A place of the user's in the block: the block is gone into to be read, and
    // the following halts. The place is put in by hand, for what the block is
    // asked for is the same scroll event a drag of its scroll bar gives.
    const above = await page.evaluate(goAbove, 240);
    await sleep(40);
    step = await page.evaluate(() => {
      const at = Math.round(window.__block.scrollTop);
      window.__putCode(window.__code.length * 0.85 | 0);
      return { at: at, after: Math.round(window.__block.scrollTop),
        looked: window.__block.dataset.looked === '1' };
    });
    if (above <= 0) { say('the block was not moved, so nothing is proved here'); }
    if (!step.looked) { say('a place of the user\'s in the block left the block unheld'); }
    if (step.after > step.at + 4) { say('the block was pulled down while the user read it'); }

    // The user comes back down to the end of the block: it is followed again.
    await page.evaluate(goToEnd);
    await sleep(40);
    step = await page.evaluate(() => {
      window.__putCode(window.__code.length);
      return {
        looked: window.__block.dataset.looked === '1',
        room: Math.round(window.__block.scrollHeight -
          (window.__block.scrollTop + window.__block.clientHeight))
      };
    });
    if (step.looked) { say('the block stayed unheld after the user came back to its end'); }
    if (step.room > 4) { say('the block of code did not go back to following its end'); }

    // ------------------------------------------------- the block of a call
    step = await page.evaluate(() => {
      // A turn of its own for the call, so the block of the code that was just
      // written is left where it is.
      const host = document.createElement('div');
      host.className = 'turn t-ai';
      document.getElementById('thread').appendChild(host);
      window.__callHost = host;
      window.__writing = () => (ink.tools.length ? ink.tools[0].writing : null);
      window.__putCall = (upTo) => {
        const list = [{ name: 'run_terminal', command: window.__command.slice(0, upTo), purpose: '' }];
        const m = { body: '', reasoning: '', reasoning_ms: 0, reasoning_done: false, tools: list };
        if (!window.__livedCall) { fillLive(host, m); window.__livedCall = true; }
        else { feedInk(m); }
        feedWrite(list);
        window.__block = host.querySelector('.tool.write .cmd-text');
      };
      window.__putCall(window.__command.length * 0.2 | 0);
      window.__putCall(window.__command.length * 0.6 | 0);
      const one = window.__block;
      const writing = window.__writing();
      return {
        made: !!one,
        scrollable: one ? one.scrollHeight > one.clientHeight : false,
        room: one ? Math.round(one.scrollHeight - (one.scrollTop + one.clientHeight)) : -1,
        held: writing ? !!writing.pinned : null
      };
    }, null);
    if (!step.made) { say('no block of a call was made'); await page.close(); continue; }
    if (!step.scrollable) { say('the block of the call never went past the room it has'); }
    if (step.room > 4) { say('the block of the call stands ' + step.room + ' from its end'); }
    if (step.held) { say('the block of the call was held with no scroll of the user in it'); }

    // The place the program asked for is never read as the hand of the user.
    step = await page.evaluate(() => {
      window.__putCall(window.__command.length * 0.75 | 0);
      const writing = window.__writing();
      return {
        held: writing ? !!writing.pinned : null,
        room: Math.round(window.__block.scrollHeight -
          (window.__block.scrollTop + window.__block.clientHeight))
      };
    });
    if (step.held) { say('a scroll the program asked for held the block of the call'); }
    if (step.room > 4) { say('the block of the call stopped following its writing'); }

    // A place of the user's in the block of the call: the following halts, and the
    // block stands where the user left it.
    const callAbove = await page.evaluate(goAbove, 120);
    await sleep(40);
    step = await page.evaluate(() => {
      const at = Math.round(window.__block.scrollTop);
      window.__putCall(window.__command.length * 0.9 | 0);
      const writing = window.__writing();
      return { at: at, after: Math.round(window.__block.scrollTop),
        held: writing ? !!writing.pinned : null };
    });
    if (callAbove <= 0) { say('the block of the call was not moved'); }
    if (!step.held) { say('a place of the user\'s in the block of the call left it unheld'); }
    if (step.after > step.at + 4) { say('the block of the call was pulled down over the user'); }

    // The user comes back down to the end of the block: it is followed again.
    await page.evaluate(goToEnd);
    await sleep(40);
    step = await page.evaluate(() => {
      window.__putCall(window.__command.length);
      const writing = window.__writing();
      return {
        held: writing ? !!writing.pinned : null,
        room: Math.round(window.__block.scrollHeight -
          (window.__block.scrollTop + window.__block.clientHeight))
      };
    });
    if (step.held) { say('the block of the call stayed held after the user came to its end'); }
    if (step.room > 4) { say('the block of the call did not go back to following its end'); }
    await page.close();
  }
  await browser.close();
  console.log(faults ? faults + ' faults' : 'the writing is followed in both faces');
  process.exit(faults ? 1 : 0);
})();
