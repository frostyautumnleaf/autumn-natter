// Going to a place of the chat from a node of one of the three lines beside it,
// checked in both pages with no engine behind the page. The chat is long - the
// type a context of an hour makes - and all of it is in the space of the
// conversation, so the way from the end of it back to an early chapter is many screens.
//
//   node goto-face-check.js
//   AN_PAGE=../web/page.html node goto-face-check.js   # one page only
//
// Six items are checked:
//   * one click of a node of the chapter line remains that chapter at the top of
//     the view, however far it is - and the same for a node of the summarisation
//     line and for a dash of the line of the user's own messages;
//   * a second click, of a node further up or further down the conversation, goes to
//     that place in one go as well, and not half the way to it;
//   * text above the place that settle after the click - a row that grows, which
//     is what an image that arrived or a look put on again does - do not leave
//     the chat remaining somewhere else;
//   * a draw that arrives while the chat is being was does not drag it back to
//     the end of the conversation;
//   * the chat stops following the answer when the user goes looking;
//   * the first node of the conversation is at the first turn of it, where the
//     questions of the plan are not.
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

// A context of many turns, each with enough in it to be a screenful of its own, so the size of
// the context grows to many times the height of the window. That is the case the going has to
// answer for.
function makeTalk(many) {
  var messages = [], chapters = [], handoffs = [];
  var paragraph = 'Paragraph of this answer, which runs to some length so that the turn it '
    + 'is in is a good deal taller than a line of text. It holds a list, and a sentence '
    + 'or two after the list, and it reports nothing of consequence.';
  for (var n = 1; n <= many; n++) {
    messages.push({ id: 'u' + n, role: 'You', body: 'Question ' + n + ' about the thing',
      when: '12:0' + (n % 10), when_full: 'today at 12:0' + (n % 10), tool: '', mono: false,
      failed: false, reasoning: '', reasoning_ms: 0, attachments: [] });
    messages.push({ id: 'm' + n, role: 'Assistant',
      body: '# Answer ' + n + '\n\n' + paragraph + '\n\n' + paragraph + '\n\n' + paragraph
        + '\n\n- the first of the points\n- the second of the points\n\n'
        + 'The end of answer ' + n + '.',
      when: '12:0' + (n % 10), when_full: 'today at 12:0' + (n % 10), tool: '', mono: false,
      failed: false, reasoning: '', reasoning_ms: 0, attachments: [] });
    chapters.push({ id: 'ch' + n, name: 'Chapter ' + n, after: 'u' + n });
    handoffs.push({ id: 'm' + n, number: n, state: 'done', when: 'today', before: 9000 + n,
      after: 900, snippet: 'The conversation about ' + n });
  }
  return { messages: messages, chapters: chapters, handoffs: handoffs };
}

// The chat of a context of so many turns, was at the end of itself as it is now when the user
// has been watching an answer arrive, with the three lines beside it drawn as well.
function setup(many) {
  var talk = makeTalk(many);
  cur = { chatting: { chapters: true, userline: true, send_anim: false },
    chat: { id: 'chat-long', title: 'Long conversation', home: false },
    messages: talk.messages, chapters: talk.chapters, handoffs: talk.handoffs,
    compaction: {}, generating: false, live: '', reasoning: '', live_tools: [] };
  readChatting(cur);
  drawThread(cur);
  drawChapters(cur);
  drawTimeline(cur);
  drawUserLine(cur);
  stick = true;
  el.thread.scrollTop = el.thread.scrollHeight;
  return { height: Math.round(el.thread.scrollHeight), view: Math.round(el.thread.clientHeight) };
}

// Where a row of the chat remains, counted from the top of the view of the conversation.
function place(mid) {
  var node = el.col.querySelector('[data-mid="' + mid + '"]');
  if (!node && String(mid).indexOf('from:') === 0) {
    node = el.col.querySelector('[data-from="' + String(mid).slice(5) + '"]');
  }
  if (!node) { return { gone: true }; }
  var view = el.thread.getBoundingClientRect();
  var here = node.getBoundingClientRect();
  return {
    rel: Math.round(here.top - view.top),
    at: Math.round(el.thread.scrollTop),
    high: Math.round(el.thread.scrollHeight),
    followed: !!stick,
  };
}

// The row that the going to a chapter means: the first row of the chapter, which is the row
// after the message the chapter is named for.
function chapterMid(after) {
  var at = messageIndex(after);
  var node = turnForIndex(at + 1) || turnForIndex(at);
  return node ? (node.dataset.mid || 'from:' + node.dataset.from) : '';
}

const LATE = 420;      // how long the going is given to stand the chat
const SLACK = 40;      // how far from the top of the view the row may stand

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const inject = 'window.makeTalk = ' + makeTalk.toString()
    + '\nwindow.setup = ' + setup.toString()
    + '\nwindow.place = ' + place.toString()
    + '\nwindow.chapterMid = ' + chapterMid.toString();

  for (const one of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 760 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(inject);
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    const laid = await page.evaluate(() => setup(40));
    if (!(laid.height > laid.view * 8)) {
      note('the conversation is not long enough to go anywhere: ' + JSON.stringify(laid));
    }
    // The text of a chat that was drawn a moment ago is still settling - the theme of the
    // program arrives over the first frames - so the going is not judged until the height of
    // the conversation has stopped moving.
    await sleep(900);

    // One click of a node of the chapter line, a long way up the conversation.
    const first = await page.evaluate(() => {
      const mid = chapterMid('u4');
      return { mid: mid, before: window.place(mid) };
    });
    if (!first.mid) { note('the row a chapter node goes to was not found'); }
    await page.evaluate(() => { gotoChapter({ id: 'ch4', name: 'Chapter 4', after: 'u4' }); });
    await sleep(LATE);
    const landed = await page.evaluate((mid) => window.place(mid), first.mid);
    if (landed.gone) { note('the row a chapter node went to is removed from the chat'); }
    else if (Math.abs(landed.rel) > SLACK) {
      note('one click of a chapter node left the row ' + landed.rel + ' from the top of the '
        + 'view, of ' + laid.view + ' tall (it stood ' + first.before.rel + ' away before)');
    }
    if (landed.followed) { note('the chat still follows the answer after the user went looking'); }
    console.log('  ' + one.name + ' chapter: ' + JSON.stringify({
      laid: laid, from: first.before.rel, to: landed.rel, at: landed.at,
    }));

    // A second click, further up the conversation, and a third further down: each goes the
    // whole way, for the row named is what is checked, not the way left over from the last
    // going.
    const up = await page.evaluate(() => {
      const mid = chapterMid('u2');
      gotoChapter({ id: 'ch2', name: 'Chapter 2', after: 'u2' });
      return mid;
    });
    await sleep(LATE);
    const upLanded = await page.evaluate((mid) => window.place(mid), up);
    if (Math.abs(upLanded.rel) > SLACK) {
      note('a second click stopped ' + upLanded.rel + ' away instead of coming to the row');
    }
    const down = await page.evaluate(() => {
      const mid = chapterMid('u20');
      gotoChapter({ id: 'ch20', name: 'Chapter 20', after: 'u20' });
      return mid;
    });
    await sleep(LATE);
    const downLanded = await page.evaluate((mid) => window.place(mid), down);
    if (Math.abs(downLanded.rel) > SLACK) {
      note('a click down the conversation stopped ' + downLanded.rel + ' away from the row named');
    }

    // Text above the place that settle after the click - a row that grows, which is what an
    // image that arrived does - must not leave the chat remaining somewhere else than the row
    // that was requested.
    const grown = await page.evaluate(() => {
      const mid = chapterMid('u6');
      gotoChapter({ id: 'ch6', name: 'Chapter 6', after: 'u6' });
      el.col.children[0].style.minHeight = '900px';
      return mid;
    });
    await sleep(LATE);
    const grownLanded = await page.evaluate((mid) => window.place(mid), grown);
    if (Math.abs(grownLanded.rel) > SLACK) {
      note('text above the place that settled after the click left the chat '
        + grownLanded.rel + ' away from the row named');
    }

    // A draw that arrives while the chat is being was must not drag it back to the end of the
    // conversation.
    const drawn = await page.evaluate(() => {
      const mid = chapterMid('u8');
      gotoChapter({ id: 'ch8', name: 'Chapter 8', after: 'u8' });
      drawThread(cur);
      return mid;
    });
    await sleep(LATE);
    const drawnLanded = await page.evaluate((mid) => window.place(mid), drawn);
    if (Math.abs(drawnLanded.rel) > SLACK) {
      note('a draw while the chat was being was left it ' + drawnLanded.rel + ' away: '
        + JSON.stringify(drawnLanded));
    }
    if (drawnLanded.followed) { note('following was taken up again by the draw'); }

    // The line of the summarisations and the line of the user's own messages go the same path
    // and to the same end.
    const summary = await page.evaluate(() => {
      setup(40);
      const one = cur.handoffs[3];
      gotoSummary(one);
      return one.id;
    });
    await sleep(LATE);
    const summaryLanded = await page.evaluate((mid) => window.place(mid), summary);
    if (Math.abs(summaryLanded.rel) > SLACK) {
      note('a node of the summarisation line stopped ' + summaryLanded.rel + ' away from the row');
    }
    const dash = await page.evaluate(() => {
      setup(40);
      gotoUserMessage('u5');
      return 'u5';
    });
    await sleep(LATE);
    const dashLanded = await page.evaluate((mid) => window.place(mid), dash);
    if (Math.abs(dashLanded.rel) > SLACK) {
      note('a dash of the user message line stopped ' + dashLanded.rel + ' away from its row');
    }

    // The first node of the conversation is at the first turn of it, and not at the questions
    // of the plan, which are in the same space.
    const head = await page.evaluate(() => {
      setup(40);
      const turns = threadTurns(el.col);
      const mid = turns.length ? turns[0].dataset.mid : '';
      gotoChapter({ id: 'ch0', name: 'The first chapter', after: '' });
      return mid;
    });
    await sleep(LATE);
    const headLanded = await page.evaluate((mid) => window.place(mid), head);
    if (!headLanded.gone && Math.abs(headLanded.rel) > SLACK) {
      note('the first node of the conversation was not at its first row: '
        + JSON.stringify(headLanded));
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
