// Verify that streamed Corporate tool calls appear as their own sub-blocks before the
// work tree catches up, and that the main heading follows the active call.
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
        live_work: [], live_work_head: 'Reading src/main.rs',
        generating: true, summarising: false,
        chat: { id: 'synthetic', title: 'Live tool blocks' },
        chatting: { collapse_thinking: true, grow_thinking: true,
          tools_in_thinking: true, corporate: true }
      };
      readChatting(state);
      drawThread(state);
      var card = el.col.querySelector('.turn:last-child > .work');
      if (!card) { return { error: 'no live progress card was rendered' }; }

      var header = card.querySelector(':scope > .whead');
      var main = header ? header.textContent : '';
      var direct = Array.prototype.slice.call(
        card.querySelectorAll(':scope > .wbody > .wblock'));
      var headings = direct.map(function (block) {
        return block.querySelector(':scope > .wbhead').textContent;
      });
      var rowsCollapsed = direct.every(function (block) {
        return block.classList.contains('shut');
      });
      var readingPath = direct[0] && direct[0].querySelector('.wtoolmark path');
      var headerOrder = Array.prototype.map.call(header.children, function (child) {
        return child.className;
      });
      var reading = { main: main, headings: headings, rowsCollapsed: rowsCollapsed,
        readingIcon: readingPath ? readingPath.getAttribute('d') : '',
        headerOrder: headerOrder };

      // A subsequent snapshot with no active tool must replace the stale action heading.
      state.live_tools = [];
      state.live_work_head = 'Thinking';
      drawThread(state);
      var next = el.col.querySelector('.turn:last-child > .work > .whead');
      reading.nextHeading = next ? next.textContent : '';
      return reading;
    });

    if (result.error) { throw new Error(result.error); }
    const problems = [];
    if (!result.main.includes('Reading src/main.rs')) {
      problems.push('main title does not name the active read');
    }
    if (result.headings.length !== 3
        || !result.headings[0].includes('Reading src/main.rs')
        || !result.headings[1].includes('Editing file src/parser.rs')
        || !result.headings[2].includes('Creating file notes/summary.md')) {
      problems.push('live read/edit/create sub-blocks are missing or out of order');
    }
    if (!result.rowsCollapsed) { problems.push('live tool sub-blocks are not collapsed initially'); }
    if (!result.readingIcon || !result.readingIcon.includes('M2 3h4.2')) {
      problems.push('the read sub-block does not use the book icon');
    }
    if (result.headerOrder.indexOf('dots') < 0
        || result.headerOrder.indexOf('wfold') < 0
        || result.headerOrder.indexOf('dots') > result.headerOrder.indexOf('wfold')) {
      problems.push('the main card dots are not before the dropdown arrow');
    }
    if (result.nextHeading.includes('Reading')) {
      problems.push('the main heading stayed on Reading after its live call disappeared');
    }
    if (problems.length) { throw new Error(problems.join('; ') + '\n' + JSON.stringify(result)); }
    console.log('live read/edit/create blocks render in order, stay collapsed, use the book icon, '
      + 'and the main title follows the active action; header dots precede the fold arrow');
  } finally {
    await browser.close();
  }
})().catch(function (error) {
  console.error(error);
  process.exit(1);
});
