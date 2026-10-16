// Count the frames of the animations of the row of previews and of the slide
// of a chat, with a chat that holds a lot of content behind it.
//
// The page is drawn in a headless browser with a fake bridge, the way
// page-shot.js does it. A counter of frames stands in the page, and it is
// read after every scene. The scenes are:
//
//   row-small    the row comes up over a chat with little in it
//   row-big      the row comes up over a chat with a lot in it
//   slide-small  a chat with little in it is slid in from the row
//   slide-big    a chat with a lot in it is slid in from the row
//
// The box has no desktop, so the frames are counted with the software
// renderer. A fault of the page shows as a low count, and it shows more than
// on a screen with a card, and not less.

'use strict';

const path = require('path');
const { execSync } = require('child_process');

const page = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');

// One message of the talk.
function said(id, role, body) {
  return { id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
           tool: '', mono: false, failed: false, when: '09:4' + (id % 10) };
}

// A lot of content of a chat: long turns with code and lists, so the thread
// stands far taller than the window. AN_TURNS sets the turns of the big talks.
function bigTalk(tag) {
  const turns = Number(process.env.AN_TURNS || 120);
  const out = [];
  for (let n = 0; n < turns; n++) {
    out.push(said(tag + n, 'You', 'Step ' + n + ': what stands here, and why does it stand?'));
    out.push(said(tag + n, 'Assistant',
      '## Part ' + n + '\n\n' +
      'A line of the talk, ' + tag + '-' + n + '. It holds words enough to fill a '
      + 'panel of its own, and a list of its own, and a block of code of its own.\n\n' +
      '- the first mark of the list\n- the second mark of the list\n- the third mark of the list\n\n' +
      '```rust\nfn part_' + n + '(x: i32) -> i32 {\n    let y = x * 2 + ' + n + ';\n    y + 1\n}\n```'));
  }
  return out;
}

const SMALL = [said('s1', 'You', 'A short question.'),
               said('s2', 'Assistant', 'A short answer of its own.')];

// The talks of the place, by id. c1 and c9 hold little, and the rest hold a
// lot. c1 is the chat that stands open at the start.
const TALKS = {
  c1: SMALL,
  c2: bigTalk('a'),
  c3: bigTalk('b'),
  c4: bigTalk('c'),
  c5: bigTalk('d'),
  c6: bigTalk('e'),
  c7: bigTalk('f'),
  c8: bigTalk('g'),
  c9: SMALL
};
const ORDER = ['c1', 'c2', 'c3', 'c4', 'c5', 'c6', 'c7', 'c8', 'c9'];

function baseState() {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'The small one' },
    chats: ORDER.map(function (id, n) {
      return { id: id, title: 'Chat ' + (n + 1), repo: '', when: 'Today 09:0' + (n % 10),
               active: id === 'c1',
               preview: TALKS[id].length > 2 ? 'A long talk of its own' : 'Nothing written yet' };
    }),
    home: false, greeting: '', overview: false,
    carousel: { index: 0, count: ORDER.length, blank: false },
    messages: TALKS.c1, live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    chapters: [],
    thinking: { mode: 'off', available: true, modes: ['off', 'low'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true, chapters: true },
    context: { used: '3,204', total: '8,192', ratio: 0.39 },
    generating: false, busy: false, code: false, sandbox: false,
    sandbox_available: true, sandbox_note: '', status: 'Ready', dark: false,
    data_dir: '/home/user/.local/share/autumn-natter'
  };
}



const sleep = function (ms) { return new Promise(function (r) { setTimeout(r, ms); }); };

async function main() {
  const puppeteer = require('puppeteer');
  let shell = '';
  try {
    shell = execSync('find "' + process.env.HOME + '/.cache/puppeteer" '
      + '-name chrome-headless-shell -type f | head -1').toString().trim();
  } catch (e) { shell = ''; }

  const throttle = Number(process.env.AN_THROTTLE || 1);
  const browser = await puppeteer.launch({
    headless: shell ? 'shell' : true,
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu',
      '--font-render-hinting=none', '--force-color-profile=srgb']
  });

  const results = [];

  async function scene(name, open) {
    const view = await browser.newPage();
    view.on('pageerror', function (e) { console.log('PAGEERROR ' + name, String(e)); });
    const held = baseState();
    const talks = JSON.stringify(TALKS);
    const order = JSON.stringify(ORDER);
    await view.evaluateOnNewDocument(function (heldJson, talksJson, orderJson) {
      // The state that a poll gives back. It stands in one place, and the
      // actions of the page move it, the way the engine does.
      const held = JSON.parse(heldJson);
      const talks = JSON.parse(talksJson);
      const order = JSON.parse(orderJson);
      window.__TAURI__ = { core: { invoke: function (command, args) {
        if (command === 'get_state') {
          return Promise.resolve(JSON.parse(JSON.stringify(held)));
        }
        if (command === 'action') {
          const name = args.path;
          if (name === 'open_chat' || name === 'roll') {
            let at = held.carousel.index;
            if (name === 'open_chat') {
              at = order.indexOf(args.body.id);
            } else {
              at = Math.max(0, Math.min(order.length - 1, at + args.body.step));
            }
            if (at >= 0) {
              const id = order[at];
              held.chat = { id: id, title: 'Chat ' + (at + 1) };
              held.messages = talks[id];
              held.carousel = { index: at, count: order.length, blank: false };
              held.chats.forEach(function (c, n) { c.active = n === at; });
              held.rev += 1;
              held.unchanged = false;
            }
          }
          return Promise.resolve({ ok: true });
        }
        return Promise.resolve({ ok: true });
      } } };
    }, JSON.stringify(held), talks, order);
    await view.setViewport({ width: 1180, height: 760, deviceScaleFactor: 1 });
    if (throttle > 1) {
      const client = await view.createCDPSession();
      await client.send('Emulation.setCPUThrottlingRate', { rate: throttle });
    }
    await view.goto('file://' + path.resolve(page), { waitUntil: 'networkidle0' });
    await sleep(700);

    await open(view);
    let report;
    try {
      report = await view.evaluate(function () {
        // The frames that stood between the two marks, and the pace of them.
        // The first 300 ms hold the animation, and the rest holds what comes
        // after it.
        const from = window.__markFrom, to = window.__markTo;
        const all = (window.__fps || []).filter(function (t) {
          return typeof t === 'number' && t >= from && t <= to; });
        const first = all.filter(function (t) { return t <= from + 300; });
        function pace(list, span) {
          if (list.length < 2) { return -1; }
          return Math.round((list.length - 1) / (span / 1000) * 10) / 10;
        }
        return { anim: pace(first, 300), tail: pace(all, to - from), frames: all.length };
      });
    } catch (e) { report = { anim: -1, tail: -1, frames: -1, err: String(e).slice(0, 200) }; }
    await view.close();
    return report;
  }

  // The box is shared, so a scene is run a few times and the median is kept.
  const repeat = Number(process.env.AN_REPEAT || 3);
  // The scenes are run in turn, and the median of their paces is kept.
  async function runScene(name, open) {
    const out = [];
    for (let n = 0; n < repeat; n++) {
      const r = await scene(name, open);
      out.push(r);
    }
    function median(key) {
      const v = out.map(function (r) { return r[key]; })
        .filter(function (x) { return x >= 0; }).sort(function (a, b) { return a - b; });
      if (!v.length) { return -1; }
      return v[Math.floor(v.length / 2)];
    }
    const m = { scene: name, anim: median('anim'), tail: median('tail') };
    results.push(m);
    console.log(name + '  runs: ' + out.map(function (r) {
      return r.anim + '/' + r.tail; }).join('  '));
    return m;
  }

  // Put the counter in the page and start it.
  async function startCount(view) {
    await view.evaluate(function () {
      window.__fps = [];
      function tick(t) { window.__fps.push(t); requestAnimationFrame(tick); }
      requestAnimationFrame(tick);
    });
  }

  // Mark the start of the window the frames are counted in.
  async function mark(view) {
    await view.evaluate(function () {
      window.__markFrom = performance.now();
      window.__markTo = window.__markFrom + 800;
    });
  }

  // row-small: the row comes up over a chat with little in it.
  await runScene('row-small', async function (view) {
    await startCount(view);
    await sleep(300);
    await mark(view);
    await view.evaluate(function () { document.getElementById('chatsbtn').click(); });
    await sleep(1000);
    return null;
  });

  // row-big: a chat with a lot in it stands open, and the row comes up over it.
  await runScene('row-big', async function (view) {
    await view.evaluate(function () { document.getElementById('turnR').click(); });
    await sleep(1200);
    await startCount(view);
    await sleep(300);
    await mark(view);
    await view.evaluate(function () { document.getElementById('chatsbtn').click(); });
    await sleep(1000);
    return null;
  });

  // slide-small: the row stands up, and a chat with little in it is picked.
  await runScene('slide-small', async function (view) {
    await view.evaluate(function () { document.getElementById('chatsbtn').click(); });
    await sleep(1200);
    await startCount(view);
    await sleep(200);
    await mark(view);
    await view.evaluate(function () {
      const cards = document.querySelectorAll('.prev');
      for (let n = 0; n < cards.length; n++) {
        if (cards[n].dataset.id === 'c9') { cards[n].click(); break; }
      }
    });
    await sleep(1000);
    return null;
  });

  // slide-big: the row stands up, and a chat with a lot in it is picked.
  await runScene('slide-big', async function (view) {
    await view.evaluate(function () { document.getElementById('chatsbtn').click(); });
    await sleep(1200);
    await startCount(view);
    await sleep(200);
    await mark(view);
    await view.evaluate(function () {
      const cards = document.querySelectorAll('.prev');
      for (let n = 0; n < cards.length; n++) {
        if (cards[n].dataset.id === 'c2') { cards[n].click(); break; }
      }
    });
    await sleep(1000);
    return null;
  });

  for (const r of results) {
    if (!r) { continue; }
    console.log(r.scene + '  anim ' + r.anim + ' fps  tail ' + r.tail + ' fps');
  }
  await browser.close();
}

main().catch(function (e) { console.error(e); process.exit(1); });
