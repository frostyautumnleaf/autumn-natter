// Exercise per-turn work blocks, cancellation colours and the Corporate response marker.
// Set AN_CHROME to the local Chromium executable and AN_SHOT_DIR to keep review images.
'use strict';

const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const root = path.resolve(__dirname, '..');
const shotDir = process.env.AN_SHOT_DIR || '/tmp/autumn-natter-working';
fs.mkdirSync(shotDir, { recursive: true });

(async () => {
  const browser = await puppeteer.launch({
    headless: true,
    executablePath: process.env.AN_CHROME || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  try {
    const page = await browser.newPage();
    await page.setViewport({ width: 1360, height: 900, deviceScaleFactor: 1 });
    const url = process.env.AN_PAGE_URL || 'http://127.0.0.1:18766/frontend/index.html';
    await page.goto(url,
      { waitUntil: 'domcontentloaded' });
    await page.evaluate(() => {
      window.workMessage = function (id, role, body, extra) {
        return Object.assign({ id, role, body, when: '12:47', tool: '', tool_name: '',
          reasoning: '', reasoning_ms: 0, thought_offsets: [], attachments: [], work: [],
          work_head: '', mono: false, failed: false, cancelled: false }, extra || {});
      };
      window.workState = function (messages, corporate, extra) {
        return Object.assign({ messages, live: '', reasoning: '', reasoning_ms: 0,
          reasoning_done: false, thought_offsets: [], live_tools: [], live_work: [],
          live_work_head: '', tool_previews: [], generating: false, running: null,
          summarising: false, handoffs: [], chat: { id: 'visual-check', title: 'Work blocks' },
          chatting: { corporate, collapse_thinking: true, grow_thinking: true,
            tools_in_thinking: true, collapse_tools: true, highlight: true } }, extra || {});
      };
      window.drawWorkState = function (state) {
        document.body.className = 'dark';
        readChatting(state);
        drawThread(state);
      };
      window.workCheck = function (ok, message) {
        if (!ok) { throw new Error(message); }
      };
    });

    await page.evaluate(() => {
      const stopped = workMessage('cancelled-tool', 'Tool', 'echo "work"', {
        reasoning: 'I should inspect the project before answering.\nLet me check the working directory.',
        reasoning_ms: 5100, tool_name: 'run_terminal', failed: true, cancelled: true,
        tool: 'The call was cancelled: you stopped the answer while the call was being written, '
          + 'so the writing stopped and the tool never ran'
      });
      const afterStop = workMessage('after-stop', 'Assistant', 'I will continue without running that call.');
      drawWorkState(workState([
        workMessage('cancel-user', 'You', 'Check the working directory.'), stopped, afterStop
      ], false));
      const thought = el.col.querySelector('.work-turn .turn.bad.t-tool .plain');
      const toolResult = el.col.querySelector('.work-turn .tool.bad .out');
      const probe = document.createElement('span');
      probe.style.color = 'var(--ink-fail)';
      document.body.appendChild(probe);
      const failColor = getComputedStyle(probe).color;
      probe.remove();
      workCheck(thought && getComputedStyle(thought).color !== failColor,
        'the cancelled tool warning leaked into the thinking text');
      workCheck(toolResult && getComputedStyle(toolResult).color === failColor,
        'the cancelled tool result lost its warning color');
      const main = el.col.querySelector('.work-turn');
      workCheck(!main.textContent.includes(afterStop.body)
        && main.nextElementSibling.textContent.includes(afterStop.body),
      'the final response remained inside the work block');
      workCheck(main.classList.contains('shut'), 'the work block should start collapsed');
      main.querySelector('.work-turn-head').click();
      workCheck(!main.classList.contains('shut'), 'the work block did not open on click');
      for (const panel of el.col.querySelectorAll('.work-turn .think.closed')) {
        panel.querySelector('.thead').click();
      }
      for (const tool of el.col.querySelectorAll('.work-turn .tool.closed')) {
        tool.querySelector('.thead').click();
      }
    });
    await page.screenshot({ path: path.join(shotDir, 'plain-cancel.png'), fullPage: true });

    await page.evaluate(() => {
      const intermediate = workMessage('intermediate', 'Assistant', 'I will inspect the file first.', {
        reasoning: 'Check the existing contents.', reasoning_ms: 1700
      });
      const activity = { id: 'search-result', head: 'Searched the web for rate limits',
        tool: 'web_search', done: true, failed: false, lines: [], output: 'The rate limit is 20.' };
      const searched = workMessage('search-result', 'Tool', 'Search results', {
        tool_name: 'web_search', tool: 'Search results', activity
      });
      const final = workMessage('final-answer', 'Assistant', 'The rate limit is 20 requests per minute.', {
        reasoning: 'The result gives the limit. State it clearly.', reasoning_ms: 900,
        work: [activity], work_head: 'Searched the web'
      });
      drawWorkState(workState([
        workMessage('turn-user', 'You', 'What is the rate limit?'), intermediate, searched, final
      ], false));
      const main = el.col.querySelector('.work-turn');
      const finalRow = main.nextElementSibling;
      workCheck(/^Worked for \d+ (?:second|seconds|minute|minutes)(?:, \d+ seconds?)?$/.test(
        main.querySelector('.work-turn-state').textContent),
      'a completed turn is not labelled Worked for its duration');
      workCheck(main.textContent.includes(intermediate.body) && main.textContent.includes(searched.tool),
        'the work block omitted an intermediate message or tool result');
      workCheck(!main.textContent.includes(final.body) && finalRow.textContent.includes(final.body),
        'the final answer is not outside the work block');
      workCheck(main.querySelector('.think') && main.querySelector('.tool'),
        'thinking and tool blocks were not retained inside the work block');
      main.querySelector('.work-turn-head').click();
    });
    await page.screenshot({ path: path.join(shotDir, 'plain-worked.png'), fullPage: true });

    await page.evaluate(() => {
      const activity = { id: 'live-search-result', head: 'Searched the web for rate limits',
        tool: 'web_search', done: true, failed: false, lines: [], output: 'The rate limit is 20.' };
      const liveTool = workMessage('live-search-result', 'Tool', 'Search results', {
        tool_name: 'web_search', tool: 'Search results', activity
      });
      drawWorkState(workState([
        workMessage('corp-user', 'You', 'Find and explain the rate limit.'), liveTool
      ], true, { generating: true, live: '', reasoning: '', live_tools: [] }));
      const card = el.col.querySelector('.work-turn .work');
      workCheck(/^Working for \d+ (?:second|seconds|minute|minutes)(?:, \d+ seconds?)?$/.test(
        el.col.querySelector('.work-turn-state').textContent),
      'an active turn is not labelled Working for its duration');
      const main = el.col.querySelector('.work-turn');
      const header = main && main.querySelector('.work-turn-head');
      workCheck(main.classList.contains('processing'),
        'the active turn did not show its response-processing icon');
      workCheck(getComputedStyle(header.querySelector('.brain-paper')).display !== 'none',
        'the animated brain and paper icon is hidden');
      workCheck(getComputedStyle(header.querySelector('.work-turn-dots')).display === 'none',
        'the three dots remained beside the processing icon');
      workCheck(main.classList.contains('shut'), 'a new work block should start collapsed');
      header.click();
    });
    await new Promise(resolve => setTimeout(resolve, 1200));
    await page.evaluate(() => {
      workCheck(/^Working for [1-9]\d* (?:second|seconds|minute|minutes)/.test(
        el.col.querySelector('.work-turn-state').textContent),
      'the active duration did not advance while the model was processing a tool result');
    });
    await page.screenshot({ path: path.join(shotDir, 'corporate-processing.png'), fullPage: true });

    await page.evaluate(() => {
      const activity = { id: 'corp-search', head: 'Searched the web for rate limits',
        tool: 'web_search', done: true, failed: false, lines: [], output: 'The rate limit is 20.' };
      const intermediate = workMessage('corp-progress', 'Assistant', 'I found the current limit.', {
        reasoning: 'Read the result.', reasoning_ms: 1600
      });
      const searched = workMessage('corp-search', 'Tool', 'Search results', {
        tool_name: 'web_search', tool: 'Search results', activity
      });
      const final = workMessage('corp-final', 'Assistant', 'The limit is 20 requests per minute.');
      drawWorkState(workState([
        workMessage('corp-work-user', 'You', 'Find the rate limit.'), intermediate, searched, final
      ], true));
      const main = el.col.querySelector('.work-turn');
      const finalRow = main.nextElementSibling;
      workCheck(main.querySelector('.work-turn-state').textContent.startsWith('Worked for '),
        'a completed Corporate turn is not labelled Worked for its duration');
      workCheck(main.textContent.includes(intermediate.body) && main.querySelector('.work'),
        'the Corporate block omitted intermediate prose or its tool activity');
      workCheck(!main.textContent.includes(final.body) && finalRow.textContent.includes(final.body),
        'the Corporate final answer remained inside the work block');
      workCheck(main.classList.contains('shut'), 'a completed Corporate block should start collapsed');
      main.querySelector('.work-turn-head').click();
    });
    await page.screenshot({ path: path.join(shotDir, 'corporate-worked.png'), fullPage: true });

    await page.evaluate(() => {
      const tool = workMessage('plain-result', 'Tool', 'Search results', {
        activity: { id: 'plain-result', head: 'Searched the web', tool: 'web_search',
          done: true, failed: false, lines: [], output: 'Result' }
      });
      drawWorkState(workState([
        workMessage('plain-result-user', 'You', 'Search in plain mode.'), tool
      ], false, { generating: true, live: '' }));
      const main = el.col.querySelector('.work-turn');
      workCheck(!main.classList.contains('processing')
        && getComputedStyle(main.querySelector('.brain-paper')).display === 'none',
      'the Corporate processing icon appeared in plain mode');
    });

    await page.evaluate(() => {
      const active = workState([
        workMessage('active-user', 'You', 'Check and report.'),
        workMessage('active-tool', 'Tool', 'cargo test', {
          tool_name: 'run_terminal', tool: '3 tests passed'
        })
      ], false, { generating: true, live: 'I am checking the result now.',
        reasoning: 'The tests passed.', reasoning_ms: 1200, thought_offsets: [0] });
      drawWorkState(active);
      window.activeWorkState = active;
      window.heldWorkNode = el.col.querySelector('.work-turn');
      workCheck(heldWorkNode.classList.contains('shut'), 'the active work block did not start collapsed');
      heldWorkNode.querySelector('.work-turn-head').click();
      workCheck(!heldWorkNode.classList.contains('shut'), 'the work block did not open on click');
      active.live = 'The result is clear; I will summarize it.';
      drawWorkState(active);
      workCheck(el.col.querySelector('.work-turn') === heldWorkNode
        && !heldWorkNode.classList.contains('shut'),
      'a streamed update replaced the work block or reset its fold');
      active.generating = false;
      active.messages.push(workMessage('active-final', 'Assistant', 'All three tests passed.'));
      active.live = '';
      active.reasoning = '';
      drawWorkState(active);
      workCheck(el.col.querySelector('.work-turn') === heldWorkNode
        && !heldWorkNode.classList.contains('shut'),
      'finishing the turn reset the user’s fold choice');
      workCheck(heldWorkNode.querySelector('.work-turn-state').textContent.startsWith('Worked for '),
        'the active block did not change to Worked for its duration');
      workCheck(!heldWorkNode.textContent.includes('All three tests passed.')
        && heldWorkNode.nextElementSibling.textContent.includes('All three tests passed.'),
      'the final message did not move outside the work block');
      const turns = [workMessage('u1', 'You', 'First question.'),
        workMessage('a1', 'Assistant', 'An intermediate update.'),
        workMessage('f1', 'Assistant', 'First final answer.'),
        workMessage('u2', 'You', 'Second question.'),
        workMessage('f2', 'Assistant', 'Second final answer.')];
      drawWorkState(workState(turns, false));
      workCheck(el.col.querySelectorAll(':scope > .work-turn').length === 2,
        'each user turn must have its own Working/Worked block');
      workCheck(Array.from(el.col.querySelectorAll(':scope > .work-turn')).every(node =>
        node.querySelector('.work-turn-state').textContent.startsWith('Worked for ')),
      'one completed turn did not retain its own Worked block');
    });
    await page.screenshot({ path: path.join(shotDir, 'plain-two-turns.png'), fullPage: true });
    console.log(path.join(shotDir, 'plain-cancel.png'));
    console.log(path.join(shotDir, 'plain-worked.png'));
    console.log(path.join(shotDir, 'corporate-processing.png'));
    console.log(path.join(shotDir, 'corporate-worked.png'));
    console.log(path.join(shotDir, 'plain-two-turns.png'));
    console.log('Working blocks, cancellation colour, and Corporate response animation passed');
  } finally {
    await browser.close();
  }
})().catch(error => {
  console.error(error);
  process.exit(1);
});
