// Check that the chat composer takes files the way a user proposes them: a file let
// go over the page, a file pasted into the box, and a paste whose event carries
// its file in the list of items only. The last form is what a web view of a
// window pointers over where the list of files is left empty, so the box must
// still take the file. A paste into a field of the settings panel is left
// alone, and a paste of a lot of text still turns into a file of its own.
//
//   node attach-check.js                  # the window page
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

  // One: two files let go over the box, as they come from the file manager of the computer.
  // Their order is the order of the blocks.
  const letGo = await page.evaluate(function () {
    var transfer = new DataTransfer();
    transfer.items.add(new File(['bytes'], 'photo.png', { type: 'image/png' }));
    transfer.items.add(new File(['the words of the note'], 'note.txt', { type: 'text/plain' }));
    var ev = new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true });
    document.getElementById('box').dispatchEvent(ev);
    return { prevented: ev.defaultPrevented,
      look: document.getElementById('box').className };
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  let held = await page.evaluate(readHeld);
  if (held.names.join(',') !== 'photo.png,note.txt') {
    faults.push('a file let go over the box left ' + JSON.stringify(held.names)
      + ', and two blocks were asked for');
  }
  if (held.hidden) { faults.push('the row of blocks is hidden while it holds files'); }
  // The box reports where a file would land while the file is on its way over it, and reports
  // nothing of the type once the file has landed.
  if (letGo.look.indexOf('drop') >= 0) {
    faults.push('the box still reports a file may be let go, after it landed: ' + letGo.look);
  }
  // A file let go must be stopped, or the web view leaves the chat for the file itself, and the
  // conversation is removed.
  if (!letGo.prevented) {
    faults.push('a file let go over the page was not stopped');
  }

  // Two: the marker of a block takes one file away again, so the next look starts from one
  // block.
  await page.evaluate(function () {
    var row = document.getElementById('parts');
    var x = row ? row.querySelector('.part button.x') : null;
    if (x) { x.click(); }
  });
  await new Promise(function (r) { setTimeout(r, 120); });
  held = await page.evaluate(readHeld);
  if (held.names.length !== 1) {
    faults.push('the marker that takes a file away left ' + JSON.stringify(held.names));
  }

  // Three: a paste into the box, the way Ctrl+V of a file comes.
  await page.evaluate(function () {
    var transfer = new DataTransfer();
    transfer.items.add(new File(['bytes'], 'clipboard-shot.png', { type: 'image/png' }));
    var ev = new ClipboardEvent('paste', {
      clipboardData: transfer, bubbles: true, cancelable: true
    });
    document.getElementById('draft').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  held = await page.evaluate(readHeld);
  if (held.names.join(',') !== 'note.txt,clipboard-shot.png') {
    faults.push('a paste of a file left ' + JSON.stringify(held.names));
  }

  // Four: the form a web view of a window gives, where the list of files is empty and the file
  // is in the list of items only.
  await page.evaluate(function () {
    var ev = new Event('paste', { bubbles: true, cancelable: true });
    ev.clipboardData = {
      files: [],
      getData: function () { return ''; },
      items: [{ kind: 'file', getAsFile: function () {
        return new File(['text only'], 'items-only.md', { type: 'text/markdown' });
      } }]
    };
    document.getElementById('draft').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 300); });
  held = await page.evaluate(readHeld);
  if (held.names.indexOf('items-only.md') < 0) {
    faults.push('a paste that carries its file in the list of items only left '
      + JSON.stringify(held.names));
  }

  // Five: a paste of a lot of text turns into a file of its own, and the box keeps its form.
  await page.evaluate(function () {
    var transfer = new DataTransfer();
    var long = '';
    for (var n = 0; n < 6500; n++) { long += 'a'; }
    transfer.setData('text/plain', long);
    var ev = new ClipboardEvent('paste', {
      clipboardData: transfer, bubbles: true, cancelable: true
    });
    document.getElementById('draft').dispatchEvent(ev);
  });
  await new Promise(function (r) { setTimeout(r, 200); });
  held = await page.evaluate(readHeld);
  if (held.names.filter(function (n) { return n.indexOf('pasted-text') === 0; }).length !== 1) {
    faults.push('a paste of a lot of text did not become a file: ' + JSON.stringify(held.names));
  }
  if (held.words.length > 0) {
    faults.push('the text of a big paste was in the box instead of in a file');
  }

  // Six: the round marker knows that a message of files and no text is one.
  if (held.send.indexOf('live') < 0) {
    faults.push('the round marker does not know that files are something to send');
  }

  if (IS_WINDOW) {
    // Seven: the path of the window itself. The web view of the system on Linux never brings a
    // file let go down to the page, so the program records it from the side of the window -
    // that a drag is above it, and that a file was let go, with the places of the files. The
    // page then asks the window to read them, and lets a drop of its own type alone, so nothing
    // lands twice.
    await page.evaluate(function () { window.__send__('window-drop', { kind: 'over' }); });
    const over = await page.evaluate(function () {
      return document.getElementById('box').className;
    });
    if (over.indexOf('drop') < 0) {
      faults.push('the window stated a drag is above it, and the box reports nothing'
        + ' of the kind: ' + over);
    }
    await page.evaluate(function () {
      window.__send__('window-drop',
        { kind: 'down', paths: ['/home/reader/one.png', '/home/reader/note.md'] });
    });
    await new Promise(function (r) { setTimeout(r, 300); });
    held = await page.evaluate(readHeld);
    if (held.names.slice(-2).join(',') !== 'one.png,note.md') {
      faults.push('a file let go over the window left ' + JSON.stringify(held.names)
        + ', and two places were given to the window');
    }
    const afterDown = await page.evaluate(function () {
      return document.getElementById('box').className;
    });
    if (afterDown.indexOf('drop') >= 0) {
      faults.push('the box still reports a file may be let go, after the window took it: '
        + afterDown);
    }
    // The same file let go over the page now must not be hung on a second time.
    await page.evaluate(function () {
      var transfer = new DataTransfer();
      transfer.items.add(new File(['bytes'], 'twice.png', { type: 'image/png' }));
      document.getElementById('box').dispatchEvent(
        new DragEvent('drop', { dataTransfer: transfer, bubbles: true, cancelable: true }));
    });
    await new Promise(function (r) { setTimeout(r, 200); });
    held = await page.evaluate(readHeld);
    if (held.names.filter(function (n) { return n === 'twice.png'; }).length > 0) {
      faults.push('a file the window had taken was hung on the message a second time: '
        + JSON.stringify(held.names));
    }

    // Eight: a paste that brought nothing asks the clipboard of the computer, which the window
    // reads through the toolkit of the system. An image of the clipboard is an image, and not a
    // file of bytes with no ending.
    await page.evaluate(function () {
      var ev = new Event('paste', { bubbles: true, cancelable: true });
      ev.clipboardData = { files: [], items: [], getData: function () { return ''; } };
      document.getElementById('draft').dispatchEvent(ev);
    });
    await new Promise(function (r) { setTimeout(r, 300); });
    held = await page.evaluate(readHeld);
    if (held.names.indexOf('pasted-picture.png') < 0) {
      faults.push('a paste of nothing at all did not ask the clipboard of the computer: '
        + JSON.stringify(held.names));
    }
    const asked = await page.evaluate(function () { return window.__asked; });
    if (asked.indexOf('clipboard_take') < 0) {
      faults.push('the paste asked the web view of the window instead of the computer');
    }
    if (asked.filter(function (a) { return a.indexOf('take_files:') === 0; }).length === 0) {
      faults.push('the drop of the window never requested the files of its places');
    }

    // Nine: a field of the stylesheet keeps its paste. A heap of text pasted into a field of
    // the stylesheet must be in the field, and not hang a block on the message of the chat.
    await page.evaluate(function () {
      var b = document.getElementById('settingsbtn');
      if (b) { b.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 400); });
    const before = await page.evaluate(readHeld);
    const prevented = await page.evaluate(function () {
      var transfer = new DataTransfer();
      var long = '';
      for (var n = 0; n < 6500; n++) { long += 'a'; }
      transfer.setData('text/plain', long);
      var ev = new ClipboardEvent('paste', {
        clipboardData: transfer, bubbles: true, cancelable: true
      });
      document.getElementById('fNewRepo').dispatchEvent(ev);
      return ev.defaultPrevented;
    });
    await new Promise(function (r) { setTimeout(r, 200); });
    const after = await page.evaluate(readHeld);
    if (prevented) {
      faults.push('a paste into a field of the stylesheet was taken by the chat composer');
    }
    if (after.names.length !== before.names.length) {
      faults.push('a paste into a field of the stylesheet hung a block on the message: '
        + JSON.stringify(after.names));
    }
  }

  if (errors.length) {
    faults.push('the page stated: ' + errors.join(' | '));
  }

  console.log('the blocks over the box, at the last look: ' + JSON.stringify(held.names));
  await browser.close();
  if (faults.length) {
    console.log('FAULTS\n' + faults.join('\n'));
    process.exit(1);
  }
  console.log('the chat composer took the file let go, the file pasted, the file of the'
    + ' items list only, and the paste of a lot of text'
    + (IS_WINDOW ? ', and the window took its own drop and its own clipboard' : ''));
}

main();
