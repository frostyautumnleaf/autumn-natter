// The line of the user's messages, looked at in both faces with no engine
// behind them: the drawing is driven by hand, so the window face and the browser
// face are read the same way. The live check (userline-check.js) does the same
// things against a running program; this one is here because the two faces must
// agree, and only one of them is served over HTTP.
//
//   node userline-face-check.js
//
// Five things are looked at, in each face:
//   * the line is drawn, one dash for one message of the user, and the sayings
//     of the model, the notes of the program and the runs of the tools stand on
//     no dash;
//   * a dash is a dash: a stroke across the line, not a bead on it;
//   * the card of a dash says which message it is, when it was sent, shows a
//     little of what it says, and counts what hung on it -- pictures told from
//     the other files;
//   * the setting switched off takes the line away;
//   * a chat with more messages than the window is tall for draws the dashes
//     closer together, and the last of them still stands inside the stage.
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

// A talk with the marks of a working chat in it: sayings of the user, of the
// model, runs of tools, a note, and one message with things hung on it.
function talk(count) {
  const out = [];
  for (let n = 1; n <= count; n++) {
    out.push({
      id: 'u' + n, role: 'You', body: 'Question ' + n + ' with a `mark` and a long tail of words',
      when: '12:0' + (n % 10), attachments: n === 2
        ? [{ id: 'a1', name: 'cat.png', mime: 'image/png', image: 'x' },
           { id: 'a2', name: 'notes.md', mime: 'text/markdown', image: false }]
        : [],
    });
    out.push({ id: 'm' + n, role: 'Assistant', body: 'Answer ' + n, when: '12:0' + (n % 10), attachments: [] });
    if (n === 1) {
      out.push({ id: 't' + n, role: 'Tool', body: 'a command ran', when: '12:03', attachments: [] });
      out.push({ id: 'n' + n, role: 'Note', body: 'a note of the program', when: '12:03', attachments: [] });
    }
  }
  return out;
}

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
    await page.evaluateOnNewDocument('window.__talk = ' + talk.toString());
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  FAULT: ' + what); };

    const made = await page.evaluate(() => {
      chatting.userline = true;
      cur = { handoffs: [], messages: [] };
      const s = { messages: [], chatting: { userline: true } };
      s.messages = window.__talk(4);
      drawUserLine(s);
      const box = document.getElementById('userline');
      const dashes = Array.from(box.querySelectorAll('.unode'));
      const style = dashes.length ? getComputedStyle(dashes[0]) : null;
      const cards = dashes.map((d) => (d.querySelector('.card') || {}).textContent || '');
      return {
        shown: !box.classList.contains('hidden') && getComputedStyle(box).display !== 'none',
        count: dashes.length,
        dash: style ? { w: parseFloat(style.width), h: parseFloat(style.height) } : null,
        cards: cards.map((c) => c.replace(/\s+/g, ' ').trim()),
      };
    });

    if (!made.shown) { note('the line is not drawn'); }
    if (made.count !== 4) { note('four messages of the user drew ' + made.count + ' dashes'); }
    if (!made.dash || made.dash.h > 5 || made.dash.w < 8) {
      note('the node is not a dash: ' + JSON.stringify(made.dash));
    }
    if (!(made.cards[0] || '').startsWith('Your message 1')) {
      note('the first card does not say which message: ' + (made.cards[0] || '').slice(0, 60));
    }
    if (!/Sent 12:0/.test(made.cards[0] || '')) { note('the first card says nothing of when'); }
    if ((made.cards[0] || '').indexOf('Question 1') < 0) {
      note('the first card shows no preview');
    }
    if (!/1 picture and 1 file/.test(made.cards[1] || '')) {
      note('the second card does not count the attachments: ' + (made.cards[1] || '').slice(0, 90));
    }

    // The setting off, and a chat long enough to need the dashes drawn closer.
    const rest = await page.evaluate(() => {
      const box = document.getElementById('userline');
      const stage = document.getElementById('stage');
      // The setting is the one the page reads, which is the mark the sheet
      // moves -- not a mark carried in the state of one draw.
      chatting.userline = false;
      drawUserLine({ messages: window.__talk(4) });
      const off = box.classList.contains('hidden') || getComputedStyle(box).display === 'none';
      chatting.userline = true;
      drawUserLine({ messages: window.__talk(60) });
      const many = box.querySelectorAll('.unode').length;
      const gap = parseFloat(getComputedStyle(box).getPropertyValue('--ugap'));
      const here = box.getBoundingClientRect();
      const room = stage.getBoundingClientRect();
      // Clicking a dash of a chat whose words are not on the page does nothing,
      // and must not fault.
      box.querySelector('.unode').click();
      return {
        off: off, many: many, gap: gap,
        fits: here.top >= room.top - 2 && here.bottom <= room.bottom + 2,
        height: Math.round(here.height),
      };
    });
    if (!rest.off) { note('the line stands with the setting off'); }
    if (rest.many !== 60) { note('60 messages drew ' + rest.many + ' dashes'); }
    if (!(rest.gap < 21)) { note('the dashes were not drawn closer: gap ' + rest.gap); }
    if (!rest.fits) { note('the line hangs out of the stage when the chat is long: ' + rest.height); }

    if (faults.length) { note('the face faulted: ' + faults.slice(0, 2).join(' | ')); }
    console.log(one.name + ':', JSON.stringify({
      count: made.count, dash: made.dash, gap: rest.gap, many: rest.many,
    }));
    await page.close();
    trouble += notes.length;
  }
  await browser.close();
  console.log(trouble ? '\n' + trouble + ' faults' : '\nnothing amiss in either face');
  return trouble;
}

main().then((n) => process.exit(n ? 1 : 0), (e) => { console.error(e); process.exit(2); });
