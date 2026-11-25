// Check saved and streaming Corporate answers in either frontend.
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE_URL || (process.env.AN_PAGE
  ? 'file://' + path.resolve(process.env.AN_PAGE)
  : 'file://' + PAGE);

(async () => {
  const shell = process.env.AN_SHELL_BIN || process.env.AN_CHROME;
  const browser = await puppeteer.launch({
    headless: shell ? true : 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto(REMOTE);
  await page.evaluate(async () => {
    function check(ok, message) {
      if (!ok) { throw new Error(message); }
    }
    function look(state) {
      readChatting(state);
      drawThread(state);
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

    // Two answers with reasoning before prose and older saved work between messages.
    var talk = [
      said(1, 'You', 'Fix the order and run the tests.'),
      said(2, 'Assistant', 'I will read the parser first.',
        { reasoning: 'Read before touching.', reasoning_ms: 4200 }),
      said(3, 'Tool', 'cargo test', { tool: '6 tests passed' }),
      said(4, 'Assistant', 'The tests pass. Two items changed.',
        { reasoning: 'Say what changed.', reasoning_ms: 2600, work: SHORT,
          work_head: 'Ran a command, Created files (3)' }),
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

    function cards(box) { return Array.from(box.querySelectorAll('.turn > .work')); }
    function title(card) { return card.querySelector('.whead').textContent; }

    const box = look(state(talk, CHAT));
    const [first, legacy, second] = cards(box);
    check(cards(box).length === 3, 'each period of activity needs its own card');
    check(title(first).includes('Thought for 4 seconds') && title(legacy).includes('Created files (3)')
      && title(second).includes('Thought for 12 seconds'), 'headings must describe their own activity');
    check(first.nextElementSibling.querySelector('.answer-words').textContent === talk[1].body
      && Array.from(box.querySelectorAll('.answer-words'), node => node.textContent)
        .join('|') === [talk[1].body, talk[3].body, talk[5].body].join('|'),
      'every assistant message must appear between cards');
    check(!box.textContent.includes('Read before touching.'), 'private thoughts are visible');
    check(Array.from(box.querySelectorAll('.work, .wblock')).every(n => n.classList.contains('shut')),
      'all disclosures should start closed');

    const blocks = Array.from(legacy.querySelectorAll('.wblock'));
    const command = blocks.find(n => n.firstElementChild.textContent.includes('Ran cargo test'));
    check(command.firstElementChild.querySelector('.wtoolmark path[d]'), 'legacy command lost its icon');
    const files = blocks.find(n => n.firstElementChild.textContent.includes('Created files (3)'));
    check(files.lastElementChild.children.length === 3, 'legacy group lost its nested files');
    const emptyLine = blocks.find(n => n.firstElementChild.textContent.includes('Reading the parser.'));
    check(emptyLine.firstElementChild.tabIndex === -1, 'empty progress line should have no active control');

    const start = [said(40, 'You', 'Explain this briefly.')];
    const firstWords = 'I’ll check the 🧪 results.';
    function phases() {
      return Array.from(box.querySelector('.turn.t-ai').children)
        .filter(node => node.matches('.work, .corporate-message'));
    }
    look(state(start, CHAT, { generating: true, reasoning: 'Private thought', thought_offsets: [0] }));
    const active = cards(box)[0];
    check(title(active).includes('Thinking') && !active.textContent.includes('Private thought'),
      'thinking should keep a card without exposing thoughts');
    look(state(start, CHAT, { generating: true, live: firstWords, reasoning: 'Private thought',
      reasoning_done: true, reasoning_ms: 1300, thought_offsets: [0] }));
    const firstProse = phases()[1];
    check(phases()[0] === active && firstProse.textContent === firstWords
      && !active.querySelector('.busy'), 'the streamed message should follow its activity card');

    const offsets = [0, firstWords.length];
    look(state(start, CHAT, { generating: true, live: firstWords, reasoning: 'Private thought again',
      thought_offsets: offsets }));
    const resumed = phases()[2];
    check(phases()[0] === active && phases()[1] === firstProse && title(resumed).includes('Thinking'),
      'resumed thinking must start a new card after the existing message');
    const finalWords = 'Here is the **answer**.';
    look(state(start, CHAT, { generating: true, live: firstWords + finalWords,
      reasoning: 'Private thought again', reasoning_done: true, thought_offsets: offsets }));
    const finalProse = phases()[3];
    check(phases()[2] === resumed && finalProse.querySelector('strong').textContent === 'answer',
      'the final response should stream outside the new card');
    const saved = said(41, 'Assistant', firstWords + finalWords,
      { reasoning: 'Private thought again', thought_offsets: offsets });
    look(state(start.concat(saved), CHAT));
    check(phases()[0] === active && phases()[1] === firstProse &&
      phases()[2] === resumed && phases()[3] === finalProse,
      'saving the answer must preserve the existing cards and messages');
    check(!box.textContent.includes('Private thought'), 'the saved answer exposed private reasoning');
    const editButton = finalProse.querySelector('button');
    editButton.click();
    check(firstProse.querySelector('textarea').value === saved.body && finalProse.hidden,
      'editing should open the complete stored message');
    firstProse.querySelector('.answer-edit-acts button:last-child').click();
    check(!finalProse.hidden && firstProse.querySelector('.answer-words').textContent === firstWords,
      'closing the editor should restore both message fragments');

    look(state(start.concat(said(42, 'Assistant', 'No tools were needed.')), CHAT));
    check(cards(box).length === 0 && box.querySelector('.answer-words').textContent === 'No tools were needed.',
      'plain assistant text should not create an empty card');

    for (const previous of [said(51, 'Tool', 'Search result'), said(51, 'Assistant', 'Checking sources.',
      { work: SHORT, work_head: 'Searching the web' })]) {
      look(state([said(50, 'You', 'Research and report.'), previous, said(52, 'Summary', '')], CHAT,
        { generating: true, summarising: true, live: 'secret handoff text', reasoning: 'secret thought',
          live_work: SHORT, live_work_head: 'Searching the web' }));
      check(cards(box).length === 1 && title(cards(box)[0]).includes('Searching the web'),
        'handoff should retain the card above the summary');
      check(!box.textContent.includes('secret'), 'handoff exposed the summary stream');
      if (previous.role === 'Assistant') {
        check(box.querySelector('.answer-words').textContent === previous.body,
          'handoff hid the previous progress message');
      }
    }

    const edit = call('edit_file', 'edit', 'Edited file parser.rs', { adds: 3, cuts: 1 });
    const image = call('look_at_picture', 'image', 'Viewed image', { adds: 8, cuts: 3 });
    look(state([said(30, 'You', 'Fix the parser.'), said('edit', 'Tool', '', { activity: edit }),
      said('image', 'Tool', '', { activity: image }), said(31, 'Assistant', 'Done.', {
        work: [heading('Earlier work', [line('Apply the fix.', [edit, image])])],
        work_head: 'Edited 1 file, Viewed image' })], CHAT));
    check(box.querySelectorAll('.wplus').length === 1 && box.querySelector('.wplus').textContent === '+3'
      && box.querySelector('.wminus').textContent === '-1', 'only the edit should carry line counts');
    check(box.querySelectorAll('.wtoolmark').length === 2, 'legacy work duplicated executed tools');

    look(state(talk, PLAIN));
    check(cards(box).length === 0 && box.querySelector('.think'), 'plain mode lost its thinking panel');
    look(state(talk, CHAT));
    check(cards(box).length === 3 && !box.querySelector('.think'), 'switching back lost corporate cards');
  });
  await browser.close();
  console.log('Corporate answers, legacy cards, handoff, mode switching and line counts passed');
})().catch(function (e) { console.error(e); process.exit(1); });
