// Check the line of chapter nodes beside the chat: a node takes the chat to the
// place its chapter begins.
//
// The page is loaded as it stands, with no engine under it. The talk is written by
// hand, with a train of thought that holds the runs of two tools in one row, for a
// chapter may begin after a message that stands in the middle of such a row. The
// moves are made instant, so the place the chat came to is read and not watched.
//
//   node chapter-check.js
//   AN_PAGE=../web/page.html AN_FETCH=1 node chapter-check.js
//
// Five things are looked at:
//   * the line holds one node for one chapter, and the nodes stand in the order
//     the chapters begin;
//   * a node takes the chat to the answer that begins its chapter, when the
//     message the chapter begins after stands in the middle of a row that holds a
//     train of thought;
//   * a node takes the chat to the answer that begins its chapter, when that
//     message stands in a row of its own;
//   * a node takes the chat to the row that holds its chapter, when the answer
//     that begins the chapter stands inside a train of thought;
//   * a chapter with no message before it takes the chat to the first turn;
//   * the chat stops following the writing when the user goes looking, and the
//     place it came to is the place of the chapter and not the place of the row
//     after it.
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE) : PAGE;

const wait = (ms) => Promise.resolve().then(() => new Promise((r) => setTimeout(r, ms)));

const msg = (id, role, body, extra) => Object.assign({
  id: id, role: role, body: body, when: '09:00', when_full: 'today at 09:00', tool: '',
  mono: false, failed: false, reasoning: '', reasoning_ms: 0, attachments: []
}, extra || {});

// A talk long enough to scroll, with two chapters in it. The first chapter begins
// inside a row that holds a train of thought and the runs of two tools; the second
// begins after a message that stands in a row of its own.
const TALK = [
  msg('q1', 'You', 'Set up the parser and test it.'),
  msg('a1', 'Assistant', 'I will start with the shape of the reader.\n\n'
    + Array.from({ length: 40 }, (x, n) => 'A line of the first answer, number ' + n + '.')
      .join('\n')),
  msg('q2', 'You', 'Go on with the reader.'),
  msg('t1', 'Assistant', '', { reasoning: 'The reader needs a token of the shape first.' }),
  msg('t2', 'Tool', 'notes/shape.rs', { tool_name: 'create_file', tool: '120 lines written' }),
  msg('t3', 'Tool', 'cargo build', { tool_name: 'run_command', tool: 'Finished in 4 s' }),
  msg('a2', 'Assistant', '# The plan for the parser\n\nThe reader is the first part.\n\n'
    + Array.from({ length: 30 }, (x, n) => 'A line of the chapter, number ' + n + '.').join('\n')),
  msg('q3', 'You', 'Now the tests.'),
  msg('a3', 'Assistant', '# Running the tests\n\nThe suite runs whole.\n\n'
    + Array.from({ length: 30 }, (x, n) => 'A line of the second chapter, number ' + n + '.')
      .join('\n')),
  msg('u1', 'Assistant', '', { reasoning: 'Now a last look at the whole of it.' }),
  msg('u2', 'Tool', 'ls notes', { tool_name: 'run_command', tool: 'shape.rs plan.md' }),
  msg('u3', 'Assistant', '# The last look\n\nWords of the last look.\n\n'
    + Array.from({ length: 20 }, (x, n) => 'A line of the last look, number ' + n + '.')
      .join('\n'), { reasoning: 'The look goes at the end of the talk.' }),
  msg('a4', 'Assistant', 'Both chapters stand, and the work is at its end.')
];

const CHAPTERS = [
  { id: 'ch1', name: 'The plan for the parser', after: 't3' },
  { id: 'ch2', name: 'Running the tests', after: 'q3' },
  { id: 'ch3', name: 'The last look', after: 'u2' },
  { id: 'ch0', name: 'The very first words', after: '' }
];

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1180, height: 620 });
  const faults = [];
  page.on('pageerror', (e) => faults.push(String(e)));
  page.on('console', (m) => {
    if (m.type() === 'error' && !/Fetch API cannot load|ERR_|Failed to fetch|favicon/.test(m.text())) {
      faults.push(m.text());
    }
  });
  await page.goto('file://' + REMOTE);
  await wait(400);

  const trouble = [];
  const note = (what) => { trouble.push(what); console.log('  FAULT: ' + what); };

  // The talk, the chapters, and the moves made instant.
  const built = await page.evaluate((talk, chapters) => {
    reduce = true;
    cur = { chatting: { send_anim: false, chapters: true, tools_in_thinking: true },
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: talk,
      chapters: chapters };
    readChatting(cur);
    drawThread(cur);
    drawChapters(cur);
    var nodes = document.querySelectorAll('.chapter');
    return { nodes: nodes.length, rows: document.querySelectorAll('#col .turn').length,
      height: el.thread.scrollHeight, view: el.thread.clientHeight };
  }, TALK, CHAPTERS);
  if (built.nodes !== CHAPTERS.length) {
    note('the line holds ' + built.nodes + ' nodes for ' + CHAPTERS.length + ' chapters');
  }
  if (!(built.height > built.view + 200)) {
    note('the talk is not taller than the view, so the check proves nothing');
  }
  console.log('built:', JSON.stringify(built));

  // Where the chat stands after a node was pressed: the turn at the top of the
  // view, and the words it holds.
  const look = () => `(() => {
    var view = el.thread.getBoundingClientRect();
    var turns = threadTurns(el.col);
    var at = -1;
    for (var n = 0; n < turns.length; n++) {
      var b = turns[n].getBoundingClientRect();
      if (b.bottom - view.top > 24) { at = n; break; }
    }
    var top = at < 0 ? null : turns[at];
    var above = at > 0 ? turns[at - 1] : null;
    return {
      at: at,
      offset: top ? Math.round(top.getBoundingClientRect().top - view.top) : -1,
      words: top ? top.textContent : '',
      above: above ? above.textContent : '',
      scrolled: Math.round(el.thread.scrollTop),
      stuck: stick
    };
  })()`;
  const few = (words) => String(words || '').slice(0, 58);

  // 1: the chapter that begins after a message standing in the middle of a row
  // that holds a train of thought. The row was looked for by the first message of
  // it alone, found nowhere, and the chat did not move at all.
  await page.evaluate(() => {
    el.thread.scrollTop = 0;
    document.querySelectorAll('.chapter')[0].click();
  });
  await wait(120);
  const atFirst = await page.evaluate(look());
  if (!atFirst.words.includes('The plan for the parser')) {
    note('the first node did not bring the chapter it names to the top: "'
      + few(atFirst.words) + '"');
  }
  if (atFirst.above.includes('The plan for the parser')) {
    note('the chapter was carried past its place: it stands above the top');
  }
  if (atFirst.scrolled <= 0) { note('the chat did not move at all'); }
  if (atFirst.stuck !== false) { note('the chat still follows the writing after the user went looking'); }
  console.log('first:', JSON.stringify({ at: atFirst.at, offset: atFirst.offset,
    scrolled: atFirst.scrolled, stuck: atFirst.stuck, words: few(atFirst.words) }));

  // 2: the chapter that begins after a message standing in a row of its own.
  await page.evaluate(() => { el.thread.scrollTop = 0; });
  await wait(60);
  await page.evaluate(() => { document.querySelectorAll('.chapter')[1].click(); });
  await wait(120);
  const atSecond = await page.evaluate(look());
  if (!atSecond.words.includes('Running the tests')) {
    note('the second node did not bring its chapter to the top: "' + atSecond.words + '"');
  }
  if (atSecond.above.includes('Running the tests')) {
    note('the second chapter was carried past its place');
  }
  console.log('second:', JSON.stringify({ at: atSecond.at, offset: atSecond.offset,
    scrolled: atSecond.scrolled, words: few(atSecond.words) }));

  // 3: the chapter that begins in the middle of a row: its answer stands inside
  // the train of thought it came out of. The row is the nearest place the chat can
  // go to, and it must hold the words of the chapter.
  await page.evaluate(() => { el.thread.scrollTop = 0; });
  await wait(60);
  await page.evaluate(() => { document.querySelectorAll('.chapter')[2].click(); });
  await wait(120);
  const atLast = await page.evaluate(look());
  if (!atLast.words.includes('The last look')) {
    note('the third node did not bring its chapter to the top: "' + few(atLast.words) + '"');
  }
  if (atLast.above.includes('The last look')) {
    note('the third chapter was carried past its place');
  }
  console.log('last:', JSON.stringify({ at: atLast.at, offset: atLast.offset,
    scrolled: atLast.scrolled, words: few(atLast.words) }));

  // 4: the chapter with no message before it: the first turn of the talk.
  await page.evaluate(() => { el.thread.scrollTop = el.thread.scrollHeight; });
  await wait(60);
  await page.evaluate(() => { document.querySelectorAll('.chapter')[3].click(); });
  await wait(120);
  const atZero = await page.evaluate(look());
  if (atZero.at !== 0) {
    note('the chapter with nothing before it did not go to the first turn: at ' + atZero.at);
  }
  if (!atZero.words.includes('Set up the parser')) {
    note('the first turn holds other words: "' + atZero.words + '"');
  }
  console.log('zero:', JSON.stringify(atZero));

  // 4: the place a node asks for is the place of its own chapter and no other:
  // the three nodes lead to three different places.
  const places = await page.evaluate(async (names) => {
    var out = [];
    var nodes = document.querySelectorAll('.chapter');
    for (var n = 0; n < nodes.length; n++) {
      el.thread.scrollTop = 0;
      nodes[n].click();
      await new Promise(function (r) { setTimeout(r, 90); });
      out.push(Math.round(el.thread.scrollTop));
    }
    return out;
  }, CHAPTERS.map((c) => c.name));
  if (new Set(places).size !== places.length) {
    note('two nodes of the line lead to the same place: ' + places.join(', '));
  }
  console.log('places:', JSON.stringify(places));

  await browser.close();
  console.log(trouble.length ? 'FAIL ' + trouble.length + ' faults' : 'ok   the chapter line beside the chat');
  process.exit(trouble.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(1); });
