// The five tools of the composition suite, as the chat shows them, checked in both pages of the
// program with no engine behind it.
//
// The engine names every call of the suite itself - what the call does, and which composition it
// works on - in the sentence it keeps as the running text of the block. Four things are checked,
// in each page:
// * the five names of the suite are shapes of the page, each with a marker of its own, and no
//   shape carries a head of its own, for a sentence of the page in front of the sentence of the
//   engine stated the same twice; none of the three names the suite gave up remains in the page
//   at all, and the badge that switches the suite on speaks of five tools and of the folder of
//   the still frames;
// * the header of a finished run is the sentence the engine sent, word for word, whether the run
//   returned what was asked for or failed, and the marker of the block is the marker of its shape;
// * a call that drew a still of the work in progress hangs that frame on its block under the name
//   of the file it was written into, as the images of the view tool are hung;
// * the row of a call being written is headed by the sentence the engine sent for it, and by
//   nothing else, and says of itself that it is being written.
//
// node infographic-check.js
// AN_PAGE=web/page.html node infographic-check.js
'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'face', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'page', file: path.join(__dirname, '..', 'web', 'page.html') },
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The five names of the suite, with the sentence the engine puts on the block of each. `said` is
// what `infographic::shown` answers for the arguments named in `args`.
const NAMED = [
  { name: 'infographic_project', args: '{"project":"bunny","canvas":"1080p"}',
    said: 'Setting up a composition (bunny)' },
  { name: 'infographic_layer', args: '{"project":"bunny","one":"head","preview":"now"}',
    said: 'Editing a layer (bunny)' },
  { name: 'infographic_cutout', args: '{"project":"bunny","image":"green.jpg"}',
    said: 'Previewing a key (bunny)' },
  { name: 'infographic_check', args: '{"project":"bunny","at":2}',
    said: 'Checking a composition (bunny)' },
  { name: 'infographic_render', args: '{"project":"bunny","fps":24}',
    said: 'Rendering a video (bunny)' },
];

// The names the suite gave up when it was overhauled, and the two words a page of the program is
// asked to speak of the suite for: the count of the tools and the folder of the frames.
const GIVEN_UP = ['infographic_guide', 'infographic_plan', 'infographic_scene'];

// The frame the run of a layer drew, and the two results that are read off a real run: the still
// of the work in progress, with the seconds of it and the folder it was written into, and the
// check that found faults.
const STILL_FILE = 'bunny-at-2s.png';
const STILL_RESULT = 'The layer "head" of the composition "bunny" is changed.\n'
  + '3 layers: head [text], plate [picture], film [video]\n'
  + '2 s long, 1920x1080 at 24 fps.\n'
  + '1 stills of the work in progress are returned, at 2 s. They are in '
  + 'infographics/bunny/frames.';
const FAULT_RESULT = '2 faults in the composition, 6 layers, 6 s long:\n'
  + 'head: the words stand 14 px over the height of the box.\n'
  + 'photo: there is no file at notes/plate.jpg\n'
  + '1 stills of the work in progress are returned, at 2 s. They are in '
  + 'infographics/bunny/frames.';
const SAVED_RESULT = 'The composition "bunny" is saved in infographics/bunny.\n'
  + '3 layers: head [text], plate [picture], film [video]\n'
  + '6 s long, 1920x1080 at 24 fps.';
const RENDER_RESULT = 'The video is written into infographics/bunny/bunny.mp4.\n'
  + '145 frames of 6 s at 24 fps, 8.4 MB.';

// A frame of a film, as the bytes a run of the suite answered with: a sky over a field, with a
// band across it, so a plate of it in a screenshot is read as a frame and not as a grey box.
function framePng(width, height) {
  const raw = Buffer.alloc(height * (1 + width * 3));
  let at = 0;
  for (let y = 0; y < height; y++) {
    raw[at++] = 0;
    for (let x = 0; x < width; x++) {
      const sky = y < height * 0.58;
      const band = !sky && y < height * 0.70;
      let colour = sky ? [46, 58, 74] : [24, 30, 26];
      if (band) { colour = [196, 92, 38]; }
      raw[at++] = colour[0];
      raw[at++] = colour[1];
      raw[at++] = colour[2];
    }
  }
  const chunk = (kind, body) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(body.length, 0);
    head.write(kind, 4, 'ascii');
    const crc = crc32(Buffer.concat([Buffer.from(kind, 'ascii'), body]));
    const tail = Buffer.alloc(4);
    tail.writeUInt32BE(crc, 0);
    return Buffer.concat([head, body, tail]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(width, 0);
  head.writeUInt32BE(height, 4);
  head[8] = 8;
  head[9] = 2;
  return 'data:image/png;base64,' + Buffer.concat([
    Buffer.from('\x89PNG\r\n\x1a\n', 'binary'),
    chunk('IHDR', head),
    chunk('IDAT', zlib.deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]).toString('base64');
}

let TABLE = null;
function crc32(buf) {
  if (!TABLE) {
    TABLE = [];
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) { c = (c & 1) ? (0xedb88320 ^ (c >>> 1)) : (c >>> 1); }
      TABLE[n] = c >>> 0;
    }
  }
  let crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) { crc = TABLE[(crc ^ buf[n]) & 0xff] ^ (crc >>> 8); }
  return (crc ^ 0xffffffff) >>> 0;
}

function said(id, body) {
  return {
    id: id, role: 'You', body: body, reasoning: '', reasoning_ms: 0, attachments: [],
    tool: '', tool_name: '', mono: false, failed: false, cancelled: false, stopped: false,
    can_continue: false, when: '09:0' + id, downloads: [], displayed: [], suggestions: [],
  };
}

// One run of a tool of the suite, as the state carries it: `body` is the sentence the engine sent
// for the call, and `tool` is what came back, for the header of the block is the sentence and the
// result of the run is under its own label.
function run(id, one, result, given) {
  return {
    id: id, role: 'Tool', body: one.said, reasoning: '', reasoning_ms: 0,
    attachments: (given && given.pictures) || [],
    tool: result, tool_name: one.name, mono: true, failed: !!(given && given.failed),
    cancelled: false, stopped: false, can_continue: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
  };
}

// The chat of a film being made: the five runs of the suite, the last of them failed, and the
// frame of the work in progress hung on the run of the layer that drew it.
function stateOf(frame) {
  const messages = [said('1', 'Make a film of the notes, with the presenter keyed out.')];
  NAMED.forEach(function (one, n) {
    const still = one.name === 'infographic_layer';
    const failed = one.name === 'infographic_check';
    const result = still ? STILL_RESULT
      : failed ? FAULT_RESULT
        : one.name === 'infographic_render' ? RENDER_RESULT : SAVED_RESULT;
    messages.push(run(String(2 + n), one, result, {
      failed: failed,
      pictures: still
        ? [{ id: 'f1', name: STILL_FILE, mime: 'image/png', image: true }]
        : [],
    }));
  });
  return {
    rev: 1, unchanged: false, theme: 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A film of the notes', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A film of the notes', repo: '', when: 'Today 09:04',
      active: true, preview: 'Make a film of the notes.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: messages,
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    // The blocks of the calls are left open, for the frame and the result of a call are read there.
    chatting: { collapse_thinking: false, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, search: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    rag: { enabled: false, files: 0, chunks: 0, pending: 0, ready: false, vision: false },
    queue: { chat: 'c1', held: false, items: [], planned: [] },
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// The two doors of the program, so the one check runs against the window and against the page.
// `stillName` is the name of the frame file the records of the chat answer with.
function stubs(state, stillName) {
  window.__held = state;
  window.__post = function (patch) {
    Object.assign(window.__held, patch);
    window.__held.rev += 1;
    window.__held.unchanged = false;
  };
  const answer = function (body) {
    return Promise.resolve({ json: function () { return Promise.resolve(body); } });
  };
  window.fetch = function (url) {
    const name = String(url);
    if (name.indexOf('attachment') >= 0) {
      return answer({ ok: true, name: stillName, data: window.__frame });
    }
    if (name.indexOf('state') >= 0) { return answer(window.__held); }
    return answer({ ok: true });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command) {
        if (command === 'get_state') { return Promise.resolve(window.__held); }
        if (command === 'get_attachment') {
          return Promise.resolve({ ok: true, name: stillName, data: window.__frame });
        }
        return Promise.resolve({});
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The blocks of the calls, in the order they stand in the chat: the words of the header, the
// marker of the block, what the pill of the state says, and every frame hung on the block.
const LOOK = function () {
  var out = [];
  var panels = document.querySelectorAll('.tool');
  for (var n = 0; n < panels.length; n++) {
    var panel = panels[n];
    var plates = panel.querySelectorAll('.tshot');
    var held = [];
    for (var p = 0; p < plates.length; p++) {
      var pic = plates[p].querySelector('img.pic');
      held.push({
        tip: String(plates[p].getAttribute('data-tip') || ''),
        alt: pic ? String(pic.getAttribute('alt') || '') : '',
      });
    }
    var head = panel.querySelector('.thead .cmd');
    var drawn = panel.querySelector('.thead svg path');
    var pill = panel.querySelector('.thead .state');
    out.push({
      head: head ? String(head.textContent || '').replace(/\s+/g, ' ').trim() : '',
      mark: drawn ? String(drawn.getAttribute('d') || '') : '',
      pill: pill ? String(pill.textContent || '').trim() : '',
      bad: panel.classList.contains('bad'),
      writing: panel.classList.contains('write'),
      plates: held,
    });
  }
  var shapes = {};
  Object.keys(window.TOOL_SHAPES || {}).forEach(function (key) {
    shapes[key] = { mark: window.TOOL_SHAPES[key].mark || '',
      head: window.TOOL_SHAPES[key].head || '', done: window.TOOL_SHAPES[key].done || '' };
  });
  return { blocks: out, shapes: shapes };
};

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const frame = framePng(240, 135);
  for (const one of PAGES) {
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };
    const faults = [];

    // The words of the page itself: the three names the suite gave up, and the badge that speaks
    // of the suite.
    const words = fs.readFileSync(one.file, 'utf8');
    GIVEN_UP.forEach(function (gone) {
      if (words.indexOf(gone) >= 0) {
        note('the name the suite gave up remains in ' + one.name + ': ' + gone);
      }
    });
    const badge = words.indexOf('Create infographics');
    if (badge < 0) {
      note('the page does not name the suite of the composition tools');
    } else {
      const said = words.slice(badge, badge + 1400);
      if (said.indexOf('Five tools') < 0) {
        note('the badge of the suite does not say how many tools it is: '
          + said.slice(0, 200));
      }
      if (said.indexOf('frames') < 0) {
        note('the badge of the suite says nothing of the folder the still frames are written into');
      }
    }

    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 900 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString()
      + '; window.__LOOK = ' + LOOK.toString() + ';');
    await page.evaluateOnNewDocument('window.__frame = ' + JSON.stringify(frame)
      + '; window.__stubs(' + JSON.stringify(stateOf(frame)) + ',' + JSON.stringify(STILL_FILE)
      + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(700);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }

    let made = await page.evaluate('window.__LOOK()');
    const blocks = made.blocks;
    if (blocks.length < NAMED.length) {
      note('the chat holds ' + blocks.length + ' blocks of calls, of ' + NAMED.length
        + ' that were asked for');
    } else {
      NAMED.forEach(function (named, n) {
        const block = blocks[n];
        const shape = made.shapes[named.name];
        if (!shape) {
          note('the page has no shape for ' + named.name);
          return;
        }
        // The engine named the call, so nothing of the page stands in front of its words, and
        // nothing of them is dropped for the words of the result.
        if (block.head !== named.said) {
          note('the header of ' + named.name + ' says "' + block.head + '" against the sentence '
            + 'the engine sent, "' + named.said + '"');
        }
        if (shape.head || shape.done) {
          note('the shape of ' + named.name + ' carries a head of its own: "' + shape.head
            + '" / "' + shape.done + '"');
        }
        if (!shape.mark) {
          note('the shape of ' + named.name + ' has no marker');
        }
        if (block.mark !== shape.mark) {
          note('the marker of the block of ' + named.name + ' is not the marker of its shape');
        }
      });
      // The call that drew a frame of the work in progress hangs it on its block, named for the
      // file the frame was written into, and the bytes of it arrive through the door of the
      // records of the chat.
      const still = blocks[1];
      if (still.plates.length !== 1) {
        note('the call that drew a still hung ' + still.plates.length + ' frames on its block');
      } else if (still.plates[0].tip.indexOf(STILL_FILE) < 0) {
        note('the frame on the block is not named for the file it is: "' + still.plates[0].tip
          + '"');
      } else if (still.plates[0].alt.indexOf(STILL_FILE) < 0) {
        note('the bytes of the frame never arrived: the plate still says "' + still.plates[0].alt
          + '"');
      }
      // A check that found faults failed, so its block says so, and still heads itself with the
      // sentence of the call rather than with the first fault.
      const check = blocks[3];
      if (!check.bad) { note('a check that found faults is not marked as failed'); }
      if (check.pill !== 'failed') {
        note('the pill of a failed check says "' + check.pill + '"');
      }
      if (check.head !== NAMED[3].said) {
        note('a failed check is headed by what it found: "' + check.head + '"');
      }
      if (blocks[4].pill !== 'done') {
        note('the pill of a render that ended says "' + blocks[4].pill + '"');
      }
    }
    await page.evaluate(function (where) {
      var thread = document.getElementById('thread');
      if (thread) { thread.scrollTop = where === 'top' ? 0 : thread.scrollHeight; }
    }, 'top');
    await sleep(200);
    await page.screenshot({ path: path.join(__dirname, 'infographic-' + one.tag + '-1-runs.png') });
    await page.evaluate(function () {
      var thread = document.getElementById('thread');
      if (thread) { thread.scrollTop = 620; }
    });
    await sleep(200);
    await page.screenshot({ path: path.join(__dirname, 'infographic-' + one.tag + '-2-frames.png') });

    // The row of a call being written: the engine named it, so the header carries that sentence
    // and nothing else, and the pill says that the text of the call is still coming.
    await page.evaluate(function (rows) {
      window.__post({
        generating: true, busy: true, status: 'The model is writing a call.',
        live: 'The heading is mended after that.', reasoning: '',
        live_tools: rows,
      });
    }, [
      { name: 'infographic_layer', command: NAMED[1].args, purpose: '',
        target: NAMED[1].said, done: false },
      { name: 'infographic_render', command: '{"project":"bunny"', purpose: '',
        target: NAMED[4].said, done: true },
    ]);
    await sleep(500);
    made = await page.evaluate('window.__LOOK()');
    const rows = made.blocks.slice(NAMED.length);
    if (rows.length < 2) {
      note('the call being written is not drawn as a row: ' + rows.length + ' of 2');
    } else {
      if (!rows[0].writing) { note('the row of a call being written is not known as one'); }
      if (rows[0].head !== NAMED[1].said) {
        note('the row of a call being written says "' + rows[0].head + '" against the sentence '
          + 'the engine sent, "' + NAMED[1].said + '"');
      }
      if (rows[0].pill !== 'writing') {
        note('the pill of a call being written says "' + rows[0].pill + '"');
      }
      if (rows[1].pill !== 'written') {
        note('the pill of a call the model has moved on from says "' + rows[1].pill + '"');
      }
    }
    await page.evaluate(function () {
      var thread = document.getElementById('thread');
      if (thread) { thread.scrollTop = thread.scrollHeight; }
    });
    await sleep(200);
    await page.screenshot({ path: path.join(__dirname, 'infographic-' + one.tag + '-3-writing.png') });

    if (faults.length) { note('page faults at the end: ' + faults.join(' | ')); }
    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
    await page.close();
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the five tools of the composition suite are shapes of both pages, every block of a '
    + 'call is headed by the sentence the engine sent with nothing in front of it, a call that '
    + 'drew a still of the work in progress hangs that frame on its block under the name of its '
    + 'file, and a call being written is headed by the same kind of sentence');
}

main().catch((e) => { console.error(e); process.exit(1); });
