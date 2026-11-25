// Verify that streamed Corporate tool calls appear as sub-blocks, while a cat command stays
// hidden until the tool starts and an active card follows its latest tool.
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
    await page.evaluate(() => {
      function check(condition, message) {
        if (!condition) { throw new Error(message); }
      }
      function work(id, callId, head, command, extra) {
        return Object.assign({ id: id, call_id: callId, head: head,
          tool: 'run_terminal', command: command }, extra);
      }
      function message(id, role, body, extra) {
        return Object.assign({ id: id, role: role, body: body }, extra);
      }
      var preview = work('stream:check', 'check', 'Running wc -c test.txt', 'wc -c test.txt');
      var reading = work('read-result', 'read', 'Read test.txt', 'cat test.txt',
        { reading: true, done: true, output: 'shop' });
      var narration = message('progress', 'Assistant', 'I read **shop**. Now I will check the file.');
      var state = {
        messages: [message('user-1', 'You', 'Read and update the file.'),
          message('read-result', 'Tool', 'cat test.txt', { activity: reading })],
        live: narration.body,
        // This is the raw stream shape served to both pages. It also contains the cat draft,
        // which has no corresponding work record until it runs.
        live_tools: [
          { name: 'run_terminal', command: 'wc -c test.txt', done: false },
          { name: 'run_terminal', command: 'cat another/', done: false }
        ],
        tool_previews: [preview], live_work: [reading], live_work_head: 'Thinking',
        generating: true, summarising: false,
        chat: { id: 'synthetic', title: 'Corporate live tools' },
        chatting: { collapse_thinking: true, grow_thinking: true,
          tools_in_thinking: true, corporate: true }
      };
      function cards() { return Array.from(el.col.querySelectorAll('.turn > .work')); }
      function rows() { return Array.from(el.col.querySelectorAll('.work > .wbody > .wblock')); }
      function sections() {
        return Array.from(el.col.querySelectorAll('.turn > .work, .turn > .corporate-message'), node => {
          if (node.classList.contains('corporate-message')) {
            return node.querySelector('.answer-words').textContent;
          }
          return Array.from(node.querySelectorAll('.wbtitle'), title => title.textContent);
        });
      }
      function expectSections(expected) {
        var actual = sections();
        check(JSON.stringify(actual) === JSON.stringify(expected), JSON.stringify(actual));
      }
      readChatting(state);
      drawThread(state);
      var words = 'I read shop. Now I will check the file.';
      expectSections([['Read test.txt'], words, ['Running wc -c test.txt']]);
      check(cards().concat(rows()).every(node => node.classList.contains('shut')),
        'cards and rows should start closed');
      var progress = el.col.querySelector('.answer-words');
      check(progress.querySelector('strong').textContent === 'shop' && progress.getBoundingClientRect().height > 0,
        'progress markdown must be visible with both cards closed');
      cards()[1].querySelector('.whead').click();
      rows()[1].querySelector('.wbhead').click();
      var originalCard = cards()[1];
      var originalRow = rows()[1];
      var arrow = originalCard.querySelector('.whead .wfold').getBoundingClientRect();
      check(Math.abs(originalRow.getBoundingClientRect().left - (arrow.left + arrow.width / 2)) < 1,
        'the sub-block border must sit below the card arrow');
      drawThread(state);
      check(rows()[1] === originalRow, 'an unchanged snapshot replaced the tool row');

      var completed = work('check-result', 'check', 'Ran wc -c test.txt', 'wc -c test.txt',
        { done: true, output: '4 test.txt' });
      state.messages.push(narration, message('check-result', 'Tool', 'wc -c test.txt', { activity: completed }));
      state.live = '';
      state.live_work = [{ group: true, sub: [reading, completed, preview] }];
      state.tool_previews = [];
      state.live_work_head = completed.head;
      drawThread(state);
      expectSections([['Read test.txt'], words, ['Ran wc -c test.txt']]);
      check(cards()[1] === originalCard && rows()[1] === originalRow &&
        !originalCard.classList.contains('shut') && !originalRow.classList.contains('shut'),
        'the fold choice was lost while a call completed');
      check(originalRow.querySelector('.wout').textContent === '4 test.txt',
        'the tool output is missing');

      // A later call can run the exact same command. Keep both real calls.
      var repeated = work('stream:repeat', 'repeat', preview.head, preview.command);
      state.live = 'I will check again.';
      state.tool_previews = [repeated];
      drawThread(state);
      expectSections([['Read test.txt'], words, [completed.head], state.live, [repeated.head]]);
      check(rows()[1] === originalRow && !originalRow.classList.contains('shut'),
        'a new card reset the earlier tool fold');
      check(cards()[2].classList.contains('shut'), 'the new card inherited the earlier fold');

      state.messages.push(message('next-progress', 'Assistant', state.live,
        { work: [reading, completed], answer_head: 'Ran a command' }),
        message('repeat-result', 'Tool', repeated.command, { activity: Object.assign({}, completed, { id: 'repeat-result' }) }),
        message('final', 'Assistant', 'The file contains four bytes.', {
          work: [work('repeat-result', 'repeat', completed.head, completed.command,
            { done: true, output: '4 test.txt' })],
          answer_head: 'Ran a command'
        }));
      state.live_work = [];
      state.live_tools = [];
      state.generating = false;
      drawThread(state);
      expectSections([['Read test.txt'], words, [completed.head], state.live,
        [completed.head], 'The file contains four bytes.']);
      check(cards().slice(1).every(node => node.querySelector('.wtitle').textContent === 'Ran a command'),
        'each card should summarize its own completed tool calls');

      // A search keeps its heading while the model reads the result in private.
      function heading() { return cards()[0].querySelector('.wtitle').textContent; }
      state.messages = [message('search-user', 'You', 'Search for the release notes.')];
      state.live = '';
      state.generating = true;
      state.reasoning = 'Planning the search.';
      state.tool_previews = [];
      drawThread(state);
      check(heading() === 'Thinking', 'a card with only live reasoning should say Thinking');

      const search = { id: 'search', tool: 'web_search', head: 'Searching the web for release notes' };
      state.tool_previews = [search];
      drawThread(state);
      check(heading() === search.head, 'the search preview should take over the heading');

      const result = message('search', 'Tool', 'Release notes', {
        activity: { ...search, head: 'Searched the web for release notes', done: true },
        answer_head: 'Searched the web'
      });
      state.messages.push(result);
      state.tool_previews = [];
      state.reasoning = '';
      drawThread(state);
      check(heading() === result.activity.head, 'the card should wait for prose before showing the summary');

      const next = message('second-search', 'Tool', 'More results', {
        activity: { ...search, id: 'second-search', head: 'Searched the web for patch numbers', done: true },
        answer_head: 'Searched the web 2 times'
      });
      state.messages.push(next);
      state.reasoning = 'Reading the results.';
      drawThread(state);
      check(heading() === next.activity.head && cards()[0].querySelector('.whead.busy'),
        'reasoning after a search should keep the search heading and the activity dots');

      state.live = 'I found the release notes.';
      state.reasoning_done = true;
      drawThread(state);
      check(heading() === next.answer_head, 'starting a message should show the card summary');
      state.reasoning = '';
      state.reasoning_done = false;

      state.messages = [message('text-only-user', 'You', 'Say hello.')];
      state.live = '';
      state.reasoning = ' \n';
      state.reasoning_ms = 120;
      state.thought_offsets = [0];
      drawThread(state);
      check(cards().length === 0, 'waiting with only whitespace reasoning should not start a card');
      state.live = 'Hello!';
      drawThread(state);
      expectSections(['Hello!']);
      state.messages.push(message('text-only', 'Assistant', 'Hello!', {
        reasoning: state.reasoning, reasoning_ms: state.reasoning_ms, thought_offsets: [0]
      }));
      state.generating = false;
      drawThread(state);
      expectSections(['Hello!']);
      state.reasoning = '';
      state.reasoning_ms = 0;
      state.thought_offsets = [];

      // In a parallel request the engine withholds a cat path until execution.
      // A visible second preview must keep its own node and fold as the first appears.
      state.messages = [message('batch-user', 'You', 'Check both files.')];
      state.live = '';
      state.live_work = [];
      state.generating = true;
      state.tool_previews = [Object.assign({}, preview, { slot: 1 })];
      drawThread(state);
      cards()[0].querySelector('.whead').click();
      var second = rows()[0];
      second.querySelector('.wbhead').click();
      state.messages.push(message('batch-read', 'Tool', 'cat test.txt', { activity: reading }));
      state.tool_previews = [Object.assign({}, preview, { slot: 0 })];
      drawThread(state);
      expectSections([['Read test.txt', preview.head]]);
      check(rows()[1] === second && !second.classList.contains('shut') && rows()[0].classList.contains('shut'),
        'an earlier hidden read stole the visible command row');
      state.messages.push(message('batch-check', 'Tool', preview.command, { activity: completed }));
      state.tool_previews = [];
      drawThread(state);
      expectSections([['Read test.txt', completed.head]]);
      check(rows()[1] === second, 'a batched call was replaced on completion');
    });
    // Set AN_CORPORATE_FRAMES to the file written by the Rust lifecycle test.
    // Replay the persisted chat in a fresh document for the final frame.
    if (process.env.AN_CORPORATE_FRAMES) {
      const frames = JSON.parse(require('fs').readFileSync(process.env.AN_CORPORATE_FRAMES));
      const phases = ['streaming', 'running', 'finished', 'reopened'];
      await page.setViewport({ width: 1200, height: 800 });
      for (const [index, frame] of frames.entries()) {
        if (index === 3) { await page.reload({ waitUntil: 'domcontentloaded' }); }
        await page.evaluate(state => draw(state), frame);
        await page.$$eval('.work.shut > .whead', nodes => nodes.forEach(node => node.click()));
        const sections = await page.$$eval('.turn.t-ai > .work, .turn.t-ai > .corporate-message', nodes =>
          nodes.map(node => node.classList.contains('work') ?
            Array.from(node.querySelectorAll('.wbtitle'), n => n.textContent) :
            node.querySelector('.answer-words').textContent));
        const expected = ['I will read the file first.', ['Read test.txt'],
          'I will check the raw bytes.', [index < 2 ? 'Running xxd test.txt' : 'Ran xxd test.txt']];
        if (index >= 2) { expected.push('The file contains the four bytes for shop.'); }
        if (JSON.stringify(sections) !== JSON.stringify(expected)) {
          throw new Error(phases[index] + ' ' + JSON.stringify(sections));
        }
        await page.$$eval('.work .wblock.shut', nodes => nodes.forEach(node => {
          if (node.querySelector('.wcmd')) { node.querySelector('.wbhead').click(); }
        }));
        if (process.env.AN_CARD_SHOT) {
          const turn = await page.$('.turn.t-ai');
          await turn.screenshot({ path: process.env.AN_CARD_SHOT + '-' + phases[index] + '.png' });
        }
      }
    }
    console.log('Corporate activity cards keep each call once, in order, and preserve their folds');
  } finally {
    await browser.close();
  }
})().catch(function (error) {
  console.error(error);
  process.exit(1);
});
