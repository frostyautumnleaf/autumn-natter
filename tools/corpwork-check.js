// Check the one card an answer stands behind in the corporate display.
//
// The corporate display hides the thoughts and the calls whole. One card stands
// for an answer: its head says what the model is at, and inside it stand the
// thoughts of the model, the words it wrote while it worked, and the short of the
// work it wrote itself. The answer the model ended with stands outside the card,
// in the plain words of the chat, and the rows of the parts written before the
// answer stand away, for the card holds them. This drives the thread by hand with
// the states the engine hands out, and looks at what the page made.
//
//   node corpwork-check.js
//   AN_PAGE=../web/page.html node corpwork-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE) : PAGE;

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + REMOTE);
  const out = await page.evaluate(async () => {
    var faults = [];
    var where = 'at the start';

    function look(state) {
      readChatting(state);
      drawThread(state);
      if (ink.thought || ink.words) {
        ink.shown = ink.said.length;
        ink.shownMusing = ink.musing.length;
        writeSaid();
        if (ink.thought) { writeThought(); }
      }
      return el.col;
    }

    function said(id, role, body, extra) {
      var m = { id: id, role: role, body: body, when: '10:' + (id % 10), tool: '',
        mono: false, failed: false, reasoning: '', reasoning_ms: 0, attachments: [],
        work: [], work_head: '', work_closed: false };
      for (var k in extra) { m[k] = extra[k]; }
      return m;
    }

    // The parts of a short of the work, as the engine gives them back.
    function line(head, sub) {
      return { head: head, lines: [], tool: 'work_log', id: '', done: false, failed: false,
        adds: 0, cuts: 0, command: '', file: '', output: '', sub: sub || [],
        line: true, group: false };
    }

    function call(tool, id, head, extra) {
      var one = { head: head, lines: [], tool: tool, id: id, done: true, failed: false,
        adds: 0, cuts: 0, command: '', file: '', output: '', sub: [],
        line: false, group: false };
      for (var k in extra) { one[k] = extra[k]; }
      return one;
    }

    function group(tool, head, run) {
      return { head: head, lines: [], tool: tool, id: '', done: true, failed: false,
        adds: 0, cuts: 0, command: '', file: '', output: '', sub: run,
        line: false, group: true };
    }

    function heading(head, sub) {
      return { head: head, lines: [], tool: 'work_log', id: '', done: false, failed: false,
        adds: 0, cuts: 0, command: '', file: '', output: '', sub: sub,
        line: false, group: false };
    }

    var CHAT = { collapse_thinking: true, grow_thinking: true, tools_in_thinking: true,
      collapse_tools: true, highlight: true, chapters: true, corporate: true };
    var PLAIN = { collapse_thinking: true, grow_thinking: true, tools_in_thinking: true,
      collapse_tools: true, highlight: true, chapters: true, corporate: false };

    // The short of the work of an answer that worked: a line with nothing under
    // it, a line with a command under it, and a run of three files made one
    // after another.
    var SHORT = [heading('Mending the order', [
      line('Reading the parser.'),
      line('Running the tests.', [call('run_terminal', 'c1', 'Ran a command cargo test')]),
      line('Writing the notes down.', [group('create_file', 'Created files (3)', [
        call('create_file', 'c2', 'Created a file a.md'),
        call('create_file', 'c3', 'Created a file b.md'),
        call('create_file', 'c4', 'Created a file c.md')
      ])])
    ])];

    // A talk of two answers: the one worked, and ended with its words outside the
    // card; the other is nothing but a thought.
    var talk = [
      said(1, 'You', 'Mend the order and run the tests.'),
      said(2, 'Assistant', 'I will read the parser first.',
        { reasoning: 'Read before touching.', reasoning_ms: 4200 }),
      said(3, 'Tool', 'cargo test', { tool: '6 tests passed' }),
      said(4, 'Assistant', 'The tests pass. Two things changed.',
        { reasoning: 'Say what changed.', reasoning_ms: 2600, work: SHORT,
          work_head: 'Ran a command, Created files (3)', work_closed: true }),
      said(5, 'You', 'And the bar?'),
      said(6, 'Assistant', 'The bar takes its height from the words beside it.',
        { reasoning: 'The answer is already known.', reasoning_ms: 11800,
          work_head: 'Thought for 12 seconds', work_closed: true })
    ];

    function state(messages, chatting, extra) {
      var s = { messages: messages, live: '', reasoning: '', reasoning_ms: 0,
        reasoning_done: false, live_tools: [], live_work: [], live_work_head: '',
        live_work_closed: false, generating: false, summarising: false, chapters: [],
        handoffs: [], chat: { id: 'c1', title: 'Probe' }, chatting: chatting };
      for (var k in extra) { s[k] = extra[k]; }
      return s;
    }

    function cards(box) {
      return Array.prototype.slice.call(box.querySelectorAll('.work'));
    }

    function turns(box) {
      return Array.prototype.slice.call(box.querySelectorAll('.turn'));
    }

    // One: one card for an answer, at the head of the row that carries the last
    // message of the answer, and the rows of the parts written before it away.
    where = 'a talk of two answers';
    var box = look(state(talk, CHAT));
    var made = cards(box);
    if (made.length !== 2) {
      faults.push('a talk of two answers made ' + made.length + ' cards, not two ' + where);
    }
    var heads = made.map(function (one) {
      return (one.querySelector('.whead') || {}).textContent || '';
    });
    if (heads.join(' / ') !== '▾Ran a command, Created files (3) / ▾Thought for 12 seconds') {
      faults.push('the heads of the two cards read "' + heads.join(' / ') + '" ' + where);
    }
    // The card stands at the head of its row, and the row that carries it is the
    // row of the last message of the answer.
    var lastRow = box.querySelector('.turn[data-mid="4"]');
    if (!lastRow || !lastRow.firstElementChild || !lastRow.firstElementChild.classList.contains('work')) {
      faults.push('the card of an answer does not stand at the head of its row ' + where);
    }
    // The words the model said while it worked stand inside the card, and the
    // answer it ended with stands outside it.
    var said = box.querySelector('.turn[data-mid="4"] .answer-words');
    if (!said) {
      faults.push('the answer the model ended with does not stand in the chat ' + where);
    }
    if (made[0].querySelector('.answer-words')) {
      faults.push('the words of the answer stand inside the card as well ' + where);
    }
    if (!made[0].querySelector('.wwords')) {
      faults.push('the words the model said while it worked do not stand in the card ' + where);
    }
    if (!made[0].querySelector('.wthink')) {
      faults.push('the thought of the model does not stand in the card ' + where);
    }
    // The rows of the parts written before the answer stand away; the words of
    // the user do not.
    var away = ['2', '3'].filter(function (id) {
      var row = box.querySelector('.turn[data-mid="' + id + '"]');
      return row && row.classList.contains('t-hush');
    });
    if (away.length !== 2) {
      faults.push('a row of the parts of an answer is put on the page ' + where);
    }
    if (box.querySelector('.turn[data-mid="1"]').classList.contains('t-hush')) {
      faults.push('the words of the user stand away ' + where);
    }
    if (box.querySelector('.turn[data-mid="5"]').classList.contains('t-hush')) {
      faults.push('the second question of the user stands away ' + where);
    }

    // Two: a run of calls of one tool is one block, said in the plural with the
    // count, and holds the calls under it. A line of the short with nothing under
    // it is a plain line with nothing to open.
    where = 'a run of calls of one tool';
    var runs = Array.prototype.slice.call(made[0].querySelectorAll('.wblock'))
      .filter(function (one) {
        var head = one.querySelector(':scope > .wbhead');
        return head && /Created files \(3\)/.test(head.textContent);
      });
    if (runs.length !== 1) {
      faults.push('three files made one after another did not become one block ' + where);
    } else {
      var held = runs[0].querySelectorAll(':scope > .wbbody > .wblock').length;
      if (held !== 3) {
        faults.push('the block of a run holds ' + held + ' calls, not three ' + where);
      }
    }
    var spare = made[0].querySelectorAll('.wplainline');
    if (spare.length !== 1 || !/Reading the parser/.test(spare[0].textContent)) {
      faults.push('a line of the short with nothing under it is not a plain line ' + where);
    } else if (spare[0].querySelector('.wfold')) {
      faults.push('a plain line of the short carries a mark to open ' + where);
    }

    // Three: an answer of nothing but a thought says how long the model thought
    // for once, and not twice.
    where = 'an answer of nothing but a thought';
    if (made[1].querySelectorAll('.wthink').length !== 1) {
      faults.push('a thought of an answer stands twice over ' + where);
    }
    if (made[1].querySelector('.wthink .wbhead')) {
      faults.push('the thought of an answer says again what the head of the card says ' + where);
    }

    // Four: an answer coming in is held by the card at the head of its turn, and
    // the rows of its parts written before stand away. The words coming in stand
    // outside the card.
    where = 'an answer coming in';
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'The tests pass.', reasoning: 'Say what changed.',
      reasoning_ms: 900, live_work: SHORT, live_work_head: 'Creating files (3)'
    }));
    var liveRow = box.querySelector('.turn:last-child');
    var liveCard = liveRow && liveRow.querySelector(':scope > .work');
    if (!liveCard) {
      faults.push('an answer coming in has no card at the head of its turn ' + where);
    } else {
      var liveHead = (liveCard.querySelector('.whead') || {}).textContent || '';
      if (!/Creating files \(3\)/.test(liveHead)) {
        faults.push('the head of the card of an answer coming in says "' + liveHead + '" ' + where);
      }
      if (!liveCard.querySelector('.wthink') || !liveCard.querySelector('.wwords')) {
        faults.push('the thought or the words of the work are not in the card of an '
          + 'answer coming in ' + where);
      }
    }
    if (box.querySelector('.turn[data-mid="2"]').classList.contains('t-hush') === false) {
      faults.push('a part of the answer coming in is put on the page as well ' + where);
    }
    // More words of the same answer come in: the card is put again, and stands
    // once, and the words of the answer stay outside it.
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'The tests pass. Two things changed.',
      reasoning: 'Say what changed.', reasoning_ms: 1400, live_work: SHORT,
      live_work_head: 'Creating files (3)'
    }));
    var again = box.querySelector('.turn:last-child');
    if (again.querySelectorAll(':scope > .work').length !== 1) {
      faults.push('an answer coming in stands behind more than one card ' + where);
    }
    var flowing = again.querySelector('.plain');
    if (!flowing || !/Two things changed/.test(flowing.textContent)) {
      faults.push('the words of an answer coming in do not stand outside the card ' + where);
    }

    // Five: the block closed by the model's own tool. The words written after the
    // mark stand outside the card, and the head of the card is done.
    where = 'the block closed by the model';
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'The tests pass.', reasoning: 'Say what changed.',
      reasoning_ms: 900, live_work: SHORT, live_work_head: 'Ran a command',
      live_work_closed: true
    }));
    var shut = box.querySelector('.turn:last-child > .work .whead');
    if (!shut || !/Ran a command/.test(shut.textContent)) {
      faults.push('the head of the card of a closed block says nothing of the work ' + where);
    }

    // Six: with the corporate display off nothing of this stands: no card, and no
    // row of an answer away.
    where = 'the corporate display off';
    box = look(state(talk, PLAIN));
    if (cards(box).length) {
      faults.push('a card stands with the corporate display off ' + where);
    }
    var hushed = turns(box).filter(function (one) { return one.classList.contains('t-hush'); });
    if (hushed.length) {
      faults.push('a row stands away with the corporate display off ' + where);
    }
    if (!box.querySelector('.turn[data-mid="2"] .think')) {
      faults.push('the thought of the model is hidden with the corporate display off ' + where);
    }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('one card stands for an answer, at the head of the row that carries its last '
    + 'message; the thoughts, the words of the work and the short of the work stand inside '
    + 'it, the answer itself outside; a run of calls of one tool is one block in the plural; '
    + 'a line of the short with nothing under it is a plain line; an answer coming in is '
    + 'held by one card and puts it again as it grows; and with the display off nothing of '
    + 'this stands');
})().catch(function (e) { console.error(e); process.exit(1); });
