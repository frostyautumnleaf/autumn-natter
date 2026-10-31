// Check that a thinking panel which holds a run of a tool is put again part by
// part. The page is loaded as it stands, with no bridge and no engine: the thread
// is driven by hand with the states the engine hands out while a model thinks,
// asks for a tool, and writes on. Three things are looked at. When the result of
// a run comes in, the panel must not be built again whole, for that threw the
// reading place of the panel away and made every thought fade a second time. The
// block of the run must leave off saying 'running' and show its result, even
// while the answer that is coming in stands after it. And the words of the answer
// must keep growing in the same panel.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

// The page to drive: the window page, or the page of the remote mode with
//   AN_PAGE=../web/page.html AN_FETCH=1 node tools/group-check.js
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

    // A thought long enough to be moved in, so the reading place of the panel
    // can be looked at. The Chatting setting that lets a panel grow is turned
    // off, for a panel that grows has no place of its own to read at.
    var long = 'The user wants the notes read. '
      + 'Every line of them has to be looked at, so the thought runs on and on.';
    var chat = { chatting: { collapse_thinking: false, grow_thinking: false,
                             tools_in_thinking: true, collapse_tools: false } };
    var head = [said('m1', 'You', 'read the notes and tell me what you find')];
    var run = function (tool) {
      return said('m2', 'Tool', 'cat notes.md', { tool: tool, tool_name: 'run_terminal',
        reasoning: long, reasoning_ms: 900 });
    };

    // One: the command is running and the model thinks again. The block says
    // 'running', and the answer that is coming in stands after it in the panel.
    where = 'the command is running';
    var one = Object.assign({ messages: head.concat([run('Running ...')]),
      generating: true, summarising: false, live: '',
      reasoning: 'The file is open. I can read it.', reasoning_ms: 400,
      reasoning_done: false, live_tools: [] }, chat);
    var box = look(one);
    var turns = box.querySelectorAll('.turn');
    var group = turns[turns.length - 1];
    var panel = group.querySelector('.think');
    var body = panel ? panel.querySelector('.words') : null;
    if (!panel) { faults.push('the run and the thought stand in no panel, ' + where); }
    if (turns.length !== 2) {
      faults.push('the talk made ' + turns.length + ' turns, ' + where);
    }
    var pill = group.querySelector('.tool .state');
    if (!pill || pill.textContent !== 'running') {
      faults.push('the block of a run that is running says ' + (pill ? pill.textContent : 'nothing') + ', ' + where);
    }
    var place = 0;
    if (body) {
      // The reading place the panel can be moved to, and moved back to. The
      // panel is not tall, so the place asked for is the place it took.
      body.scrollTop = 40;
      place = body.scrollTop;
      if (!place) { faults.push('the panel of the thought cannot be moved in, ' + where); }
      // The nodes are marked, so it can be said afterwards whether they are the
      // nodes that stood before or new ones made again.
      panel.__kept = 'panel';
      body.firstChild.__kept = 'part';
    }

    // Two: the result of the run comes in while the model writes on. The block
    // must say what became of the command, and the panel must be the panel that
    // stood before, at the place the user had moved it to.
    where = 'the result came in';
    var two = Object.assign({ messages: head.concat([run('the notes, read')]),
      generating: true, summarising: false, live: '',
      reasoning: 'It printed the notes. Now the answer.', reasoning_ms: 700,
      reasoning_done: false, live_tools: [] }, chat);
    box = look(two);
    turns = box.querySelectorAll('.turn');
    var same = turns[turns.length - 1];
    var samePanel = same.querySelector('.think');
    if (!samePanel || samePanel.__kept !== 'panel') {
      faults.push('the panel was built again whole when the result came in, ' + where);
    }
    var sameBody = samePanel ? samePanel.querySelector('.words') : null;
    if (sameBody && sameBody.scrollTop !== place) {
      faults.push('the reading place of the panel moved from ' + place + ' to '
        + sameBody.scrollTop + ', ' + where);
    }
    if (sameBody && sameBody.firstChild && sameBody.firstChild.__kept !== 'part') {
      faults.push('the thought of the panel was made again, ' + where);
    }
    var done = same.querySelector('.tool .state');
    if (!done || done.textContent !== 'done') {
      faults.push('the block of a run that is over says ' + (done ? done.textContent : 'nothing') + ', ' + where);
    }
    var out = same.querySelector('.tool .out');
    if (!out || out.textContent.indexOf('the notes, read') < 0) {
      faults.push('the result of the run is not on the page, ' + where);
    }
    if (same.textContent.indexOf('Now the answer') < 0) {
      faults.push('the thought after the run is gone, ' + where);
    }

    // Three: the answer is over and stands in the chat. The panel of the talk
    // read back is made once, and the run of the tool is of it.
    where = 'the talk read back';
    var three = Object.assign({ messages: head.concat([run('the notes, read'),
        said('m3', 'Assistant', 'The notes hold one line.')]),
      generating: false, summarising: false, live: '', reasoning: '', reasoning_ms: 0,
      live_tools: [] }, chat);
    box = look(three);
    turns = box.querySelectorAll('.turn');
    if (turns.length !== 3) {
      faults.push('the talk of three messages made ' + turns.length + ' turns, ' + where);
    }
    var back = turns[1];
    if (!back) { faults.push('the run of the tool has no turn, ' + where); }
    back = back || document.createElement('div');
    if (back.querySelectorAll('.think').length !== 1) {
      faults.push('the talk read back holds ' + back.querySelectorAll('.think').length + ' panels, ' + where);
    }
    if (back.querySelectorAll('.tool').length !== 1) {
      faults.push('the talk read back holds ' + back.querySelectorAll('.tool').length + ' runs, ' + where);
    }
    if (box.textContent.indexOf('The notes hold one line') < 0) {
      faults.push('the answer of the talk read back is gone, ' + where);
    }

    // Four: a run of a tool that stands on its own, with the answer coming in
    // after it and the Chatting setting off. The block must still leave off
    // saying 'running' when its result comes in.
    where = 'the run outside the panel';
    var four = Object.assign({ messages: head.concat([run('Running ...')]),
      generating: true, summarising: false, live: '',
      reasoning: '', reasoning_ms: 0, reasoning_done: false, live_tools: [] },
      { chatting: { collapse_thinking: false, grow_thinking: false,
                    tools_in_thinking: false, collapse_tools: false } });
    box = look(four);
    turns = box.querySelectorAll('.turn');
    var plainRun = turns[turns.length - 2];
    var plainPill = plainRun ? plainRun.querySelector('.tool .state') : null;
    if (!plainPill || plainPill.textContent !== 'running') {
      faults.push('the block of a run outside a panel says ' + (plainPill ? plainPill.textContent : 'nothing') + ', ' + where);
    }
    if (plainRun) { plainRun.__kept = 'turn'; }
    var five = Object.assign({ messages: head.concat([run('the notes, read')]),
      generating: true, summarising: false, live: '',
      reasoning: '', reasoning_ms: 0, reasoning_done: false, live_tools: [] },
      { chatting: { collapse_thinking: false, grow_thinking: false,
                    tools_in_thinking: false, collapse_tools: false } });
    box = look(five);
    turns = box.querySelectorAll('.turn');
    var keptRun = turns[turns.length - 2];
    if (!keptRun || keptRun.__kept !== 'turn') {
      faults.push('the turn of a run outside a panel was made again, ' + where);
    }
    var keptPill = keptRun ? keptRun.querySelector('.tool .state') : null;
    if (!keptPill || keptPill.textContent !== 'done') {
      faults.push('the block of a run outside a panel says ' + (keptPill ? keptPill.textContent : 'nothing') + ' after its result, ' + where);
    }

    return faults;
  });
  await browser.close();
  if (out.length) {
    console.log(out.join('\n'));
    process.exit(1);
  }
  console.log('a panel that holds a run of a tool is put again part by part: the '
    + 'reading place is kept, the thoughts are not made again, and the block of '
    + 'the run says what became of it while the answer goes on');
})().catch((e) => { console.error(e); process.exit(1); });
