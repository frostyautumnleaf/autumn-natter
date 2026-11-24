// Check the one card an answer is behind in the corporate display.
//
// The corporate display hides private thoughts and ordinary tool messages. One card means an
// answer; progress messages are ordered sub-blocks and the final response stays outside.
// This drives the thread manually with the states the engine points out and checks what the page
// made.
//
// node corpwork-check.js AN_PAGE=../web/page.html node corpwork-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE_URL || (process.env.AN_PAGE
  ? 'file://' + path.resolve(process.env.AN_PAGE)
  : 'file://' + PAGE);

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto(REMOTE);
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
        work: [], work_head: '' };
      for (var k in extra) { m[k] = extra[k]; }
      return m;
    }

    // The parts of a summary of the work, as the engine gives them back.
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

    // The summary of the work of an answer that worked: a line with nothing under it, a line
    // with a command under it, and a run of three files made one after another.
    var SHORT = [heading('Mending the order', [
      line('Reading the parser.'),
      line('Running the tests.', [call('run_terminal', 'c1', 'Ran cargo test')]),
      line('Writing the notes down.', [group('create_file', 'Created files (3)', [
        call('create_file', 'c2', 'Created a file a.md'),
        call('create_file', 'c3', 'Created a file b.md'),
        call('create_file', 'c4', 'Created a file c.md')
      ])])
    ])];

    // A context of two answers: progress stays in the first card, its final output sits outside,
    // and the other answer has no visible thought sub-block.
    var talk = [
      said(1, 'You', 'Fix the order and run the tests.'),
      said(2, 'Assistant', 'I will read the parser first.',
        { reasoning: 'Read before touching.', reasoning_ms: 4200 }),
      said(3, 'Tool', 'cargo test', { tool: '6 tests passed' }),
      said(4, 'Assistant', 'The tests pass. Two items changed.',
        { reasoning: 'Say what changed.', reasoning_ms: 2600, work: SHORT,
          work_head: 'Ran, Created files (3)' }),
      said(5, 'You', 'And the bar?'),
      said(6, 'Assistant', 'The top bar takes its height from the text beside it.',
        { reasoning: 'The answer is already known.', reasoning_ms: 11800,
          work_head: 'Thought for 12 seconds' })
    ];

    function state(messages, chatting, extra) {
      var s = { messages: messages, live: '', reasoning: '', reasoning_ms: 0,
        reasoning_done: false, live_tools: [], live_work: [], live_work_head: '',
 generating: false, summarising: false, chapters: [],
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

    // One: one card for an answer, at the header of the row that carries the last message of
    // the answer, and the rows of the parts written before it away.
    where = 'a context of two answers';
    var box = look(state(talk, CHAT));
    var made = cards(box);
    if (made.length !== 2) {
      faults.push('a context of two answers made ' + made.length + ' cards, not two ' + where);
    }
    var heads = made.map(function (one) {
      return (one.querySelector('.whead') || {}).textContent || '';
    });
    if (heads.join(' / ') !== '▾Ran, Created files (3) / ▾Thought for 12 seconds') {
      faults.push('the headers of the two cards read "' + heads.join(' / ') + '" ' + where);
    }
    // The card is at the header of its row, and the row that carries it is the row of the last
    // message of the answer.
    var lastRow = box.querySelector('.turn[data-mid="4"]');
    if (!lastRow || !lastRow.firstElementChild || !lastRow.firstElementChild.classList.contains('work')) {
      faults.push('the card of an answer is not at the header of its row ' + where);
    }
    // Progress stays in the card while the final answer is a normal, visible chat message.
    var answerText = box.querySelector('.turn[data-mid="4"] > .work .wwords[data-said="2"]');
    if (!answerText) {
      faults.push('the progress update is not a sub-block in the card ' + where);
    }
    var finalPlain = lastRow && lastRow.querySelector(':scope > .plain');
    if (!finalPlain || getComputedStyle(finalPlain).display === 'none'
        || !/The tests pass\. Two items changed\./.test(finalPlain.textContent || '')
        || lastRow.querySelector(':scope > .work .wwords[data-said="4"]')) {
      faults.push('the final answer is not shown once outside the card ' + where);
    }
    if (!made[0].querySelector('.wwords')) {
      faults.push('the text the model stated while it worked are not in the card ' + where);
    }
    if (made[0].querySelector('.wthink')) {
      faults.push('the card shows a thought sub-block ' + where);
    }
    var expandable = Array.prototype.slice.call(made[0].querySelectorAll('.wblock'))
      .filter(function (one) { return one.querySelector(':scope > .wbbody'); });
    if (expandable.some(function (one) { return !one.classList.contains('shut'); })) {
      faults.push('an expandable sub-block is open by default ' + where);
    }
    // The rows of the parts written before the answer is offset; the text of the user does not.
    var away = ['2', '3'].filter(function (id) {
      var row = box.querySelector('.turn[data-mid="' + id + '"]');
      return row && row.classList.contains('t-hush');
    });
    if (away.length !== 2) {
      faults.push('a row of the parts of an answer is put on the page ' + where);
    }
    if (box.querySelector('.turn[data-mid="1"]').classList.contains('t-hush')) {
      faults.push('the text of the user is offset ' + where);
    }
    if (box.querySelector('.turn[data-mid="5"]').classList.contains('t-hush')) {
      faults.push('the second question of the user is offset ' + where);
    }

    // A direct answer with no tool work still has a card, and the final message remains outside.
    where = 'a direct answer with no tool work';
    box = look(state([
      said(20, 'You', 'What is 2 + 2?'),
      said(21, 'Assistant', '4.', { work_head: 'Done' })
    ], CHAT));
    var direct = box.querySelector('.turn[data-mid="21"] > .work');
    if (!direct) {
      faults.push('a direct answer with no logged work has no card ' + where);
    } else {
      var directHead = (direct.querySelector('.whead') || {}).textContent || '';
      if (!/Done/.test(directHead) || direct.querySelector('.wwords')) {
        faults.push('the direct answer was added to its progress card ' + where);
      }
    }
    var directPlain = box.querySelector('.turn[data-mid="21"] > .plain');
    if (!directPlain || getComputedStyle(directPlain).display === 'none'
        || !/4\./.test(directPlain.textContent || '')) {
      faults.push('the direct answer is not visible outside its card ' + where);
    }

    // The most important state change is the handoff from the live row to the saved answer.
    // Keep the same rendered thread while generation stops and the final message is persisted.
    where = 'the live-to-saved answer transition';
    var start = [said(40, 'You', 'Explain this briefly.')];
    box = look(state(start, CHAT, {
      generating: true, live: '', reasoning: 'Thinking through the explanation.',
      reasoning_ms: 700, live_work: [], live_work_head: 'Thinking'
    }));
    if (!box.querySelector('.turn:last-child > .work')) {
      faults.push('the live answer has no card before it finishes ' + where);
    }
    var saved = start.concat([said(41, 'Assistant', 'Here is the explanation.', {
      reasoning: 'Thinking through the explanation.', reasoning_ms: 700,
      work_head: 'Done'
    })]);
    box = look(state(saved, CHAT));
    if (!box.querySelector('.turn[data-mid="41"] > .work')) {
      faults.push('the card vanished when the live row became a saved answer ' + where);
    }
    var savedPlain = box.querySelector('.turn[data-mid="41"] > .plain');
    if (!savedPlain || getComputedStyle(savedPlain).display === 'none'
        || !/Here is the explanation/.test(savedPlain.textContent || '')) {
      faults.push('the final answer is not outside the saved card ' + where);
    }
    box = look(state(saved, CHAT));
    if (cards(box).length !== 1) {
      faults.push('the saved answer card did not survive the next page update ' + where);
    }

    // During a handoff the model's summary text remains hidden, but a tool-only step has no
    // saved Assistant message to carry its progress card. Keep that card above the summary
    // marker until the answer resumes.
    where = 'a tool-only answer during handoff summarisation';
    var duringHandoff = state([
      said(50, 'You', 'Research and report.'),
      said(51, 'Tool', 'Search result', { tool: 'Search returned' }),
      said(52, 'Summary', '', { work_head: '' })
    ], CHAT, {
      generating: true, summarising: true, live: 'summary model text stays hidden',
      reasoning: 'summary model thoughts stay hidden', live_work: SHORT,
      live_work_head: 'Searching the web'
    });
    box = look(duringHandoff);
    var heldCard = box.querySelector('.turn[data-mid="live-work"] > .work');
    if (!heldCard || !/Searching the web/.test(heldCard.textContent)) {
      faults.push('the work card disappeared during the handoff ' + where);
    } else if (heldCard.querySelector('.answer-words, .wthink')) {
      faults.push('summary text or thoughts are visible inside the held card ' + where);
    }
    if (box.textContent.indexOf('summary model text stays hidden') >= 0
        || box.textContent.indexOf('summary model thoughts stay hidden') >= 0) {
      faults.push('the hidden handoff text was displayed ' + where);
    }
    // If earlier assistant content already owns this answer's card, do not draw a duplicate.
    var storedHandoff = state([
      said(53, 'You', 'Research and report.'),
      said(54, 'Assistant', 'Checking sources.', { work: SHORT, work_head: 'Searching the web' }),
      said(55, 'Summary', '')
    ], CHAT, { generating: true, summarising: true, live_work: [],
      live_work_head: 'Searching the web' });
    box = look(storedHandoff);
    if (cards(box).length !== 1) {
      faults.push('handoff duplicated a card already stored on the answer ' + where);
    }

    // Two: a run of calls of one tool is one block, stated in the plural with the count, and
    // holds the calls under it. A line of the summary with nothing under it is a plain line
    // with nothing to open.
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
      faults.push('a line of the summary with nothing under it is not a plain line ' + where);
    } else if (spare[0].querySelector('.wfold')) {
      faults.push('a plain line of the summary carries a marker to open ' + where);
    }

    // Three: private thought text never appears as a Corporate card sub-block.
    where = 'an answer with private thought text';
    if (made[1].querySelector('.wthink, .wthought')) {
      faults.push('private thought text appears in the card ' + where);
    }

    // Four: an answer arriving is held by the card at the header of its turn, and the rows of
    // its parts written before is offset. Live text is a sub-block while it is being written.
    where = 'an answer coming in';
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'Still writing this response.', reasoning: 'Say what changed.',
      reasoning_ms: 900, live_work: SHORT, live_work_head: 'Creating files (3)'
    }));
    var liveRow = box.querySelector('.turn:last-child');
    var liveCard = liveRow && liveRow.querySelector(':scope > .work');
    if (!liveCard) {
      faults.push('an answer arriving has no card at the header of its turn ' + where);
    } else {
      var liveHead = (liveCard.querySelector('.whead') || {}).textContent || '';
      if (!/Creating files \(3\)/.test(liveHead)) {
        faults.push('the header of the card of an answer arriving reports "' + liveHead + '" ' + where);
      }
      var liveSub = liveCard.querySelector('.wwords[data-said="live"]');
      if (liveCard.querySelector('.wthink') || !liveSub
          || !/Still writing this response/.test(liveSub.textContent)) {
        faults.push('the text of the work is missing or a thought is shown in its card ' + where);
      }
    }
    if (box.querySelector('.turn[data-mid="2"]').classList.contains('t-hush') === false) {
      faults.push('a part of the answer arriving is put on the page as well ' + where);
    }
    // More text of the same answer arrives: the card is put again, and remains once, and the
    // live text stays hidden from the chat while it is shown as a sub-block.
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'Still writing this response. More detail follows.',
      reasoning: 'Say what changed.', reasoning_ms: 1400, live_work: SHORT,
      live_work_head: 'Creating files (3)'
    }));
    var again = box.querySelector('.turn:last-child');
    if (again.querySelectorAll(':scope > .work').length !== 1) {
      faults.push('an answer arriving is behind more than one card ' + where);
    }
    var flowing = again.querySelector('.plain');
    if (!flowing || !flowing.classList.contains('corp-held-words')
        || getComputedStyle(flowing).display !== 'none') {
      faults.push('the text of an answer arriving is visible in the chat before it is complete ' + where);
    }
    var liveWords = again.querySelector(':scope > .work .wwords[data-said="live"]');
    if (!liveWords || !/More detail follows/.test(liveWords.textContent)) {
      faults.push('the live answer is not shown as a sub-block while it is being written ' + where);
    }

    // When the model moves from thinking to a tool call, the live answer is grouped with the
    // tool's thought row. That row must stay visible because it now carries the live card.
    where = 'a live answer while its tool call runs';
    box = look(state([
      said(60, 'You', 'Run the tests.'),
      said(61, 'Tool', 'cargo test', { reasoning: 'I will run the focused tests now.',
        tool: 'Running ...' })
    ], CHAT, { generating: true, live: '', reasoning: '', live_work: SHORT,
      live_work_head: 'Running a command cargo test' }));
    var runningTurn = box.querySelector('.turn[data-mid="61"]');
    if (!runningTurn || !runningTurn.querySelector(':scope > .work')) {
      faults.push('the live card is missing while the tool call runs ' + where);
    }
    if (runningTurn && runningTurn.classList.contains('t-hush')) {
      faults.push('the thinking/tool group hid its live corporate card ' + where);
    }

    // Work from a completed tool step must appear between the thought before that tool and
    // the next thought, in the order the steps actually happened.
    where = 'work sub-blocks in creation order';
    var ordered = [
      said(70, 'You', 'Check the project.'),
      said(71, 'Assistant', 'Checking the project before the tool.', {
        reasoning: 'First thought before the tool.' }),
      said(72, 'Tool', 'search output', { tool: 'Search returned' }),
      said(73, 'Assistant', '', { reasoning: 'Second thought after the tool.',
        work: [heading('Check the test suite', [line('Running the checks.', [
          call('run_terminal', 'ordered-call', 'Ran pytest')
        ])])], work_head: 'Ran' }),
      said(74, 'Assistant', 'The checks passed.', { reasoning: 'Final thought.',
        work_head: 'Searched the web, Ran' })
    ];
    box = look(state(ordered, CHAT));
    var orderedCards = cards(box);
    var orderedCard = orderedCards.length ? orderedCards[orderedCards.length - 1] : null;
    var orderedWords = orderedCard ? orderedCard.textContent : '';
    var firstThought = orderedWords.indexOf('First thought before the tool.');
    var firstSaid = orderedWords.indexOf('Checking the project before the tool.');
    var workStep = orderedWords.indexOf('Check the test suite');
    var secondThought = orderedWords.indexOf('Second thought after the tool.');
    var orderedOutputs = orderedCard ? orderedCard.querySelectorAll('.wwords') : [];
    if (!orderedCard || firstThought < 0 || firstSaid < 0 || workStep < 0
        || secondThought < 0 || firstThought >= firstSaid || firstSaid >= workStep
        || workStep >= secondThought || orderedOutputs.length !== 1) {
      faults.push('the work sub-blocks are out of creation order ' + where + ': '
        + firstThought + '/' + firstSaid + '/' + workStep + '/' + secondThought
        + ', assistant outputs=' + orderedOutputs.length + ' ' + orderedWords);
    }
    var finalRow = box.querySelector('.turn[data-mid="74"]');
    var finalText = finalRow && finalRow.querySelector(':scope > .plain');
    if (!finalText || getComputedStyle(finalText).display === 'none'
        || !/The checks passed/.test(finalText.textContent || '')) {
      faults.push('the last assistant message is not outside the card ' + where);
    }

    // A live assistant output remains in the card until generation ends.
    where = 'a streaming answer sub-block';
    box = look(state(talk.slice(0, 4), CHAT, {
      generating: true, live: 'The tests pass.', reasoning: 'Say what changed.',
      reasoning_ms: 900, live_work: SHORT, live_work_head: 'Ran',
    }));
    var shut = box.querySelector('.turn:last-child > .work .whead');
    if (!shut || !/Ran/.test(shut.textContent)) {
      faults.push('the header of the card of a closed block reports nothing of the work ' + where);
    }
    var closedRow = box.querySelector('.turn:last-child');
    var finalWords = closedRow && closedRow.querySelector(':scope > .work .wwords[data-said="live"]');
    var duplicate = closedRow && closedRow.querySelector(':scope > .plain');
    if (!finalWords || !/The tests pass/.test(finalWords.textContent)
        || !duplicate || getComputedStyle(duplicate).display !== 'none') {
      faults.push('the streaming answer is missing from the card or shown in the chat ' + where);
    }

    // Six: with the corporate display off nothing of this remains: no card, and no row of an
    // answer away.
    where = 'the corporate display off';
    box = look(state(talk, PLAIN));
    if (cards(box).length) {
      faults.push('a card is with the corporate display off ' + where);
    }
    var hushed = turns(box).filter(function (one) { return one.classList.contains('t-hush'); });
    if (hushed.length) {
      faults.push('a row is offset with the corporate display off ' + where);
    }
    if (!box.querySelector('.turn[data-mid="2"] .think')) {
      faults.push('the thought of the model is hidden with the corporate display off ' + where);
    }

    // File call blocks retain the line-change markers in each nested sub-block.
    where = 'a file edit sub-block';
    var changed = [heading('Update the parser', [line('Applying the fix.', [
      call('edit_file', 'e1', 'Edited file src/parser.rs', { adds: 3, cuts: 1,
        file: 'src/parser.rs' })
    ])])];
    box = look(state([said(30, 'You', 'Fix the parser.'), said(31, 'Assistant', 'Done.', {
      work: changed, work_head: 'Edited 1 file'
    })], CHAT));
    var editCard = box.querySelector('.turn[data-mid="31"] > .work');
    var plus = editCard && editCard.querySelector('.wplus');
    var minus = editCard && editCard.querySelector('.wminus');
    if (!plus || plus.textContent !== '+3' || !minus || minus.textContent !== '-1') {
      faults.push('the edit sub-block does not show its added and removed line counts ' + where);
    }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('one card means an answer, at the header of the row that carries its last '
    + 'message; progress outputs stay in ordered card sub-blocks while the final answer is '
    + 'outside; private thoughts are hidden and tool outcomes are inside; sub-blocks start '
    + 'collapsed; a run of calls of one tool is '
    + 'one block in the plural; '
    + 'a line of the short with nothing under it is a plain line; an answer coming in is '
    + 'held by one card and puts it again as it grows; direct answers keep their card; file '
    + 'edits keep line counts; the card survives live-to-saved and handoff transitions and '
    + 'stays visible while tools run; work sub-blocks remain in creation order; with the '
    + 'display off nothing of this stands');
})().catch(function (e) { console.error(e); process.exit(1); });
