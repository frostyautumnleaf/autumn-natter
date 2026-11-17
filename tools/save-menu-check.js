// Check what the button of a block of offered files does with the browser. The
// folder a browser downloads into is rarely the folder the user wanted, so a
// browser that answers to `showSaveFilePicker` is asked where the file is to go:
// the native dialog of the operating system, with the name of the file the block
// showed and the kind of it. Where the browser cannot ask - the window of the
// program, a page that is not stood on a secure place, an older browser - the file
// goes to the download folder, as it always did. A dialog the user let go is a
// thing they said no to: nothing is fetched and nothing is written.
//
//   node save-menu-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node save-menu-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

function stateOf() {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'The files an answer left', },
    chats: [{ id: 'c1', title: 'The files an answer left', repo: '', when: 'Today 09:04',
      active: true, preview: 'Give me the page.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Leave me the page and the table.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Tool', body: 'a-page-of-the-work.html',
        tool: '2 files are offered to the user in download blocks',
        tool_name: 'offer_files',
        downloads: [
          { name: 'a-page-of-the-work', extension: 'html', kind: 'page',
            path: 'a-page-of-the-work.html', size: 4213 },
          { name: 'the counts of the run', extension: 'csv', kind: 'table',
            path: 'counts/of-the-run.csv', size: 118 }
        ],
        mono: false, failed: false, when: '09:05' },
      { id: '3', role: 'Assistant', body: 'The page and the table stand under this answer.',
        reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false,
        failed: false, when: '09:06' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

async function main() {
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = require('child_process').execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
        || process.env.HOME + '/.cache/puppeteer') + '" '
        + '-name chrome-headless-shell -type f | head -1').toString().trim();
    } catch (e) { shell = ''; }
  }
  const browser = await puppeteer.launch({
    headless: 'shell', executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', function (e) { errors.push(String(e)); });
  const state = stateOf();
  if (USE_FETCH) {
    await page.evaluateOnNewDocument(function (state) {
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
    }, state);
  } else {
    await page.evaluateOnNewDocument(function (state) {
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, state);
  }
  await page.setViewport({ width: 1100, height: 900 });
  await page.goto('file://' + path.resolve(PAGE));
  await new Promise(function (r) { setTimeout(r, 500); });

  const work = await page.evaluate(async function () {
    var road = [];        // every step of a download, in the order it was taken
    var picks = [];       // what the dialog was asked with
    var written = [];     // the bytes that came to the file the dialog chose
    var anchors = [];     // the names given to the browser for a plain download
    var fetches = [];     // the roads the bytes were asked for

    // The bytes of a file, as the small server of the program answers them. The
    // road of the archive is answered too, for the button of the whole offer.
    var plain = window.fetch;
    window.fetch = function (where) {
      var said = String(where);
      if (said.indexOf('/api/gift') === 0 || said.indexOf('/api/gift_zip') === 0) {
        fetches.push(said);
        road.push('fetch');
        return Promise.resolve({
          ok: true,
          blob: function () { return Promise.resolve(new Blob(['the bytes of the file'])); }
        });
      }
      // Every other road is left to the fetch the page came with, which is the
      // small server of the remote mode or nothing at all.
      return plain.apply(window, arguments);
    };

    // The click of an anchor is how a browser is told to download. It is watched,
    // so a plain download can be told from a file written where the user chose.
    var clickOf = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () {
      if (this.download) { anchors.push(this.download); road.push('anchor'); }
      return clickOf.apply(this, arguments);
    };

    // The dialog of the browser. The handle it gives back opens, writes and
    // closes, which is all the page does with it.
    function handleOf(name) {
      return {
        createWritable: function () {
          return Promise.resolve({
            write: function (made) { written.push(name + ':' + made.size + 'B'); road.push('write'); },
            close: function () { return Promise.resolve(); }
          });
        }
      };
    }
    function answerPicker(name) {
      window.showSaveFilePicker = function (wants) {
        picks.push(wants);
        road.push('picker');
        return Promise.resolve(handleOf(name));
      };
    }
    function abortPicker() {
      window.showSaveFilePicker = function (wants) {
        picks.push(wants);
        road.push('picker');
        var why = new Error('The user let the dialog go.');
        why.name = 'AbortError';
        return Promise.reject(why);
      };
    }

    function block(n) {
      var blocks = document.querySelectorAll('.gifts .gift');
      return blocks.length > n ? blocks[n] : null;
    }
    function wait(ms) { return new Promise(function (r) { setTimeout(r, ms || 240); }); }
    function reset() { road = []; picks = []; written = []; anchors = []; fetches = []; }

    // The page of the program asks for the bytes over the bridge; the road of the
    // small server is the one the browser takes. Both faces are checked here as
    // the page of the browser, which is where the dialog stands.
    if (typeof inWindow !== 'undefined') { inWindow = false; }
    if (!block(0) || !block(1)) { return { none: true }; }

    // A browser that answers: the dialog is asked first, with the whole name of
    // the file and its kind, and the bytes go to the file the user chose. Nothing
    // is given to the browser as a plain download.
    reset();
    answerPicker('a-page-of-the-work.html');
    block(0).querySelector('.get').click();
    await wait();
    var asked = { road: road.slice(), picks: picks.slice(), written: written.slice(),
      anchors: anchors.slice(), fetches: fetches.slice() };

    // The dialog let go: nothing is fetched, nothing is written, and the browser
    // is not given the file either.
    reset();
    abortPicker();
    block(0).querySelector('.get').click();
    await wait();
    var cancelled = { road: road.slice(), fetches: fetches.slice(),
      written: written.slice(), anchors: anchors.slice() };

    // A browser that does not answer: the file goes to the download folder, under
    // the name the block showed, as it always did.
    reset();
    delete window.showSaveFilePicker;
    block(0).querySelector('.get').click();
    await wait();
    var plainDown = { road: road.slice(), anchors: anchors.slice(),
      fetches: fetches.slice(), written: written.slice() };

    // The whole of an offer: the archive is asked for, and the dialog is asked
    // with the name of the archive.
    reset();
    answerPicker('a-page-of-the-work.html.zip');
    var all = document.querySelector('.gifts .all');
    if (all) { all.click(); }
    await wait();
    var whole = { road: road.slice(), picks: picks.slice(), written: written.slice(),
      anchors: anchors.slice(), fetches: fetches.slice() };

    window.fetch = plain;
    HTMLAnchorElement.prototype.click = clickOf;
    return { asked: asked, cancelled: cancelled, plainDown: plainDown, whole: whole,
      blocks: document.querySelectorAll('.gifts .gift').length };
  });

  await browser.close();

  const faults = [];
  if (work.none) { throw new Error('no block of offered files stands in the chat'); }
  if (work.blocks !== 2) { faults.push('the offer drew ' + work.blocks + ' blocks'); }

  // The dialog was asked before the bytes, with the name and the kind of the file.
  const asked = work.asked;
  if (asked.road[0] !== 'picker') {
    faults.push('the dialog was not asked first: ' + asked.road.join(' -> '));
  }
  if (asked.road.indexOf('fetch') < 0) { faults.push('the bytes were never asked for'); }
  if (asked.road.indexOf('write') < 0) { faults.push('the bytes never came to the chosen file'); }
  if (asked.anchors.length) {
    faults.push('a browser that was asked was also given a plain download: ' + asked.anchors[0]);
  }
  const wants = asked.picks[0] || {};
  if (wants.suggestedName !== 'a-page-of-the-work.html') {
    faults.push('the dialog was not given the whole name of the file: '
      + JSON.stringify(wants.suggestedName));
  }
  const accept = ((wants.types || [])[0] || {}).accept || {};
  if (!accept['text/html']) {
    faults.push('the dialog was not given the kind of the file: ' + JSON.stringify(wants.types));
  }
  if (asked.written.indexOf('a-page-of-the-work.html:21B') < 0) {
    faults.push('the bytes written are not the bytes of the file: ' + asked.written.join(','));
  }
  if (asked.fetches[0].indexOf('/api/gift?path=') !== 0) {
    faults.push('the road of the file is not the road of the offer: ' + asked.fetches[0]);
  }

  // A dialog let go is a thing the user said no to.
  if (work.cancelled.fetches.length || work.cancelled.written.length
    || work.cancelled.anchors.length) {
    faults.push('a dialog let go still took the file: ' + work.cancelled.road.join(' -> '));
  }

  // With no dialog to ask, the plain download stands, under the right name.
  const plainDown = work.plainDown;
  if (plainDown.anchors[0] !== 'a-page-of-the-work.html') {
    faults.push('the plain download did not carry the name of the file: '
      + JSON.stringify(plainDown.anchors));
  }
  if (plainDown.written.length) {
    faults.push('a browser that cannot ask wrote into a handle of its own');
  }

  // The whole of an offer is asked for as an archive, with a name of its own.
  if (work.whole.picks.length && work.whole.picks[0].suggestedName !== 'a-page-of-the-work.html.zip') {
    faults.push('the archive was not given its name: '
      + JSON.stringify(work.whole.picks[0].suggestedName));
  }
  if (work.whole.fetches[0] && work.whole.fetches[0].indexOf('/api/gift_zip?') !== 0) {
    faults.push('the whole of the offer was not asked for as an archive: ' + work.whole.fetches[0]);
  }
  if (work.whole.anchors.length) {
    faults.push('the archive was given as a plain download although the dialog was asked');
  }

  const face = process.env.AN_PAGE ? 'the page of the remote mode' : 'the window page';
  if (!faults.length) {
    console.log('the button of a block of offered files asks the browser where the file is '
      + 'to go, writes the bytes there, lets a dialog that was let go stand for a no, and '
      + 'falls back on the download folder where the browser cannot ask - on ' + face);
    return;
  }
  console.log(`faults on ${face}:`);
  faults.forEach(function (f) { console.log(' - ' + f); });
  process.exitCode = 1;
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
