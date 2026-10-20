// Check the block of a tool call that is being written. The page is loaded as
// it stands, with no bridge and no engine: the writer of the live answer is
// driven by hand, with a command that grows a few characters at a time. After
// every look the block is read back. Two things the reader sees while a call is
// written are looked at too: only one mark beats, and the block does not stand
// hard against the words above it.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + PAGE);
  const bad = await page.evaluate(async () => {
    var host = document.createElement('div');
    host.className = 'turn t-ai';
    document.body.appendChild(host);
    var faults = [];
    var FULL = 'cargo test --release markdown -- --nocapture';
    var WHY = 'see whether the order of the blocks is mended';
    // The words a model may say before it begins a call. The other look of the
    // same answer spends no word before the call, which is how the two marks
    // were first seen beating over one another.
    var SAID = 'I will write a short greeting to a file called hello.txt, '
      + 'and I will read it back to you.';

    // The marks of the beat that really stand. A mark put away with no display
    // does not beat, and the reader cannot see it.
    function beating(box) {
      var found = box.querySelectorAll('.caret'), shown = [];
      for (var n = 0; n < found.length; n++) {
        if (window.getComputedStyle(found[n]).display !== 'none') { shown.push(found[n]); }
      }
      return shown;
    }

    // The states the engine would hand out: the command grows, then the reason
    // of it grows.
    var steps = [];
    for (var n = 0; n <= FULL.length; n += 4) {
      steps.push({ command: FULL.slice(0, n), purpose: '' });
    }
    for (var w = 0; w <= WHY.length; w += 5) {
      steps.push({ command: FULL, purpose: WHY.slice(0, w) });
    }

    var bodies = ['', SAID];
    for (var b = 0; b < bodies.length; b++) {
      var body = bodies[b];
      var where = body ? 'after words' : 'with no words';
      for (var s = 0; s < steps.length; s++) {
        var one = steps[s];
        where = (body ? 'after words' : 'with no words') + ' at ' + s;
        // The first look of a turn is made by fillLive, as drawThread does it.
        var list = [{ name: 'run_terminal', command: one.command, purpose: one.purpose }];
        fillLive(host, { body: body, reasoning: '', reasoning_ms: 0, reasoning_done: false,
          tools: list });
        feedWrite(list);
        var panels = host.querySelectorAll('.tool.write');
        if (panels.length !== 1) {
          faults.push('the block of the call is there ' + panels.length + ' times ' + where);
          continue;
        }
        var box = ink.tools[0];
        var written = box.text.textContent;
        if (written !== one.command) {
          faults.push('the command on the page is ' + JSON.stringify(written) + ' at ' + s +
            ', not ' + JSON.stringify(one.command) + ' ' + where);
        }
        // The mark of the writing never stands before the words of it.
        var caret = box.caret;
        if (!caret.parentNode) { faults.push('the mark of the writing is gone ' + where); continue; }
        if (caret.nextSibling) {
          faults.push('words stand after the mark of the writing ' + where);
        }
        var inCommand = caret.parentNode === box.text;
        if (one.purpose && inCommand) {
          faults.push('the mark stayed on the command while the reason was written ' + where);
        }
        if (!one.purpose && !inCommand) {
          faults.push('the mark left the command before the reason came ' + where);
        }
        if (one.purpose && box.why.words.nodeValue !== one.purpose) {
          faults.push('the reason on the page is behind ' + where);
        }
        // The head of the block says the first line of the command.
        var head = panels[0].querySelector('.cmd').textContent;
        if (one.command && head.indexOf(one.command.split('\n')[0].slice(0, 20)) !== 0) {
          faults.push('the head does not follow the command at ' + s + ': ' + head);
        }
        // One mark of the beat only. The block carries it, and the mark of the
        // answer stands aside while the call is written.
        var marks = beating(host);
        if (marks.length !== 1) {
          faults.push(marks.length + ' marks beat ' + where);
        } else if (!marks[0].closest('.tool.write')) {
          faults.push('the mark that beats stands outside the block ' + where);
        }
        // The block is kept apart from the words above it, and only from words.
        var room = window.getComputedStyle(ink.toolHost);
        var ruled = room.borderTopWidth !== '0px';
        if (!!body !== ruled) {
          faults.push('the rule over the block ' + (ruled ? 'stood' : 'was left out') + ' ' + where);
        }
        if (body && (parseFloat(room.marginTop) < 16 || parseFloat(room.paddingTop) < 16)) {
          faults.push('the block stands too close under the words ' + where);
        }
      }
      // The call has gone, so the block goes, the mark of the answer beats
      // again, and the rule of the block goes with it.
      fillLive(host, { body: body, reasoning: '', reasoning_ms: 0, reasoning_done: false,
        tools: [] });
      feedWrite([]);
      if (ink.tools.length !== 0) { faults.push('a block stayed when the call went away ' + where); }
      if (host.querySelectorAll('.tool.write').length !== 0) {
        faults.push('the panel of a gone call stayed on the page ' + where);
      }
      var gone = beating(host);
      if (gone.length !== 1) {
        faults.push('the mark of the answer did not come back when the call went ' + where);
      } else if (gone[0] !== ink.caret) {
        faults.push('the mark that beats is not the mark of the answer ' + where);
      }
      if (window.getComputedStyle(ink.toolHost).borderTopWidth !== '0px') {
        faults.push('the rule stayed when the call went away ' + where);
      }
    }
    // The block is made once and then written again, so a look does not build it
    // anew. Two looks of the same words therefore keep the nodes they had.
    fillLive(host, { body: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
      tools: [{ name: 'run_terminal', command: 'uname -a', purpose: '' }] });
    feedWrite([{ name: 'run_terminal', command: 'uname -a', purpose: '' }]);
    var first = ink.tools[0].panel;
    feedWrite([{ name: 'run_terminal', command: 'uname -a', purpose: '' }]);
    if (ink.tools[0].panel !== first) { faults.push('the block was made again for no new word'); }
    if (first.parentNode === null) { faults.push('the block left the page'); }

    // The same block, driven with the states of a file tool. A model writes the
    // two halves of a change, and the name of the file, in the order it likes.
    // The states below are the ones a call hands out that puts the words to come
    // in before the words to take out, and the name of the file after both.
    // Two things the reader sees are looked at: a word of the change stands in
    // the body as soon as there is one, and the head names a file, or says a
    // file is begun, but never wears a word of the change as a name of a file.
    var CALLS = [
      { name: 'edit_file', said: 'I will change the file.', states: [
        { target: '', command: '' },
        { target: '', command: '> ' },
        { target: '', command: '> un' },
        { target: '', command: '> uno' },
        { target: '', command: 'on\n> uno' },
        { target: '', command: 'one\n> uno' },
        { target: 'notes', command: 'one\n> uno' },
        { target: 'notes/todo.txt', command: 'one\n> uno' },
        { target: 'notes/todo.txt', command: 'one\n> uno\ntwo\n> dos' }
      ] },
      { name: 'create_file', said: 'I will make the file.', states: [
        { target: '', command: '' },
        { target: '', command: '# gr' },
        { target: 'greet', command: '# greeting' },
        { target: 'greet.md', command: '# greeting\n' }
      ] }
    ];
    for (var c = 0; c < CALLS.length; c++) {
      var call = CALLS[c];
      var words = call.name === 'edit_file' ? 'Editing file: ' : 'Creating file: ';
      var seen = [];
      for (var t = 0; t < call.states.length; t++) {
        var part = call.states[t];
        var half = { name: call.name, target: part.target, command: part.command,
          purpose: '' };
        var where = call.name + ' at ' + t;
        if (t === 0) {
          fillLive(host, { body: call.said, reasoning: '', reasoning_ms: 0,
            reasoning_done: true, tools: [half] });
        } else {
          feedWrite([half]);
        }
        var found = host.querySelectorAll('.tool.write');
        if (found.length !== 1) {
          faults.push('the block of the file call is there ' + found.length + ' times ' + where);
          break;
        }
        var partBox = ink.tools[0];
        if (partBox.words.nodeValue !== half.command) {
          faults.push('the body of the file block is ' + JSON.stringify(partBox.words.nodeValue)
            + ' and not the words of the change ' + where);
        }
        // A half of a change that has come in stands to be read. The block was
        // once blank until the two halves of a change happened to meet.
        if (half.command.trim() && !partBox.text.textContent.trim()) {
          faults.push('the body of the file block was blank over words that had come ' + where);
        }
        var head = found[0].querySelector('.cmd').textContent;
        var named = words + (part.target.trim() ? part.target : 'a file, begun');
        if (head !== named) {
          faults.push('the head of the file block is ' + JSON.stringify(head)
            + ' and not ' + JSON.stringify(named) + ' ' + where);
        }
        if (seen.indexOf(head) === -1) { seen.push(head); }
      }
      // The head of the block moved from the file begun to the file named, and
      // nothing else. A head that read out a word of the change would be here.
      if (seen.length < 2) {
        faults.push('the head of the ' + call.name + ' block never named the file');
      }
    }
    return faults;
  });
  await browser.close();
  if (bad.length) { console.log('FAULTS\n' + bad.slice(0, 8).join('\n')); process.exit(1); }
  console.log('the command was written to the page a few words at a time: one mark beat, '
    + 'and the block stood under the words of the answer with room and a rule;\n'
    + 'the halves of a file call came in the order the model liked, and the body of the '
    + 'block showed a word of the change as it came while the head named the file, '
    + 'or said a file was begun');
})();
