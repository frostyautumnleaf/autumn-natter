// Check that a fold the reader moved by hand stays where the reader put it.
//
// A row of the chat is made again whenever the state of the chat moves: the
// answer that was coming in becomes a message of the chat, a run of a tool comes
// in among the thoughts, the words of an answer are written again. A fold kept
// only in the class of a panel went with the panel, and the reader who had opened
// a thinking panel saw it close itself when the model called a tool inside it, and
// a reader who had opened a block of a run saw it shut when the model finished
// writing. The folds are now read off a row before it is made again and put back
// after. The page is loaded as it stands, with no bridge and no engine.
const fs = require('fs');
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
      var m = { id: id, role: role, body: body, when: '10:0', tool: '', mono: false,
        failed: false, reasoning: '', reasoning_ms: 0 };
      for (var k in extra) { m[k] = extra[k]; }
      return m;
    }

    // The head of a panel is what the reader clicks.
    function open(node) {
      var head = node.querySelector('.thead');
      if (!head) { faults.push('no head to click ' + where); return; }
      head.click();
    }
    function closed(node) { return !!node && node.classList.contains('closed'); }

    var chat = { collapse_thinking: true, collapse_tools: true, tools_in_thinking: true };
    var head = [
      said('m1', 'You', 'look at the box and tell me what you find'),
      said('m2', 'Assistant', 'I will look.', { reasoning: 'The reader wants a look.',
        reasoning_ms: 1200 })
    ];

    // One: a thinking panel that starts closed. The reader opens it while the
    // answer is coming in; the answer then becomes a message of the chat, which
    // makes the row again. The panel must stand open still.
    where = 'a thinking panel opened over an answer that finished';
    var one = { messages: head, generating: true, summarising: false, live: '',
      reasoning: 'I must run a command first.', reasoning_ms: 900, reasoning_done: false,
      chatting: chat, live_tools: [] };
    var box = look(one);
    var panel = box.querySelector('.turn:last-child .think');
    if (!panel) { faults.push('no thinking panel came ' + where); }
    if (panel && !closed(panel)) {
      faults.push('a thinking panel stood open though the setting folds it ' + where);
    }
    if (panel) {
      open(panel);
      if (closed(panel)) { faults.push('the click did not open the panel ' + where); }
    }
    var two = { messages: head.concat([
        said('m3', 'Assistant', 'The box holds notes.', { reasoning: 'I must run a command first.',
          reasoning_ms: 900 }),
        said('m4', 'Tool', 'uname -a', { tool: 'Linux natter 6.9', tool_name: 'run_terminal' })
      ]),
      generating: false, summarising: false, live: '', reasoning: '', reasoning_ms: 0,
      chatting: chat, live_tools: [] };
    box = look(two);
    var thinks = box.querySelectorAll('.think');
    var again = thinks[thinks.length - 1];
    if (!again) { faults.push('the thinking panel went away ' + where); }
    else if (closed(again)) {
      faults.push('the thinking panel the reader opened closed itself when the answer '
        + 'became a message of the chat, ' + where);
    }

    // Two: a block of a run inside a thinking panel, folded as the setting has it.
    // The reader opens it; the model then writes on, and the part of the panel is
    // made again with the new words of the run. The block must stand open still.
    where = 'a block of a run opened inside a thinking panel';
    var three = { messages: head.concat([
        said('m5', 'Assistant', '', { reasoning: 'I must look at the notes first.',
          reasoning_ms: 800 }),
        said('m6', 'Tool', 'cat notes', { tool: 'the first line', tool_name: 'run_terminal' })
      ]),
      generating: true, summarising: false, live: 'The notes say', reasoning: '',
      reasoning_ms: 0, reasoning_done: true, chatting: chat, live_tools: [] };
    box = look(three);
    var block = box.querySelector('.think .tool');
    if (!block) { faults.push('no block of a run inside the thinking panel ' + where); }
    if (block && !closed(block)) {
      faults.push('a block of a run stood open though the setting folds it ' + where);
    }
    if (block) {
      open(block);
      if (closed(block)) { faults.push('the click did not open the block ' + where); }
    }
    var four = JSON.parse(JSON.stringify(three));
    four.messages[3].tool = 'the first line\nand a second one';
    four.live = 'The notes say more and more';
    box = look(four);
    var block2 = box.querySelector('.think .tool');
    if (!block2) { faults.push('the block of the run went away ' + where); }
    else if (closed(block2)) {
      faults.push('the block of a run the reader opened shut itself when the model '
        + 'finished writing, ' + where);
    }

    // Three: a fold the reader never moved follows the setting, whatever the
    // making of the row did. A panel of the chat that stands closed must not be
    // forced open by the folds of another row.
    where = 'a fold the reader never moved';
    var five = { messages: head.concat([
        said('m7', 'Assistant', 'And here are the words.', { reasoning: 'A thought of its own.',
          reasoning_ms: 400 })
      ]),
      generating: false, summarising: false, live: '', reasoning: '', reasoning_ms: 0,
      chatting: chat, live_tools: [] };
    box = look(five);
    var turns = box.querySelectorAll('.turn');
    var last = turns[turns.length - 1];
    var ownPanel = last.querySelector('.think');
    if (!ownPanel) { faults.push('the last message has no thinking panel ' + where); }
    else if (!closed(ownPanel)) {
      faults.push('a thinking panel the reader never touched stood open though the '
        + 'setting folds it, ' + where);
    }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('a fold the reader moved stays: a thinking panel opened over an answer '
    + 'that finished, and a block of a run opened inside a thinking panel; a fold the '
    + 'reader never moved still follows the setting');
})().catch(function (e) { console.error(e); process.exit(1); });
