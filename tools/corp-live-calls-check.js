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
      function card() {
        var cards = el.col.querySelectorAll('.turn > .work');
        check(cards.length === 1, 'expected one card, found ' + cards.length);
        return cards[0];
      }
      function rows() { return Array.from(card().querySelectorAll(':scope > .wbody > .wblock')); }
      function headings() {
        return rows().map(function (row) { return row.querySelector('.wbtitle').textContent; });
      }
      function expectHeadings(expected) {
        var actual = headings();
        check(JSON.stringify(actual) === JSON.stringify(expected), JSON.stringify(actual));
      }
      readChatting(state);
      drawThread(state);
      var words = 'I read shop. Now I will check the file.';
      expectHeadings(['Read test.txt', words, 'Running wc -c test.txt']);
      check(card().classList.contains('shut') && rows().every(function (row) {
        return row.classList.contains('shut');
      }), 'card and rows should start closed');
      check(rows()[1].querySelector('strong').textContent === 'shop', 'progress markdown was lost');
      card().querySelector('.whead').click();
      rows()[2].querySelector('.wbhead').click();
      var originalRow = rows()[2];
      var arrow = card().querySelector('.whead .wfold').getBoundingClientRect();
      check(Math.abs(rows()[0].getBoundingClientRect().left - (arrow.left + arrow.width / 2)) < 1,
        'the sub-block border must sit below the card arrow');
      drawThread(state);
      check(rows()[2] === originalRow, 'an unchanged snapshot replaced the tool row');

      var completed = work('check-result', 'check', 'Ran wc -c test.txt', 'wc -c test.txt',
        { done: true, output: '4 test.txt' });
      state.messages.push(narration, message('check-result', 'Tool', 'wc -c test.txt', { activity: completed }));
      state.live = '';
      state.live_work = [{ group: true, sub: [reading, completed, preview] }];
      state.tool_previews = [];
      state.live_work_head = completed.head;
      drawThread(state);
      expectHeadings(['Read test.txt', words, 'Ran wc -c test.txt']);
      check(!card().classList.contains('shut') && !rows()[2].classList.contains('shut'),
        'the fold choice was lost while a call completed');
      check(rows()[2].querySelector('.wout').textContent === '4 test.txt',
        'the tool output is missing');

      // A later call can run the exact same command. Keep both real calls.
      var repeated = work('stream:repeat', 'repeat', preview.head, preview.command);
      state.live = 'I will check again.';
      state.tool_previews = [repeated];
      drawThread(state);
      expectHeadings(['Read test.txt', words, 'Ran wc -c test.txt', state.live, repeated.head]);
      check(!rows()[2].classList.contains('shut'), 'a new row reset the earlier tool fold');
      check(rows()[2].dataset.foldKey === originalRow.dataset.foldKey, 'tool identity changed');

      state.messages.push(message('next-progress', 'Assistant', state.live,
        { work: [reading, completed] }),
        message('repeat-result', 'Tool', repeated.command, { activity: Object.assign({}, completed, { id: 'repeat-result' }) }),
        message('final', 'Assistant', 'The file contains four bytes.', {
          work: [work('repeat-result', 'repeat', completed.head, completed.command,
            { done: true, output: '4 test.txt' })],
          work_head: 'Read 1 file, Ran 2 commands'
        }));
      state.live_work = [];
      state.live_tools = [];
      state.generating = false;
      drawThread(state);
      expectHeadings(['Read test.txt', words, 'Ran wc -c test.txt', state.live, completed.head]);
      check(card().querySelector('.whead').textContent.includes('Ran 2 commands'),
        'the main heading did not use the completed summary');
      check(!card().textContent.includes('The file contains four bytes.')
        && el.col.textContent.includes('The file contains four bytes.'),
        'the final response must appear outside the card');

      // In a parallel request the engine withholds a cat path until execution.
      // A visible second preview must keep its own node and fold as the first appears.
      state.messages = [message('batch-user', 'You', 'Check both files.')];
      state.live = '';
      state.live_work = [];
      state.generating = true;
      state.tool_previews = [Object.assign({}, preview, { slot: 1 })];
      drawThread(state);
      card().querySelector('.whead').click();
      var second = rows()[0];
      second.querySelector('.wbhead').click();
      state.messages.push(message('batch-read', 'Tool', 'cat test.txt', { activity: reading }));
      state.tool_previews = [Object.assign({}, preview, { slot: 0 })];
      drawThread(state);
      expectHeadings(['Read test.txt', preview.head]);
      check(rows()[1] === second && !second.classList.contains('shut') && rows()[0].classList.contains('shut'),
        'an earlier hidden read stole the visible command row');
      state.messages.push(message('batch-check', 'Tool', preview.command, { activity: completed }));
      state.tool_previews = [];
      drawThread(state);
      expectHeadings(['Read test.txt', completed.head]);
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
        const card = await page.$('.work');
        if (await card.evaluate(node => node.classList.contains('shut'))) {
          await card.$eval('.whead', head => head.click());
        }
        const labels = await card.$$eval('.wbtitle', nodes => nodes.map(n => n.textContent));
        const expected = index < 2 ? 'Running xxd test.txt' : 'Ran xxd test.txt';
        if (labels.length !== 4 || labels[3] !== expected) {
          throw new Error(phases[index] + ' ' + JSON.stringify(labels));
        }
        await card.$$eval('.wblock.shut', nodes => nodes.forEach(node => {
          if (node.querySelector('.wcmd')) { node.querySelector('.wbhead').click(); }
        }));
        if (process.env.AN_CARD_SHOT) {
          const turn = await page.$('.turn.t-ai');
          await turn.screenshot({ path: process.env.AN_CARD_SHOT + '-' + phases[index] + '.png' });
        }
      }
    }
    console.log('Corporate card keeps each call once, in order, and preserves its fold');
  } finally {
    await browser.close();
  }
})().catch(function (error) {
  console.error(error);
  process.exit(1);
});
