// Check where the blocks of a call being written stand while an answer comes in.
//
// A call that came out of a thought reads as one part of that thought, and stands
// inside the thinking panel. But a call asked for after the model has said a word
// of the answer itself did not come out of the thought above it, and the block of
// it was put into the panel that closed before those words - which read as the
// tool having been called before the answer was written. The block now stands
// under the words of the answer from the moment the model has spoken. A call that
// ends a step is run then and there, so a block of a call is never left standing
// to be moved about after the words of the answer have come: the row is made from
// the messages instead, where the run of the tool stands under the thought that
// led to it or below the panel, as the talk reads. The page is loaded as it
// stands, with no bridge and no engine: the thread is driven by hand with the
// states the engine hands out.
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

    function call(text, done) {
      return [{ name: 'run_terminal', command: text || 'uname -a', purpose: '', target: '',
        done: !!done }];
    }

    // The one block of a call being written, and whether it stands in a thinking
    // panel.
    function block(box) {
      var made = box.querySelectorAll('.tool.write');
      if (made.length !== 1) {
        faults.push('there are ' + made.length + ' blocks of a call being written ' + where);
        return null;
      }
      return made[0];
    }
    function inPanel(node) { return !!node && !!node.closest('.think'); }

    var chat = { collapse_thinking: false, collapse_tools: true, tools_in_thinking: true };
    var head = [said('m1', 'You', 'what kernel is this?')];

    // One: the thought leads to the call, and nothing of the answer has been said.
    // The block stands inside the panel of the thought.
    where = 'a call out of a thought, with no words of the answer';
    var one = { messages: head, generating: true, summarising: false, live: '',
      reasoning: 'I must read the kernel.', reasoning_ms: 800, reasoning_done: false,
      chatting: chat, live_tools: [] };
    look(one);
    var two = JSON.parse(JSON.stringify(one));
    two.live_tools = call('uname -a');
    var box = look(two);
    var node = block(box);
    if (node && !inPanel(node)) {
      faults.push('a call that came out of a thought stands outside the thinking panel ' + where);
    }

    // Two: the thought, then words of the answer, then the call. The block must
    // stand under the words, and not in the panel that closed before them.
    where = 'a call after the model wrote words outside the thinking block';
    var three = { messages: head, generating: true, summarising: false, live: '',
      reasoning: 'I must read the kernel.', reasoning_ms: 800, reasoning_done: false,
      chatting: chat, live_tools: [] };
    look(three);
    var four = JSON.parse(JSON.stringify(three));
    four.live = 'I will read it now.';
    four.reasoning_done = true;
    look(four);
    var five = JSON.parse(JSON.stringify(four));
    five.live_tools = call('uname -a');
    box = look(five);
    node = block(box);
    if (node && inPanel(node)) {
      faults.push('a call asked for after the answer was written stands in the thinking '
        + 'panel that closed before the words ' + where);
    }
    // And it stands after the words of the answer, where a reader looks for it.
    var words = box.querySelector('.turn:last-child .plain');
    if (node && words && !(node.compareDocumentPosition(words) & Node.DOCUMENT_POSITION_PRECEDING)) {
      faults.push('the block of the call stands before the words of the answer ' + where);
    }

    // Three: no thoughts at all, so no panel, and the block stands under the words.
    where = 'a call with no thought before it';
    var nine = { messages: head, generating: true, summarising: false, live: '',
      reasoning: '', reasoning_ms: 0, chatting: chat, live_tools: [] };
    look(nine);
    var ten = JSON.parse(JSON.stringify(nine));
    ten.live_tools = call('uname -a');
    box = look(ten);
    node = block(box);
    if (node && inPanel(node)) {
      faults.push('a call with no thought before it stands in a thinking panel ' + where);
    }

    // Four: the setting off puts the block under the panel whatever the thought.
    where = 'the setting for the runs inside the thinking panel, off';
    var eleven = { messages: head, generating: true, summarising: false, live: '',
      reasoning: 'I must read the kernel.', reasoning_ms: 800,
      chatting: { collapse_thinking: false, collapse_tools: true, tools_in_thinking: false },
      live_tools: [] };
    look(eleven);
    var twelve = JSON.parse(JSON.stringify(eleven));
    twelve.live_tools = call('uname -a');
    box = look(twelve);
    node = block(box);
    if (node && inPanel(node)) {
      faults.push('with the setting off the block stands inside the panel ' + where);
    }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('a call out of a thought stands inside the thinking panel; a call asked for '
    + 'after the model wrote words of its answer stands under those words and not in the '
    + 'panel before them; and with no thought before a call, or the setting off, the '
    + 'block stands under the words');
})().catch(function (e) { console.error(e); process.exit(1); });
