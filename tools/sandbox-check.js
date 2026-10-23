// Check the menu of the sandbox badge. The badge used to turn the jail on and
// off with one click; now it opens a menu, for what a command of the chat may
// reach is three things beside the jail itself, and the reader must see all of
// them at once and open them one by one. All three stand off until they are
// opened. The page is loaded as it stands, with no bridge and no engine: the
// menu is filled by hand with the states the engine hands out. The box of the
// menu is measured as well, for a setting with a long name must be read on one
// line of a box that grew to hold it, and never cut off.
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
    // What the page asks of the engine is caught here instead of sent.
    var sent = [];
    post = function (where, body) { sent.push([where, body]); };

    var state = { sandbox: true, sandbox_available: true, sandbox_network: false,
      sandbox_pip: false, sandbox_fetch: false, generating: false,
      thinking: { mode: 'off', available: false, modes: [] } };
    cur = state;

    // The badge opens the menu rather than turning the jail off.
    el.sandbox.click();
    if (el.sandboxmenu.classList.contains('hidden')) {
      faults.push('the badge did not open the menu of the sandbox');
    }
    if (sent.length) { faults.push('the badge asked the engine at once: ' + JSON.stringify(sent)); }

    var lines = el.sandboxmenu.querySelectorAll('button');
    if (lines.length !== 4) {
      faults.push('the menu holds ' + lines.length + ' lines, not the jail and three settings');
    }
    var words = [];
    for (var i = 0; i < lines.length; i++) { words.push(lines[i].textContent); }
    var wanted = ['Sandbox', 'Network access', 'Allow pip install', 'Allow wget and fetch'];
    for (var w = 0; w < wanted.length; w++) {
      if (words.join('|').indexOf(wanted[w]) < 0) {
        faults.push('the menu has no line for ' + wanted[w]);
      }
    }
    // All three stand off, and the switches say so. The jail is a line of the
    // menu too, and it stands where the state has it, so the three settings are
    // counted after it: the jail stands at the head of the menu.
    function gates() {
      var all = el.sandboxmenu.querySelectorAll('button.toggle');
      return Array.prototype.slice.call(all, 1);
    }
    // The number of lines of words a node is set in. A line of the menu that is
    // broken in two reads badly, and it is what these measurements are for.
    function linesOf(node) {
      var range = document.createRange();
      range.selectNodeContents(node);
      return range.getClientRects().length;
    }
    var on = 0;
    var first = gates();
    if (first.length !== 3) {
      faults.push('the menu has ' + first.length + ' lines of settings, not three');
    }
    for (var g = 0; g < first.length; g++) { if (first[g].classList.contains('on')) { on++; } }
    if (on !== 0) { faults.push(on + ' of the settings read as open though all stand off'); }

    // A click on a line asks for that one setting, and leaves the menu standing
    // so the switch is seen to move.
    sent = [];
    var pip = null, fetchLine = null;
    for (var k = 0; k < first.length; k++) {
      if (first[k].textContent.indexOf('pip install') >= 0) { pip = first[k]; }
      if (first[k].textContent.indexOf('wget and fetch') >= 0) { fetchLine = first[k]; }
    }
    if (!pip || !fetchLine) { faults.push('a setting of the menu was not to be found'); }
    if (pip) {
      pip.click();
      if (sent.length !== 1 || sent[0][0] !== '/api/sandbox_setting'
          || sent[0][1].part !== 'pip' || sent[0][1].on !== true) {
        faults.push('the line of the install asked for ' + JSON.stringify(sent));
      }
      if (el.sandboxmenu.classList.contains('hidden')) {
        faults.push('the menu went away under a click on one of its lines');
      }
    }
    // The engine says the install is open now, so the menu is filled again and
    // the switch of that line stands open. The other two stay shut.
    state.sandbox_pip = true;
    fillSandboxMenu(state);
    var open = [];
    var again = gates();
    for (var n = 0; n < again.length; n++) {
      if (again[n].classList.contains('on')) { open.push(again[n].textContent); }
    }
    if (open.length !== 1 || open[0].indexOf('pip install') < 0) {
      faults.push('the menu shows ' + JSON.stringify(open) + ' open, and only the install was opened');
    }
    sent = [];
    fetchLine = null;
    for (var q = 0; q < again.length; q++) {
      if (again[q].textContent.indexOf('wget and fetch') >= 0) { fetchLine = again[q]; }
    }
    if (fetchLine) {
      fetchLine.click();
      if (sent.length !== 1 || sent[0][1].part !== 'fetch' || sent[0][1].on !== true) {
        faults.push('the line of the fetch asked for ' + JSON.stringify(sent));
      }
    }
    // A setting that stands open is closed by the same line.
    sent = [];
    state.sandbox_fetch = true;
    fillSandboxMenu(state);
    var shut = null;
    var third = gates();
    for (var t = 0; t < third.length; t++) {
      if (third[t].textContent.indexOf('wget and fetch') >= 0) { shut = third[t]; }
    }
    if (shut) {
      shut.click();
      if (sent.length !== 1 || sent[0][1].part !== 'fetch' || sent[0][1].on !== false) {
        faults.push('a line of an open setting did not close it: ' + JSON.stringify(sent));
      }
    }
    // A click elsewhere in the page puts the menu away.
    el.sandboxmenu.classList.remove('hidden');
    document.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    if (!el.sandboxmenu.classList.contains('hidden')) {
      faults.push('the menu stood open after a click elsewhere in the page');
    }
    // The words of the badge say what a command may reach, so a reader who does
    // not open the menu is not left guessing. Everything stands shut again first.
    state.sandbox_pip = false;
    state.sandbox_fetch = false;
    state.sandbox_network = true;
    drawBadges(state);
    if (String(el.sandbox.title).indexOf('network') < 0) {
      faults.push('the badge says nothing of the network being open: ' + el.sandbox.title);
    }
    state.sandbox_network = false;
    state.sandbox_pip = false;
    state.sandbox_fetch = false;
    drawBadges(state);
    if (String(el.sandbox.title).indexOf('no network') < 0) {
      faults.push('the badge says nothing of the network being shut: ' + el.sandbox.title);
    }
    // The box of the menu is as wide as the longest of its lines. No line of
    // words is broken in two by a box that was too narrow for the name of a
    // setting, and no word is cut off at the end of the box. The menu was put
    // away above, so it is stood up again to be measured.
    el.sandboxmenu.classList.remove('hidden');
    fillSandboxMenu(state);
    var box = el.sandboxmenu.getBoundingClientRect();
    if (el.sandboxmenu.scrollWidth > el.sandboxmenu.clientWidth + 1) {
      faults.push('the box of the menu is narrower than its lines: it holds '
        + el.sandboxmenu.scrollWidth + 'px of words in ' + el.sandboxmenu.clientWidth + 'px');
    }
    var each = el.sandboxmenu.querySelectorAll('button');
    for (var b = 0; b < each.length; b++) {
      var said = each[b].querySelector('.word');
      if (!said) { faults.push('a line of the menu has no words of its own'); continue; }
      if (linesOf(said) !== 1) {
        faults.push('the line ' + JSON.stringify(said.textContent) + ' is broken in '
          + linesOf(said) + ' lines of words, in a box of ' + Math.round(box.width) + 'px');
      }
      if (said.scrollWidth > said.clientWidth + 1) {
        faults.push('the words ' + JSON.stringify(said.textContent) + ' are cut off: they need '
          + said.scrollWidth + 'px and have ' + said.clientWidth + 'px');
      }
    }
    // A name longer than any of these makes the box wider rather than breaking
    // the words, for the box is sized by the longest of its lines.
    var longer = 'Allow wget and fetch and curl';
    el.sandboxmenu.appendChild(gateLine('network', longer, '', false));
    var grown = el.sandboxmenu.getBoundingClientRect();
    var lastLine = el.sandboxmenu.querySelector('button:last-child .word');
    if (grown.width <= box.width) {
      faults.push('the box did not grow for a longer name: it stayed '
        + Math.round(box.width) + 'px and would not hold ' + Math.round(grown.width) + 'px');
    }
    if (linesOf(lastLine) !== 1 || lastLine.scrollWidth > lastLine.clientWidth + 1) {
      faults.push('a longer name was not read on one line of the grown box');
    }
    // A name too long for what is left of the window is wrapped rather than cut
    // off, and the box never stands beyond the window.
    el.sandboxmenu.innerHTML = '';
    el.sandboxmenu.appendChild(gateLine('network',
      'Allow wget and fetch and everything else that a command of this chat may want to run, '
      + 'and a word more or two besides', '', false));
    var widest = el.sandboxmenu.getBoundingClientRect();
    var said2 = el.sandboxmenu.querySelector('button .word');
    if (said2.scrollWidth > said2.clientWidth + 1) {
      faults.push('a name too long for the box was cut off rather than wrapped');
    }
    if (linesOf(said2) < 2) {
      faults.push('a name too long for the box was not wrapped at all');
    }
    // The box never leaves the window, whatever the names it holds.
    if (widest.right > window.innerWidth + 1 || widest.left < -1) {
      faults.push('the box of the menu stands beyond the window: from '
        + Math.round(widest.left) + 'px to ' + Math.round(widest.right) + 'px of '
        + window.innerWidth + 'px');
    }
    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('the sandbox badge opens a menu of the jail and three settings, named '
    + 'Sandbox, Network access, Allow pip install and Allow wget and fetch; all three '
    + 'stand off, each is asked for on its own, the words of the badge say what a '
    + 'command may reach, and the box of the menu is as wide as the longest of its lines');
})().catch(function (e) { console.error(e); process.exit(1); });
