// Check the MCP badge and the MCP tab. The badge is a door to the servers of
// the reader's own: it holds one line for each server that stands in the menu,
// and a line turns a server on or off for this chat alone. The tab holds every
// server, the mark chosen for it, and the words of one that is written in the
// program itself. The page is loaded as it stands, with no bridge and no
// engine: the menu and the tab are filled by hand with the states the engine
// hands out.
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
    var sent = [];
    post = function (where, body) { sent.push([where, body]); };

    var servers = [
      { id: 'one', name: 'Bookshop', slug: 'bookshop', kind: 'stdio', icon: 7, shown: true,
        on: true, tools: ['look_up', 'reserve'], fault: '' },
      { id: 'two', name: 'Clock', slug: 'clock', kind: 'internal', icon: 2, shown: true,
        on: false, tools: ['now'], fault: '' },
      { id: 'three', name: 'Archive', slug: 'archive', kind: 'http', icon: 9, shown: false,
        on: false, tools: [], fault: 'The server did not answer' }
    ];
    var state = { mcp: { python: 'python3', on: ['one'], servers: servers },
      generating: false, thinking: { mode: 'off', available: false, modes: [] } };
    cur = state;

    // The badge opens a menu, and asks nothing of the program until a line of
    // that menu is pressed.
    el.mcp.click();
    if (el.mcpmenu.classList.contains('hidden')) {
      faults.push('the badge did not open the menu of the servers');
    }
    if (sent.length) { faults.push('the badge asked the program at once'); }
    var lines = el.mcpmenu.querySelectorAll('button.toggle');
    if (lines.length !== 2) {
      faults.push('the menu holds ' + lines.length + ' lines, not the two servers of the menu');
    }
    var words = el.mcpmenu.textContent;
    if (words.indexOf('Bookshop') < 0 || words.indexOf('Clock') < 0) {
      faults.push('a server of the menu is missing its name');
    }
    if (words.indexOf('Archive') >= 0) {
      faults.push('a server kept out of the menu stands in it');
    }
    if (el.mcpmenu.querySelectorAll('.ico svg').length !== 2) {
      faults.push('a line of the menu has no mark of its own');
    }
    if (!el.mcpmenu.querySelector('button.toggle.on')) {
      faults.push('the server this chat stands on is not marked in the menu');
    }
    // One line turns one server on or off, and says which.
    lines[0].click();
    if (!sent.length || sent[0][0] !== '/api/mcp_on'
        || sent[0][1].on !== false || sent[0][1].id !== 'one') {
      faults.push('the line of the menu did not ask for the one server: ' + JSON.stringify(sent));
    }
    // The badge says how many servers the chat offers.
    drawMcp(state);
    if (el.mcptext.textContent.indexOf('1') < 0) {
      faults.push('the badge does not say how many servers are offered: ' + el.mcptext.textContent);
    }
    if (!el.mcp.classList.contains('on')) { faults.push('the badge does not stand bright'); }
    el.mcp.click();

    // The tab holds every server, the one kept out of the menu saying so, and
    // the one that did not answer saying why.
    var list = byId('mcpList');
    list.innerHTML = '';
    for (var i = 0; i < servers.length; i++) { list.appendChild(mcpRow(servers[i])); }
    var rows = list.querySelectorAll('.mcprow');
    if (rows.length !== 3) { faults.push('the tab holds ' + rows.length + ' servers, not three'); }
    var said = list.textContent;
    if (said.indexOf('Did not answer') < 0) {
      faults.push('a server that did not answer does not say so');
    }
    if (list.querySelectorAll('.mcprow .ico svg').length !== 3) {
      faults.push('a server of the list has no mark of its own');
    }
    var menuButtons = [];
    for (var r = 0; r < rows.length; r++) {
      var hold = rows[r].querySelector('button[title^="Let this server stand"]');
      if (!hold) { faults.push('a row has no switch for standing in the menu'); }
      else { menuButtons.push(hold.textContent); }
    }
    if (menuButtons.join('|') !== 'In menu|In menu|Out') {
      faults.push('the switch of the menu does not say where each server stands: '
        + menuButtons.join('|'));
    }
    // The picker offers sixteen marks and stands on one of them.
    showMcpEditor(null);
    var marks = byId('mcIcon').querySelectorAll('button');
    if (marks.length !== 16) { faults.push('the picker holds ' + marks.length + ' marks'); }
    if (!byId('mcIcon').querySelector('button.on')) {
      faults.push('the picker has no mark stood on');
    }
    marks[4].click();
    if (mcpIcon !== 4 || !byId('mcIcon').querySelector('button.on.title')
        && byId('mcIcon').querySelectorAll('button')[4].className.indexOf('on') < 0) {
      faults.push('a mark of the picker cannot be stood on');
    }
    // The kind moves which fields stand.
    var kind = byId('mcKind');
    kind.value = 'internal';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcCodeRow').style.display === 'none') {
      faults.push('the words of a server of the program itself are hidden');
    }
    if (byId('mcUrlRow').style.display !== 'none') {
      faults.push('the address of a server of the program itself is shown');
    }
    kind.value = 'http';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcUrlRow').style.display === 'none') {
      faults.push('the address of a server of the network is hidden');
    }
    if (byId('mcCodeRow').style.display !== 'none') {
      faults.push('the words of Python stand under a server of the network');
    }
    // What stands in the fields is what is kept, under the kind it began as.
    byId('mcName').value = 'Tally';
    byId('mcUrl').value = 'http://127.0.0.1:9000/mcp';
    mcpSave();
    var save = sent[sent.length - 1];
    if (!save || save[0] !== '/api/mcp' || save[1].op !== 'save') {
      faults.push('Save did not ask for the server to be kept');
    } else {
      var server = save[1].server;
      if (server.name !== 'Tally' || server.kind !== 'http'
          || server.url !== 'http://127.0.0.1:9000/mcp' || server.icon !== 4) {
        faults.push('Save sent other words than the fields held: ' + JSON.stringify(server));
      }
    }
    // The example a server of the program itself begins with comes from the
    // program with the list of the servers, and the field of the code takes it
    // when the kind is moved to that one.
    var EXAMPLE = '"""An example of a server of the program itself.\n\n' +
      '@tool\ndef hello(name: str) -> str:\n    """Say hello to one person."""\n' +
      '    return "Hello, " + name;\n';
    mcpRead = function () {
      return Promise.resolve({ servers: servers, python: 'python3', template: EXAMPLE });
    };
    loadMcp();
    await new Promise(function (rest) { setTimeout(rest, 20); });
    if (mcpTemplate !== EXAMPLE) {
      faults.push('the example did not come over the read of the servers');
    }
    kind.value = 'internal';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcCode').value !== EXAMPLE) {
      faults.push('the field of the code did not take the example: ' + byId('mcCode').value);
    }
    // What the reader wrote is never written over, and the example is offered
    // again when the field still holds the example whole.
    byId('mcCode').value = 'the words of the reader';
    kind.value = 'stdio';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    kind.value = 'internal';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcCode').value !== 'the words of the reader') {
      faults.push('the example was written over the words of the reader');
    }
    byId('mcCode').value = EXAMPLE;
    kind.value = 'stdio';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    kind.value = 'internal';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcCode').value !== EXAMPLE) {
      faults.push('the example could not be offered again to a fresh field');
    }
    // A page that never got the example leaves the field as the reader left it.
    mcpTemplate = '';
    byId('mcCode').value = '';
    kind.value = 'stdio';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    kind.value = 'internal';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    if (byId('mcCode').value !== '') {
      faults.push('a field was filled with nothing at all');
    }
    // A tool of a server is drawn in the shape of a tool of a server, and the
    // name the reader sees is the name the server gave it.
    var shape = toolShape('mcp__bookshop__look_up');
    if (String(shape.head).indexOf('Calling') < 0) {
      faults.push('a tool of a server is not drawn as one');
    }
    if (mcpToolWords('mcp__bookshop__look_up') !== 'look_up') {
      faults.push('the name of the tool of a server is not read back');
    }
    return faults;
  });
  await browser.close();
  if (out.length) {
    console.log('FAULTS');
    out.forEach(function (f) { console.log(f); });
    process.exit(1);
  }
  console.log('the MCP badge opens a menu of the servers with a mark and a switch each, '
    + 'the tab holds every server with its mark, its kind and what became of the look, '
    + 'the picker offers sixteen marks, the kind moves which fields stand, the field of '
    + 'the code of a server of the program itself begins with the example the program '
    + 'sends and keeps what the reader wrote, and Save sends what the fields hold');
})();
