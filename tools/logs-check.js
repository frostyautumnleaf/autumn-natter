// Check the Logs tab of the sheet of settings: the log of this run is chosen by
// itself and read again by itself, manual reads stand still until the button is
// pressed, the search shows the lines that hold the words looked for, and a line
// of a fault is read in the colour of a fault. The log is stood in for by a list
// of words that grows when the check asks, which is how a start of the server
// that is going on is spoken for without a server and without a model.
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
  await page.setViewport({ width: 1180, height: 900 });
  await page.goto('file://' + REMOTE);
  const out = await page.evaluate(async () => {
    var faults = [];
    var wait = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };
    var by = function (id) { return document.getElementById(id); };
    var rows = function () {
      return Array.prototype.slice.call(by('logBody').querySelectorAll('.ln'));
    };
    var lastLine = function () {
      var all = rows();
      return all.length ? all[all.length - 1].textContent : '';
    };

    // The log of the server, stood in for. It grows when the check asks, and the
    // calls of the bridge are caught here instead of sent to the program.
    var held = [];
    for (var n = 1; n <= 30; n++) { held.push('0.0' + n + ' I line ' + n + ' of the log'); }
    held.push('0.03.10 W a warning about the room of the talk');
    held.push('0.03.11 E error: the model file could not be read');
    var reads = 0;
    getLogs = function () {
      return Promise.resolve({
        dir: '/data/autumn_natter/logs', version: '0.1.0', kept: 10,
        logs: [
          { id: 'server-2.log', when: '2026-10-23, 21:19', size: '4 KiB', bytes: 4096, current: true },
          { id: 'server-1.log', when: '2026-10-23, 20:04', size: '1 KiB', bytes: 1024, current: false }
        ]
      });
    };
    getLog = function (id) {
      reads++;
      return Promise.resolve({ id: id, text: held.join('\n') + '\n', bytes: 4096, cut: false });
    };
    getConfig = function () { return Promise.resolve({ llama_dir: '/home/me/llamacpp/build/bin' }); };
    cur = { sandbox_note: 'bubblewrap works. A command sees only its own folder.',
      data_dir: '/data/autumn_natter', dark: false };

    // The tab comes forward, and the log this run is writing is the one chosen.
    openSettings('logs');
    await wait(80);
    if (by('paneLogs').classList.contains('hidden')) { faults.push('the Logs tab did not come forward'); }
    if (!by('tabLogs').classList.contains('on')) { faults.push('the Logs tab is not marked as the one in front'); }
    if (by('logPick').value !== 'server-2.log') {
      faults.push('the log chosen by itself is ' + by('logPick').value + ' and not the current one');
    }
    if (by('saveSheet').style.display !== 'none') {
      faults.push('the button for saving stands on the Logs tab, where nothing is saved');
    }
    if (by('logDir').textContent.indexOf('logs') < 0) { faults.push('the folder of the logs is not named'); }
    if (by('logLlama').textContent.indexOf('llamacpp') < 0) { faults.push('the folder of llama.cpp is not named'); }
    if (by('logSandbox').textContent.indexOf('bubblewrap') < 0) { faults.push('the answer of the jail is not shown'); }
    var state = by('logState').textContent;
    if (state.indexOf('server-2.log') < 0) { faults.push('the line over the log does not name the log'); }
    if (state.indexOf('read again at') < 0) { faults.push('the line over the log does not say when it was read'); }

    // The end of the log stands there, and a line of a fault is coloured as one.
    if (lastLine().indexOf('the model file could not be read') < 0) {
      faults.push('the last line of the log is not shown: ' + lastLine());
    }
    var kinds = { err: 0, warn: 0 };
    var every = rows();
    for (var e = 0; e < every.length; e++) {
      if (every[e].classList.contains('err')) { kinds.err++; }
      if (every[e].classList.contains('warn')) { kinds.warn++; }
    }
    if (kinds.err !== 1) { faults.push(kinds.err + ' lines read as a fault, not the one that holds the error'); }
    if (kinds.warn !== 1) { faults.push(kinds.warn + ' lines read as a warning, not the one that holds it'); }
    var body = by('logBody');
    if (body.scrollHeight - body.scrollTop - body.clientHeight > 3) {
      faults.push('the view does not stand at the last line of the log');
    }

    // The search shows the lines that hold the words, and marks the words.
    var search = by('logSearch');
    search.value = 'model file';
    search.dispatchEvent(new Event('input'));
    await wait(40);
    var found = rows();
    if (found.length !== 1) {
      faults.push('the search left ' + found.length + ' lines, not the one that holds the words');
    }
    if (found.length && !found[0].querySelector('b.hit')) {
      faults.push('the words looked for are not marked on the line that holds them');
    }
    if (by('logCount').textContent.indexOf('1 line') < 0) {
      faults.push('the count of the search does not say one line: ' + by('logCount').textContent);
    }
    search.value = 'nothing holds these words';
    search.dispatchEvent(new Event('input'));
    await wait(40);
    if (rows().length !== 0) { faults.push('a search that holds nothing showed lines anyway'); }
    if (by('logCount').textContent.indexOf('nothing') < 0) {
      faults.push('a search that holds nothing did not say so: ' + by('logCount').textContent);
    }

    // The two steps go from one line that holds the words to the next, and round
    // the back again.
    search.value = 'of the log';
    search.dispatchEvent(new Event('input'));
    await wait(40);
    var hits = rows();
    if (hits.length !== 30) { faults.push('the search found ' + hits.length + ' lines, not 30'); }
    by('logNext').click();
    var now = body.querySelectorAll('.ln.now');
    if (now.length !== 1) { faults.push('a step did not stand on exactly one line'); }
    else if (hits.length && now[0] !== hits[0]) { faults.push('the first step does not stand on the first line'); }
    by('logNext').click();
    now = body.querySelectorAll('.ln.now');
    if (hits.length > 1 && now.length && now[0] !== hits[1]) { faults.push('the second step did not move on'); }
    by('logPrev').click();
    by('logPrev').click();
    now = body.querySelectorAll('.ln.now');
    if (hits.length && now.length && now[0] !== hits[hits.length - 1]) {
      faults.push('a step back from the first line did not come round to the last');
    }

    // Reading up turns the following off again, as the chat does.
    search.value = '';
    search.dispatchEvent(new Event('input'));
    await wait(40);
    body.scrollTop = 0;
    body.dispatchEvent(new Event('scroll'));
    await wait(40);
    if (by('logFollow').checked) { faults.push('reading up did not turn the following off'); }
    by('logFollow').checked = true;
    by('logFollow').dispatchEvent(new Event('change'));
    await wait(40);
    if (body.scrollHeight - body.scrollTop - body.clientHeight > 3) {
      faults.push('turning the following on did not bring the view back to the end');
    }

    // Manual: nothing moves until the button beside the field is pressed.
    var rate = by('logRate');
    rate.value = 'manual';
    rate.dispatchEvent(new Event('change'));
    held.push('9.99.00 I a line that came while the reading stood manual');
    reads = 0;
    await wait(1300);
    if (reads !== 0) { faults.push('a manual read asked ' + reads + ' times by itself'); }
    if (lastLine().indexOf('came while the reading stood manual') >= 0) {
      faults.push('a manual read moved by itself');
    }
    by('logRefresh').click();
    await wait(120);
    if (lastLine().indexOf('came while the reading stood manual') < 0) {
      faults.push('the button did not read the log again');
    }

    // Live: the new line comes by itself, and the five second pace does not come
    // sooner than it says.
    rate.value = 'live';
    rate.dispatchEvent(new Event('change'));
    held.push('9.99.01 I a line that came while the tab stood open');
    await wait(1500);
    if (lastLine().indexOf('while the tab stood open') < 0) {
      faults.push('a live read did not bring the line that came in: ' + lastLine());
    }
    rate.value = '5';
    rate.dispatchEvent(new Event('change'));
    held.push('9.99.02 I a line of the five second wait');
    await wait(1500);
    if (lastLine().indexOf('five second wait') >= 0) {
      faults.push('the five second pace came sooner than the five seconds it stands for');
    }
    await wait(4200);
    if (lastLine().indexOf('five second wait') < 0) {
      faults.push('the five second pace never brought the line it was given');
    }

    // The sheet is put away, and the reading of the log stops with it.
    reads = 0;
    closeSettings();
    held.push('9.99.03 I a line written after the sheet was put away');
    await wait(1400);
    if (reads !== 0) { faults.push('the log was read ' + reads + ' times with the sheet closed'); }
    if (!by('overlay').classList.contains('hidden')) { faults.push('the sheet did not go away'); }
    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('the Logs tab chooses the log of this run by itself, reads it again while the');
  console.log('update stands on live or on five seconds and stands still on manual, shows the');
  console.log('lines that hold the words looked for with the words marked and the two steps');
  console.log('between them, colours a line of a fault, and stops reading when the sheet is put away');
})().catch(function (e) { console.error(e); process.exit(1); });
