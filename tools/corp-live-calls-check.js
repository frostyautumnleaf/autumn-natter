// Verify that streamed Corporate tool calls appear as sub-blocks, while a cat command stays
// hidden until the tool starts and the main heading does not follow streamed arguments.
//
// Run with AN_PAGE_URL pointing at frontend/index.html, for example through screenshot.sh's
// local HTTP server.
const puppeteer = require('puppeteer');

(async () => {
  const executable = process.env.AN_SHELL_BIN || process.env.AN_CHROME || '';
  const browser = await puppeteer.launch({
    headless: executable ? true : 'shell',
    executablePath: executable || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  try {
    const page = await browser.newPage();
    await page.goto(process.env.AN_PAGE_URL, { waitUntil: 'networkidle0' });
    const result = await page.evaluate(() => {
      var state = {
        messages: [{ id: 'user-1', role: 'You', body: 'Read and update these files.',
          when: '', attachments: [] }],
        live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
        live_tools: [
          { call_id: 'tool-read', index: 0, name: 'run_terminal',
            command: 'cat src/main.rs', target: '', done: false },
          { call_id: 'tool-edit', index: 1, name: 'edit_file',
            command: '', target: 'src/parser.rs', done: false },
          { call_id: 'tool-create', index: 2, name: 'create_file',
            command: '', target: 'notes/summary.md', done: false }
        ],
        live_work: [], live_work_head: 'Thinking',
        generating: true, summarising: false,
        chat: { id: 'synthetic', title: 'Live tool blocks' },
        chatting: { collapse_thinking: true, grow_thinking: true,
          tools_in_thinking: true, corporate: true }
      };
      readChatting(state);
      function readCard() {
        return el.col.querySelector('.turn:last-child > .work');
      }
      drawThread(state);
      var card = readCard();
      if (!card) { return { error: 'no live progress card was rendered' }; }
      var header = card.querySelector(':scope > .whead');
      var direct = Array.prototype.slice.call(
        card.querySelectorAll(':scope > .wbody > .wblock'));
      var partial = {
        main: header ? header.textContent : '',
        headings: direct.map(function (block) {
          return block.querySelector(':scope > .wbhead').textContent;
        }),
        rowsCollapsed: direct.every(function (block) {
          return block.classList.contains('shut');
        }),
        headerOrder: Array.prototype.map.call(header.children, function (child) {
          return child.className;
        })
      };

      // Once the call has left the stream and its tool work begins, the completed read row is
      // added to the work tree. The main title stays on the answer phase.
      state.live_tools[0].done = true;
      state.live_work = [
        { head: 'Reading main.rs', tool: 'run_terminal', id: 'read-message',
          call_id: 'tool-read', command: 'cat src/main.rs', file: '',
          reading: true, done: false, sub: [], lines: [] }
      ];
      state.live_work_head = 'Reading main.rs';
      drawThread(state);
      card = readCard();
      direct = Array.prototype.slice.call(
        card.querySelectorAll(':scope > .wbody > .wblock'));
      var headings = direct.map(function (block) {
        return block.querySelector(':scope > .wbhead').textContent;
      });
      var readingPath = direct[0] && direct[0].querySelector('.wtoolmark path');
      partial.finalizedMain = card.querySelector(':scope > .whead').textContent;
      partial.finalizedHeadings = headings;
      partial.readingIcon = readingPath ? readingPath.getAttribute('d') : '';

      // The main title remains on the finalized read when the stream item is replaced by work.
      state.live_tools = [];
      drawThread(state);
      var next = el.col.querySelector('.turn:last-child > .work > .whead');
      partial.nextHeading = next ? next.textContent : '';
      return partial;
    });

    if (result.error) { throw new Error(result.error); }
    const problems = [];
    if (!result.main.includes('Thinking')
        || result.headings.some(function (head) { return head.includes('Reading'); })) {
      problems.push('main title or a read row changed while path arguments were streaming');
    }
    if (result.headings.length !== 2
        || !result.headings[0].includes('Editing file src/parser.rs')
        || !result.headings[1].includes('Creating file notes/summary.md')) {
      problems.push('partial cat command was shown, or other live tool blocks are missing');
    }
    if (!result.rowsCollapsed) { problems.push('live tool sub-blocks are not collapsed initially'); }
    if (!result.finalizedMain.includes('Reading main.rs')) {
      problems.push('main title did not move to the read after the call was finalized');
    }
    if (result.finalizedHeadings.length !== 3
        || !result.finalizedHeadings[0].includes('Reading main.rs')
        || !result.finalizedHeadings[1].includes('Editing file src/parser.rs')
        || !result.finalizedHeadings[2].includes('Creating file notes/summary.md')) {
      problems.push('the finalized read/edit/create sub-blocks are missing or out of order');
    }
    if (!result.readingIcon || !result.readingIcon.includes('M2 3h4.2')) {
      problems.push('the read sub-block does not use the book icon');
    }
    if (result.headerOrder.indexOf('dots') < 0
        || result.headerOrder.indexOf('wfold') < 0
        || result.headerOrder.indexOf('dots') > result.headerOrder.indexOf('wfold')) {
      problems.push('the main card dots are not before the dropdown arrow');
    }
    if (!result.nextHeading.includes('Reading main.rs')) {
      problems.push('the main heading changed when live arguments were replaced by work');
    }
    if (problems.length) { throw new Error(problems.join('; ') + '\n' + JSON.stringify(result)); }
    console.log('partial cat calls stay hidden, finalized reads render in order with the book icon, '
      + 'and the main title stays stable; header dots precede the fold arrow');
  } finally {
    await browser.close();
  }
})().catch(function (error) {
  console.error(error);
  process.exit(1);
});
