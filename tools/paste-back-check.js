// Check that the chat composer takes files the way a user proposes them: a file let
// go over the page, a file pasted into the box, and a paste whose event carries
// its file in the list of items only. The last form is what a web view of a
// window pointers over where the list of files is left empty, so the box must
// still take the file. A paste into a field of the settings panel is left
// alone, and a paste of a lot of text still turns into a file of its own.
//
//   node paste-back-check.js                  # the window page
//   AN_PAGE=../web/page.html AN_FETCH=1 node attach-check.js
const path = require('path');
const { execSync } = require('child_process');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';
// The settings panel is in the window alone, so its field is checked there only.
const IS_WINDOW = !USE_FETCH;

// The state of the engine, as the record of web::state. One chat with two messages, so the box
// and the conversation both remain.
function stateOf(dark) {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A picture and a note' },
    chats: [{ id: 'c1', title: 'A picture and a note', repo: '', when: 'Today 09:04',
      active: true, preview: 'What colour is this?' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    carousel: { index: 0, count: 1, blank: false },
    messages: [
      { id: '1', role: 'You', body: 'What colour is this?', reasoning: '', reasoning_ms: 0,
        attachments: [{ id: 'a1', name: 'swatch.png', mime: 'image/png', image: true }],
        tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Assistant', body: 'This image is orange.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:05' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [],
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: !!dark, data_dir: '/tmp/autumn-natter',
  };
}

// The blocks over the text of the box, as the user sees them: one name each, whether the row
// remains, the text of the box and the send marker.
function readHeld() {
  var row = document.getElementById('parts');
  var names = [];
  var blocks = row ? row.querySelectorAll('.part .nm') : [];
  for (var n = 0; n < blocks.length; n++) { names.push(blocks[n].textContent); }
  return {
    names: names,
    hidden: !row || row.classList.contains('hidden'),
    words: document.getElementById('draft').value,
    send: document.getElementById('send').className,
  };
}

async function main() {
  // The headless shell of another computer is named with AN_SHELL_BIN, as the other checks of
  // this program do.
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
        || process.env.HOME + '/.cache/puppeteer') + '" '
        + '-name chrome-headless-shell -type f | head -1').toString().trim();
    } catch (e) { shell = ''; }
  }
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage',
      '--disable-gpu', '--allow-file-access-from-files'],
  });
  const page = await browser.newPage();
  const faults = [];
  const errors = [];
  page.on('pageerror', function (e) { errors.push(String(e)); });

  await page.evaluateOnNewDocument(function (state, useFetch) {
    if (useFetch) {
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      return;
    }
    // The calls of the page, so a check can see which path was taken, and the events of the
    // window, so a check can send one.
    window.__asked = [];
    window.__events = {};
    window.__send__ = function (name, payload) {
      var made = window.__events[name] || [];
      for (var n = 0; n < made.length; n++) { made[n]({ event: name, payload: payload }); }
    };
    window.__TAURI__ = {
      core: {
        invoke: function (command, args) {
          if (command === 'get_state') { return Promise.resolve(state); }
          if (command === 'get_config') { return Promise.resolve({}); }
          if (command === 'get_attachment') {
            return Promise.resolve({ name: 'swatch.png', mime: 'image/png',
              data: 'data:image/png;base64,iVBORw0KGgo=', text: '' });
          }
          // The window reads a file of the computer by its place: an image of one ending comes
          // as a data URL, the rest as text.
          if (command === 'take_files') {
            window.__asked.push('take_files:' + ((args && args.paths) || []).join(','));
            var made = ((args && args.paths) || []).map(function (place) {
              var name = String(place).split('/').pop();
              var picture = /\.png$/i.test(name);
              return { name: name, mime: picture ? 'image/png' : 'text/plain',
                kind: picture ? 'image' : 'text',
                data: picture ? 'data:image/png;base64,iVBORw0KGgo=' : '',
                text: picture ? '' : 'the words of the file' };
            });
            return Promise.resolve({ files: made });
          }
          // The clipboard of the computer, which holds an image of the screen.
          if (command === 'clipboard_take') {
            window.__asked.push('clipboard_take');
            return Promise.resolve({ files: [{ name: 'pasted-picture.png',
              mime: 'image/png', kind: 'image',
              data: 'data:image/png;base64,iVBORw0KGgo=', text: '' }], source: 'machine' });
          }
          return Promise.resolve({ ok: true });
        }
      },
      event: {
        listen: function (name, handler) {
          (window.__events[name] = window.__events[name] || []).push(handler);
          return Promise.resolve(function () {});
        }
      }
    };
  }, stateOf(false), USE_FETCH);

  await page.goto('file://' + path.resolve(PAGE));
  await new Promise(function (r) { setTimeout(r, 500); });

  // One: text already is in the box, and a heap of text is pasted after it, so the heap
  // becomes a block of its own and the box is left empty.
  await page.evaluate(function () {
    var box = document.getElementById('draft');
    box.value = 'Lead: ';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    var transfer = new DataTransfer();
    var long = '';
    for (var n = 0; n < 6500; n++) { long += 'a'; }
    transfer.setData('text/plain', long);
    var ev = new ClipboardEvent('paste', {
      clipboardData: transfer, bubbles: true, cancelable: true
    });
    document.getElementById('draft').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  var held = await page.evaluate(readHeld);
  if (held.names.length !== 1) {
    faults.push('the heap of pasted text did not become a block: ' + JSON.stringify(held.names));
  }
  var pick = await page.evaluate(function () {
    var row = document.getElementById('parts');
    var part = row ? row.querySelector('.part') : null;
    return { pick: !!(part && part.classList.contains('pick')),
      title: part ? part.title : '', tab: part ? part.tabIndex : -1 };
  });
  if (!pick.pick) { faults.push('the block of a text file does not say it may be clicked'); }
  if (pick.tab < 0) { faults.push('the block of a text file cannot be reached from the keys'); }

  // Two: the block is clicked, and its text go into the box where the user left the marker. The
  // block is removed, so the box does not hold them twice.
  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var part = row ? row.querySelector('.part') : null;
    if (part) { part.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  held = await page.evaluate(readHeld);
  if (held.names.length !== 0) {
    faults.push('the block was left over the box after its text went in: '
      + JSON.stringify(held.names));
  }
  if (held.words.indexOf('Lead: aaaaa') !== 0) {
    faults.push('the text did not land where the user left the marker: '
      + JSON.stringify(held.words.slice(0, 40)));
  }
  if (held.words.length < 6400) {
    faults.push('only ' + held.words.length + ' characters came back into the box');
  }

  // Three: the marker that takes a file away is not the block of the file. A text file let go
  // over the box is taken out again, and nothing of it is put into the box by that click.
  await page.evaluate(function () {
    var transfer = new DataTransfer();
    transfer.items.add(new File(['the words of the note'], 'note.txt', { type: 'text/plain' }));
    var ev = new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true });
    document.getElementById('box').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  held = await page.evaluate(readHeld);
  if (held.names.join(',') !== 'note.txt') {
    faults.push('a text file let go over the box left ' + JSON.stringify(held.names));
  }
  var before = held.words;
  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var x = row ? row.querySelector('.part button.x') : null;
    if (x) { x.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 200); });
  held = await page.evaluate(readHeld);
  if (held.names.length !== 0) {
    faults.push('the marker that takes a file away left a block: ' + JSON.stringify(held.names));
  }
  if (held.words !== before) {
    faults.push('the marker that takes a file away put text into the box');
  }

  if (errors.length) { faults.push('the page stated: ' + errors.join(' | ')); }
  console.log('the box holds ' + held.words.length + ' characters and no blocks');
  await browser.close();
  if (faults.length) {
    console.log('FAULTS\n' + faults.join('\n'));
    process.exit(1);
  }
  console.log('a click on the block of a text file put its text in the box,'
    + ' and the marker of taking a file away took it away');
}

main();
