// Check that a picture the model was shown stands in the block of the run that
// opened it. The bytes of a picture are sent to the model and were never written
// into the block, so the user could only read the receipt of the run and take
// it on trust. Now the plate stands in the block under the name of the picture,
// with an eye at its top right and a tip that says the model can see the picture.
// A command of the terminal tool, and a run that opened no picture, keep the
// block they always had.
//
//   node view-check.js                  # the window page
//   AN_PAGE=../web/page.html AN_FETCH=1 node view-check.js
const path = require('path');
const { execSync } = require('child_process');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// A picture of 96 by 60, for the plate of a run. It is a real PNG, so the plate
// is of the size the user would look at and the eye can be measured against it.
const PIC = 'data:image/png;base64,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';

function stateOf(dark) {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A look at the cat' },
    chats: [{ id: 'c1', title: 'A look at the cat', repo: '', when: 'Today 09:04',
      active: true, preview: 'What does this look like?' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    carousel: { index: 0, count: 1, blank: false },
    messages: [
      { id: '1', role: 'You', body: 'What does this look like?', reasoning: '', reasoning_ms: 0,
        attachments: [{ id: 'a1', name: 'swatch.png', mime: 'image/png', image: true }],
        tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Assistant', body: 'Let me look at it.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      // The run that opened a picture for the model. The picture hangs on the
      // message of the run, which is how it goes to the model again.
      { id: '3', role: 'Tool', body: 'notes/cat.png', reasoning: '', reasoning_ms: 0,
        attachments: [{ id: 'a2', name: 'cat.png', mime: 'image/png', image: true }],
        tool: 'Opened an image: notes/cat.png (PNG, 640 by 480 pixels, 12 KiB). '
            + 'The image itself is sent to the model with this text.',
        mono: true, failed: false, tool_name: 'view_image', when: '09:05' },
      // A run that read no picture hangs nothing on itself.
      { id: '4', role: 'Tool', body: 'notes/notes.txt', reasoning: '', reasoning_ms: 0,
        attachments: [],
        tool: "'notes.txt' is not an image this program can send.",
        mono: true, failed: true, tool_name: 'view_image', when: '09:05' },
      // A command, which never has a picture of its own.
      { id: '5', role: 'Tool', body: 'ls notes', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: 'cat.png\nnotes.txt', mono: true, failed: false,
        tool_name: 'run_terminal', when: '09:05' },
      { id: '6', role: 'Assistant', body: 'The cat is asleep on the keyboard.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:06' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [],
    thinking: { mode: 'off', available: true, modes: ['off'] },
    // The blocks of the runs stand open, for the picture is in the body of one.
    chatting: { collapse_thinking: false, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: !!dark, data_dir: '/tmp/autumn-natter',
  };
}

// The blocks of the runs, in the order they stand in the chat: the tool each
// one is, how many plates it holds, and what is in the first plate of one.
function readRuns() {
  var out = [];
  var panels = document.querySelectorAll('.tool');
  for (var n = 0; n < panels.length; n++) {
    var panel = panels[n];
    var plates = panel.querySelectorAll('.tshot');
    var first = plates[0] || null;
    var img = first ? first.querySelector('img.pic') : null;
    var eye = first ? first.querySelector('.seen') : null;
    var box = first ? first.getBoundingClientRect() : null;
    var eyeBox = eye ? eye.getBoundingClientRect() : null;
    out.push({
      head: (panel.querySelector('.cmd') || {}).textContent || '',
      closed: panel.classList.contains('closed'),
      shots: plates.length,
      row: !!first && String(first.className).split(' ')[0] === 'tshot',
      src: img ? String(img.getAttribute('src') || '') : '',
      alt: img ? String(img.getAttribute('alt') || '') : '',
      title: eye ? String(eye.getAttribute('title') || '') : '',
      aria: eye ? String(eye.getAttribute('aria-label') || '') : '',
      d: eye && eye.querySelector('svg path')
        ? String(eye.querySelector('svg path').getAttribute('d') || '') : '',
      // The eye stands at the top right of the plate, and within it.
      corner: (box && eyeBox)
        ? (eyeBox.right <= box.right + 0.6 && eyeBox.top >= box.top - 0.6
           && (box.right - eyeBox.right) < box.width / 2
           && (eyeBox.top - box.top) < box.height / 2)
        : false,
      seen: !!first && first.offsetWidth > 0 && first.offsetHeight > 0,
      order: [],
    });
    var space = panel.querySelector('.tspace');
    if (space) {
      for (var c = space.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 1) { out[out.length - 1].order.push(c.className); }
      }
    }
  }
  return out;
}

async function main() {
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
  await page.setViewport({ width: 1180, height: 1000 });
  const faults = [];
  const errors = [];
  page.on('pageerror', function (e) { errors.push(String(e)); });

  await page.evaluateOnNewDocument(function (state, pic, useFetch) {
    // Every ask for the bytes of a picture is counted, so a check can say that
    // the bytes of one picture are asked for once and then kept.
    window.__att = [];
    if (useFetch) {
      window.fetch = function (url) {
        var road = String(url);
        if (road.indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(state); } });
        }
        if (road.indexOf('/api/attachment') === 0) {
          window.__att.push(road);
          return Promise.resolve({ json: function () {
            return Promise.resolve({ name: 'cat.png', mime: 'image/png', data: pic, text: '' });
          } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      return;
    }
    window.__TAURI__ = {
      core: {
        invoke: function (command, args) {
          if (command === 'get_state') { return Promise.resolve(state); }
          if (command === 'get_config') { return Promise.resolve({}); }
          if (command === 'get_attachment') {
            window.__att.push(String((args && args.id) || ''));
            return Promise.resolve({ name: 'cat.png', mime: 'image/png', data: pic, text: '' });
          }
          return Promise.resolve({ ok: true });
        }
      },
      event: {
        listen: function () { return Promise.resolve(function () {}); }
      }
    };
  }, stateOf(false), PIC, USE_FETCH);

  await page.goto('file://' + path.resolve(PAGE));
  await new Promise(function (r) { setTimeout(r, 600); });

  const runs = await page.evaluate(readRuns);
  const named = {};
  for (var n = 0; n < runs.length; n++) { named[runs[n].head] = runs[n]; }

  // One: the block of the run that opened a picture holds the picture.
  const seenRun = runs.filter(function (one) {
    return one.head.indexOf('Looking at an image: notes/cat.png') === 0;
  })[0];
  if (!seenRun) {
    faults.push('no block of the run that looked at an image: '
      + JSON.stringify(runs.map(function (o) { return o.head; })));
  } else {
    if (seenRun.shots !== 1) {
      faults.push('the block of the run holds ' + seenRun.shots + ' plates, not one');
    }
    if (seenRun.src !== PIC) {
      faults.push('the plate of the run holds no picture of the model: ' + seenRun.src.slice(0, 40));
    }
    if (seenRun.alt !== 'cat.png') {
      faults.push('the picture of the run is named "' + seenRun.alt + '", not cat.png');
    }
    if (!seenRun.seen) {
      faults.push('the picture of the run takes up no room, so the user cannot see it');
    }
    // Two: the eye at the top right, and the tip that says what it means.
    if (!/model can see/i.test(seenRun.title)) {
      faults.push('the tip of the eye says "' + seenRun.title + '"');
    }
    if (seenRun.aria !== seenRun.title) {
      faults.push('the eye is named to a user of the screen as "' + seenRun.aria + '"');
    }
    if (seenRun.d.indexOf('M1.9 8c') !== 0) {
      faults.push('the mark of the eye is not an eye: ' + seenRun.d.slice(0, 24));
    }
    if (!seenRun.corner) {
      faults.push('the eye does not stand at the top right of the plate');
    }
    // Three: the plate stands under the name of the picture and above the
    // receipt, so the block reads name, picture, result.
    if (seenRun.order.join(',') !== 'lab,cmd-text,tshots,lab,out') {
      faults.push('the parts of the block stand in the order '
        + JSON.stringify(seenRun.order));
    }
  }

  // Four: a run that read no picture, and a command, keep the block they had.
  const blind = runs.filter(function (one) {
    return one.head.indexOf('Looking at an image: notes/notes.txt') === 0;
  })[0];
  if (!blind) {
    faults.push('the run that read no picture is gone from the chat');
  } else if (blind.shots !== 0) {
    faults.push('a run that read no picture holds ' + blind.shots + ' plates');
  }
  const command = runs.filter(function (one) {
    return one.head.indexOf('ls notes') === 0;
  })[0];
  if (!command) {
    faults.push('the block of the command is gone from the chat');
  } else if (command.shots !== 0) {
    faults.push('a command holds ' + command.shots + ' plates');
  }

  // Five: the eye stands on what the model was shown, and not on the picture
  // the user hung on their own message.
  const onBubble = await page.evaluate(function () {
    var bubble = document.querySelector('.bubble .part.image');
    return { held: !!bubble, eyes: document.querySelectorAll('.bubble .seen').length };
  });
  if (!onBubble.held) {
    faults.push('the picture of the user is gone from their bubble');
  } else if (onBubble.eyes !== 0) {
    faults.push('the eye stands on ' + onBubble.eyes + ' of the blocks of the user');
  }

  // Six: the bytes of a picture are asked for once and then kept. A block of
  // the same run made again, as a poll makes it, takes the picture it was given
  // before and asks for nothing.
  const asked = await page.evaluate(function () { return (window.__att || []).slice(); });
  if (asked.length === 0) {
    faults.push('the plate of the run never asked for its bytes');
  }
  const ofCat = asked.filter(function (one) { return String(one).indexOf('a2') >= 0; });
  if (ofCat.length !== 1) {
    faults.push('the bytes of the picture of the run were asked for ' + ofCat.length
      + ' times: ' + JSON.stringify(ofCat));
  }
  const redraw = await page.evaluate(function () {
    var again = toolBox((cur.messages || [])[2]);
    var img = again.querySelector('.tshot img.pic');
    return { held: !!img, src: img ? String(img.getAttribute('src') || '') : '',
      eyes: again.querySelectorAll('.seen').length };
  });
  if (!redraw || !redraw.held) {
    faults.push('a block made again holds no picture at once: ' + JSON.stringify(redraw));
  } else if (redraw.src !== PIC) {
    faults.push('a block made again holds the wrong picture');
  } else if (redraw.eyes !== 1) {
    faults.push('a block made again holds ' + redraw.eyes + ' eyes, not one');
  }
  const after = await page.evaluate(function () { return (window.__att || []).length; });
  if (after !== asked.length) {
    faults.push('a block made again asked for the bytes a second time');
  }

  // Seven: the fold of the block puts the picture away with the rest of the body.
  const folded = await page.evaluate(function () {
    var panels = document.querySelectorAll('.tool');
    for (var n = 0; n < panels.length; n++) {
      var plate = panels[n].querySelector('.tshot');
      if (!plate) { continue; }
      panels[n].querySelector('.thead').click();
      return { closed: panels[n].classList.contains('closed'),
        room: plate.offsetWidth > 0 && plate.offsetHeight > 0 };
    }
    return { closed: false, note: 'no plate to fold' };
  });
  if (!folded.closed) {
    faults.push('the head of the block did not fold: ' + JSON.stringify(folded));
  }
  if (folded.room) {
    faults.push('the picture of a folded block still takes up room');
  }

  if (errors.length) { faults.push('page faults: ' + errors.join(' | ')); }
  await browser.close();
  console.log(faults.length ? 'FAIL\n  ' + faults.join('\n  ') : 'ok  view-check'
    + (USE_FETCH ? ' (the page of the remote mode)' : ' (the window)'));
  process.exit(faults.length ? 1 : 0);
}

main().catch(function (e) { console.log('FAIL ' + (e && e.stack || e)); process.exit(1); });
