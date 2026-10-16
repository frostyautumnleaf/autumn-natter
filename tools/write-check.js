// Check the block of a tool call that is being written. The page is loaded as
// it stands, with no bridge and no engine: the writer of the live answer is
// driven by hand, with a command that grows a few characters at a time. After
// every look the block is read back.
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
    // The states the engine would hand out: the command grows, then the reason
    // of it grows, then the call goes away.
    var steps = [];
    for (var n = 0; n <= FULL.length; n += 4) {
      steps.push({ command: FULL.slice(0, n), purpose: '' });
    }
    for (var w = 0; w <= WHY.length; w += 5) {
      steps.push({ command: FULL, purpose: WHY.slice(0, w) });
    }
    steps.push(null);

    var seen = [];
    for (var s = 0; s < steps.length; s++) {
      var one = steps[s];
      // The first look of a turn is made by fillLive, as drawThread does it.
      var list = one ? [{ name: 'run_terminal', command: one.command, purpose: one.purpose }] : [];
      fillLive(host, { body: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
        tools: list });
      feedWrite(list);
      if (!one) {
        if (ink.tools.length !== 0) { faults.push('a block stayed when the call went away'); }
        if (host.querySelectorAll('.tool.write').length !== 0) {
          faults.push('the panel of a gone call stayed on the page');
        }
        break;
      }
      var panels = host.querySelectorAll('.tool.write');
      if (panels.length !== 1) {
        faults.push('the block of the call is there ' + panels.length + ' times at ' + s);
        continue;
      }
      var box = ink.tools[0];
      var written = box.text.textContent.replace('', '');
      if (written !== one.command) {
        faults.push('the command on the page is ' + JSON.stringify(written) + ' at ' + s +
          ', not ' + JSON.stringify(one.command));
      }
      // The mark of the writing never stands before the words of it.
      var caret = box.caret;
      if (!caret.parentNode) { faults.push('the mark of the writing is gone at ' + s); continue; }
      if (caret.nextSibling) {
        faults.push('words stand after the mark of the writing at ' + s);
      }
      var inCommand = caret.parentNode === box.text;
      if (one.purpose && inCommand) {
        faults.push('the mark stayed on the command while the reason was written at ' + s);
      }
      if (!one.purpose && !inCommand) {
        faults.push('the mark left the command before the reason came at ' + s);
      }
      if (one.purpose && box.why.words.nodeValue !== one.purpose) {
        faults.push('the reason on the page is behind at ' + s);
      }
      // The head of the block says the first line of the command.
      var head = panels[0].querySelector('.cmd').textContent;
      if (one.command && head.indexOf(one.command.split('\n')[0].slice(0, 20)) !== 0) {
        faults.push('the head does not follow the command at ' + s + ': ' + head);
      }
      seen.push(box.words.nodeValue.length);
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
    return faults;
  });
  await browser.close();
  if (bad.length) { console.log('FAULTS\n' + bad.slice(0, 8).join('\n')); process.exit(1); }
  console.log('the command was written to the page a few words at a time, and the mark stayed behind it');
})();
