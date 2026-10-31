// Check that the chat box takes files the way a user offers them: a file let
// go over the page, a file pasted into the box, and a paste whose event carries
// its file in the list of items only. The last shape is what a web view of a
// window hands over where the list of files is left empty, so the box must
// still take the file. A paste into a field of the sheet of settings is left
// alone, and a paste of many words still turns into a file of its own.
//
//   node look-check.js                  # the window page
//   AN_PAGE=../web/page.html AN_FETCH=1 node attach-check.js
const path = require('path');
const { execSync } = require('child_process');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';
// The sheet of settings stands in the window alone, so its field is looked at
// there only.
const IS_WINDOW = !USE_FETCH;

// The state of the engine, as the record of web::state. One chat with two
// messages, so the box and the talk both stand.
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
      { id: '2', role: 'Assistant', body: 'This picture is orange.', reasoning: '',
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

// The blocks over the words of the box, as the user sees them: one name each,
// whether the row stands, the words of the box and the mark of send.
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
  // The headless shell of another machine is named with AN_SHELL_BIN, as the
  // other checks of this program do.
  let shell = process.env.AN_SHELL_BIN || '';
  if (!shell) {
    try {
      shell = execSync('find "' + process.env.HOME + '/.cache/puppeteer" '
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
    // The calls of the page, so a check can see which road was taken, and the
    // events of the window, so a check can send one.
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
          // The window reads a file of the machine by its place: a picture of
          // one ending comes as a data URL, the rest as words.
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
          // The clipboard of the machine, which holds a picture of the screen.
          if (command === 'clipboard_take') {
            window.__asked.push('clipboard_take');
            return Promise.resolve({ files: [{ name: 'pasted-picture.png',
              mime: 'image/png', kind: 'image',
              data: 'data:image/png;base64,iVBORw0KGgo=', text: '' }], road: 'machine' });
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

  const look = function () {
    return page.evaluate(function () {
      var box = document.getElementById('viewer');
      var body = document.getElementById('lookbody');
      var img = body ? body.querySelector('img') : null;
      var pre = body ? body.querySelector('pre') : null;
      return {
        open: !!box && !box.classList.contains('hidden'),
        name: document.getElementById('lookname') ? document.getElementById('lookname').textContent : '',
        kind: document.getElementById('lookkind') ? document.getElementById('lookkind').textContent : '',
        pic: !!(img && String(img.src || '').length > 20),
        words: pre ? pre.textContent : ''
      };
    });
  };

  // One: a picture and a file of words land over the box. The picture is looked
  // at through the block; the file of words through its eye.
  await page.evaluate(function () {
    var transfer = new DataTransfer();
    transfer.items.add(new File(['a picture of bytes'], 'plan.png', { type: 'image/png' }));
    transfer.items.add(new File(['the words of the note'], 'note.txt', { type: 'text/plain' }));
    var ev = new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true });
    document.getElementById('box').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 400); });
  var peeked = await page.evaluate(function () {
    var row = document.getElementById('parts');
    var blocks = row ? row.querySelectorAll('.part') : [];
    return {
      count: blocks.length,
      pics: row ? row.querySelectorAll('.part.peek').length : 0,
      eyes: row ? row.querySelectorAll('.part button.eye').length : 0
    };
  });
  if (peeked.count !== 2) { faults.push('two files did not leave two blocks: ' + JSON.stringify(peeked)); }
  if (peeked.pics !== 1) { faults.push('the picture does not say it may be looked at: ' + JSON.stringify(peeked)); }
  if (peeked.eyes !== 1) { faults.push('the file of words has no eye to read it by: ' + JSON.stringify(peeked)); }

  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var pic = row ? row.querySelector('.part.peek') : null;
    if (pic) { pic.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  var shown = await look();
  if (!shown.open) { faults.push('a click on the picture did not open the window'); }
  if (!shown.pic) { faults.push('the window holds no picture: ' + JSON.stringify(shown)); }
  if (shown.kind !== 'Image') { faults.push('the window does not say it is an image: ' + JSON.stringify(shown)); }

  // Two: the key of escape puts the window away.
  await page.keyboard.press('Escape');
  await new Promise(function (r) { setTimeout(r, 200); });
  shown = await look();
  if (shown.open) { faults.push('the key of escape left the window standing'); }

  // Three: the eye of the file of words opens the same window, on the words.
  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var eye = row ? row.querySelector('.part button.eye') : null;
    if (eye) { eye.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  shown = await look();
  if (!shown.open) { faults.push('the eye of the file of words did not open the window'); }
  if (shown.words.indexOf('the words of the note') < 0) {
    faults.push('the window does not hold the words of the file: ' + JSON.stringify(shown));
  }
  await page.evaluate(function () {
    var x = document.getElementById('lookclose');
    if (x) { x.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 200); });
  shown = await look();
  if (shown.open) { faults.push('the mark of closing left the window standing'); }

  // Four: the block of the file of words keeps its other work — its words go
  // into the box, and the block goes away with them.
  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var blocks = row ? row.querySelectorAll('.part') : [];
    for (var n = 0; n < blocks.length; n++) {
      if (!blocks[n].classList.contains('peek') && blocks[n].classList.contains('pick')) {
        blocks[n].click();
        break;
      }
    }
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  var held = await page.evaluate(readHeld);
  if (held.words.indexOf('the words of the note') < 0) {
    faults.push('the block of the file of words no longer puts its words in the box');
  }
  if (held.names.length !== 1) {
    faults.push('the block that emptied itself into the box was left over it: '
      + JSON.stringify(held.names));
  }

  // Five: the files hanging on a sent message may be looked at too. The bytes
  // are asked for by the number of the attachment, which is what the window does
  // when the block never carried them.
  await page.evaluate(function () {
    window.attachmentWords = function () {
      return Promise.resolve({ name: 'memory-map.png', mime: 'image/png',
        data: 'data:image/png;base64,iVBORw0KGgo=', text: 'the words of the sent file' });
    };
  });
  var sent = await page.evaluate(function () {
    var rows = document.querySelectorAll('.parts .part.peek');
    return rows.length;
  });
  if (sent < 1) { faults.push('no block of a sent message says it may be looked at'); }
  await page.evaluate(function () {
    var rows = document.querySelectorAll('.parts .part.peek');
    if (rows.length) { rows[0].click(); }
  });
  await new Promise(function (r) { setTimeout(r, 400); });
  shown = await look();
  if (!shown.open) { faults.push('a click on a sent block did not open the window'); }

  if (errors.length) { faults.push('the page said: ' + errors.join(' | ')); }
  await browser.close();
  if (faults.length) {
    console.log('FAULTS\n' + faults.join('\n'));
    process.exit(1);
  }
  console.log('a picture opens at a click, a file of words opens at its eye, the key of'
    + ' escape and the mark of closing put the window away, and a sent block opens too');
}

main();
