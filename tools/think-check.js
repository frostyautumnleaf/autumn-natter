// Check the thinking panels of a turn that asks for a tool. The page is loaded
// as it stands, with no bridge and no engine: the thread is driven by hand with
// the states the engine hands out while a model thinks, asks for a tool, and
// then thinks again. A tool block that stands inside a thinking panel must be to
// be seen while the panel is open. A train of thought that runs over a run of
// tools must stand in one panel, and not in a row of them. More than one tool
// call must fit inside one thinking block. A panel that the user folded must
// put its whole body away, the runs among the thoughts as well. And with the
// Chatting setting off the old shape must stand, so nothing is taken away from a
// user who does not want it.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

// The page to drive: the window page, or the page of the remote mode with
//   AN_PAGE=../web/page.html AN_FETCH=1 node tools/think-check.js
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

    // A block the user can see: it has a box, and no part of the page above
    // it stands with display none. A tool block folded away inside a thinking
    // panel is the fault this check was written for.
    function shown(node) {
      if (!node) { return false; }
      for (var n = node; n && n.nodeType === 1; n = n.parentNode) {
        if (window.getComputedStyle(n).display === 'none') { return false; }
      }
      return node.getClientRects().length > 0;
    }

    // The thread is drawn as the page draws it, and the writer is brought up to
    // the words the engine has given, so the words can be read back at once.
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

    var said = function (id, role, body, extra) {
      var m = { id: id, role: role, body: body, when: '10:0', tool: '', mono: false,
        failed: false, reasoning: '', reasoning_ms: 0 };
      for (var k in extra) { m[k] = extra[k]; }
      return m;
    };

    var head = [
      said('m1', 'You', 'look at the box and tell me what you find'),
      said('m2', 'Assistant', 'I will look.', { reasoning: 'The user wants a look.',
        reasoning_ms: 1200 })
    ];

    // One: a call being written while the model thinks. The block must stand on
    // the page, inside the panel of the thought that led to it. The panel stands
    // open for this look; a folded panel is looked at last.
    where = 'a call being written under the thoughts';
    var one = { messages: head, generating: true, summarising: false, live: '',
      reasoning: 'I must run a command first.', reasoning_ms: 900, reasoning_done: false,
      chatting: { collapse_thinking: false, tools_in_thinking: true },
      live_tools: [{ name: 'run_terminal', command: 'uname -a', purpose: '', target: '',
        done: false }] };
    var box = look(one);
    var panels = box.querySelectorAll('.think');
    var writes = box.querySelectorAll('.tool.write');
    if (panels.length !== 2) { faults.push('the live answer brought ' + panels.length + ' panels, ' + where); }
    if (writes.length !== 1) { faults.push('the call being written is there ' + writes.length + ' times, ' + where); }
    if (writes.length === 1 && !shown(writes[0])) { faults.push('the call being written cannot be seen, ' + where); }
    if (writes.length === 1 && !writes[0].closest('.think')) {
      faults.push('the call being written does not stand inside the thinking panel, ' + where);
    }

    // Two: the tool ran, and the model thinks again. The thought of this step
    // goes on in the panel of the thought that led to the run.
    where = 'the thought after a run';
    var two = { messages: head.concat([said('m3', 'Tool', 'uname -a', {
        tool: 'Linux natter 6.9', tool_name: 'run_terminal',
        reasoning: 'I must run a command first.', reasoning_ms: 900 })]),
      generating: true, summarising: false, live: '',
      reasoning: 'It printed the kernel. Now the notes.', reasoning_ms: 700,
      reasoning_done: false, live_tools: [] };
    box = look(two);
    var turns = box.querySelectorAll('.turn');
    var group = turns[turns.length - 1];
    var groupPanels = group.querySelectorAll('.think');
    if (groupPanels.length !== 1) {
      faults.push('the turn after a run holds ' + groupPanels.length + ' thinking panels, ' + where);
    }
    var thoughts = group.querySelectorAll('.thought');
    if (thoughts.length < 2) {
      faults.push('the panel holds ' + thoughts.length + ' thoughts and their run, ' + where);
    }
    if (group.querySelectorAll('.tool').length !== 1) {
      faults.push('the run of the tool is not in the panel of the thought that led to it, ' + where);
    }
    if (group.textContent.indexOf('Now the notes') < 0) {
      faults.push('the thought after the run is not on the page, ' + where);
    }
    if (box.querySelectorAll('.think').length !== 2) {
      faults.push('the chat has ' + box.querySelectorAll('.think').length + ' panels, ' + where);
    }

    // Three: two calls inside one thought. The second has no thinking of its
    // own, for the model asked for both in one breath.
    where = 'two calls in one thought';
    var three = { messages: two.messages.concat([said('m4', 'Tool', 'ls notes', {
        tool: 'cat.md', tool_name: 'run_terminal' })]),
      generating: true, summarising: false, live: '',
      reasoning: 'Both are read. I can answer now.', reasoning_ms: 400,
      reasoning_done: false, live_tools: [] };
    box = look(three);
    turns = box.querySelectorAll('.turn');
    group = turns[turns.length - 1];
    if (group.querySelectorAll('.think').length !== 1) {
      faults.push('two calls in one thought made ' + group.querySelectorAll('.think').length + ' panels, ' + where);
    }
    if (group.querySelectorAll('.tool').length !== 2) {
      faults.push('the panel holds ' + group.querySelectorAll('.tool').length + ' runs, ' + where);
    }

    // Four: the answer comes out of the thinking. It stands under the panel, in
    // the same turn, and the panel is still one.
    where = 'the answer at the end of the thought';
    var four = { messages: three.messages, generating: true, summarising: false,
      live: 'The kernel is 6.9 and the notes hold one file.', reasoning: 'Both are read. I can answer now.',
      reasoning_ms: 400, reasoning_done: true, live_tools: [] };
    box = look(four);
    turns = box.querySelectorAll('.turn');
    group = turns[turns.length - 1];
    if (group.querySelectorAll('.think').length !== 1) {
      faults.push('the answer left the thinking in ' + group.querySelectorAll('.think').length + ' panels, ' + where);
    }
    if (group.textContent.indexOf('The kernel is 6.9') < 0) {
      faults.push('the words of the answer are not on the page, ' + where);
    }
    var plains = group.querySelectorAll('.plain');
    var words = plains[plains.length - 1];
    if (!words || words.textContent.indexOf('The kernel is 6.9') < 0) {
      faults.push('the words of the answer do not stand under the panel, ' + where);
    }
    if (words && words.querySelector('.tool')) {
      faults.push('a run of a tool stands among the words of the answer, ' + where);
    }

    // Five: the answer is over. The same train of thought, read again from the
    // chat, must still stand in one panel.
    where = 'the same talk read back from the chat';
    var five = { messages: three.messages.concat([said('m5', 'Assistant',
        'The kernel is 6.9 and the notes hold one file.', {
        reasoning: 'Both are read. I can answer now.', reasoning_ms: 400 })]),
      generating: false, summarising: false, live: '', reasoning: '', reasoning_ms: 0,
      live_tools: [] };
    box = look(five);
    turns = box.querySelectorAll('.turn');
    if (turns.length !== 3) {
      faults.push('the talk of five messages made ' + turns.length + ' turns, ' + where);
    } else {
      var last = turns[2];
      if (last.querySelectorAll('.think').length !== 1) {
        faults.push('the talk read back holds ' + last.querySelectorAll('.think').length + ' panels, ' + where);
      }
      if (last.querySelectorAll('.tool').length !== 2) {
        faults.push('the talk read back holds ' + last.querySelectorAll('.tool').length + ' runs, ' + where);
      }
      if (last.textContent.indexOf('The kernel is 6.9') < 0) {
        faults.push('the answer of the talk read back is gone, ' + where);
      }
    }

    // Six: the setting off. The run stands under the panel again, and each
    // thought keeps a panel of its own.
    where = 'the setting off';
    var six = JSON.parse(JSON.stringify(five));
    six.chatting = { collapse_thinking: false, tools_in_thinking: false };
    box = look(six);
    turns = box.querySelectorAll('.turn');
    var lastOf = turns[turns.length - 1];
    if (lastOf.querySelectorAll('.think').length !== 1) {
      faults.push('with the setting off the last turn holds ' + lastOf.querySelectorAll('.think').length + ' panels, ' + where);
    }
    var allTools = box.querySelectorAll('.think .tool');
    if (allTools.length !== 0) {
      faults.push('with the setting off ' + allTools.length + ' runs still stand inside a thinking panel, ' + where);
    }
    // Seven: the fold. A panel the user closed puts the whole of its body
    // away: the thoughts, and the blocks of the runs that stand among them. A
    // run that stayed to be seen under a closed head read as a fault of the
    // fold. The head is all that is left, and it opens the panel again.
    where = 'a folded thinking panel';
    var seven = JSON.parse(JSON.stringify(two));
    seven.chatting = { collapse_thinking: true, tools_in_thinking: true };
    box = look(seven);
    turns = box.querySelectorAll('.turn');
    var folded = turns[turns.length - 1];
    var foldedTool = folded.querySelector('.tool');
    if (shown(foldedTool)) { faults.push('a run inside a folded thinking panel is to be seen, ' + where); }
    var foldedThought = folded.querySelector('.thought');
    if (shown(foldedThought)) { faults.push('the thoughts of a folded panel stand open, ' + where); }
    var foldedHead = folded.querySelector('.think > .thead');
    if (!shown(foldedHead)) { faults.push('the head of a folded panel cannot be seen, ' + where); }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.log(out.join('\n'));
    process.exit(1);
  }
  console.log('a train of thought stands in one panel, the runs inside it are to'
    + ' be seen while the panel is open and are put away when it is folded, and the'
    + ' setting off brings the old shape back');
})().catch((e) => { console.error(e); process.exit(1); });
