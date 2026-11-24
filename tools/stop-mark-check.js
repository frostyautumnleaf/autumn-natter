// The mark the box of the writing wears while an answer is being written.
//
// The box carries two marks in one place: the way out, while there is something to send, and the
// mark of the stop, while an answer is arriving. The report said the second of them was a square,
// which reads as a thing that is there rather than as an answer that is ended - the program draws
// an X wherever a thing is taken off, and stopping an answer is such a thing. The mark alone was
// not enough: a mark the pointer cannot put an end to is a picture, so the check presses it too,
// and asks that a chat at work is answered by nothing but the end of the answer.
//
// What the tool reads, in every one of the five skins and on both faces:
//
//   * at rest the mark of the stop is not drawn, and the mark of sending is;
//   * while an answer is being written the two are the other way about, and the button is the
//     button of the stop, not the same button with a new picture in it;
//   * the mark drawn is an X: its strokes lie along the two diagonals of the box, they cross in
//     the middle of it, and they reach to the four corners;
//   * the mark is drawn in one colour, and that colour reads against the button under it at not
//     less than 3 to 1;
//   * the tip of the button says what the button does now, and stops being the tip of sending;
//   * a press while an answer is being written asks for the end of the answer and nothing else,
//     and a press at rest sends what is written in the box.
//
// The other half of what was reported - that the loading of a model goes on when the stop is
// pressed - is not in the page, and is checked in Rust by `tests/live_stop_load.rs`.
//
//   node stop-mark-check.js          # both faces, every skin
//   node stop-mark-check.js shots    # the same, with an image of every skin
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'web', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const THEMES = ['light', 'dark', 'silver', 'corporate', 'neon', 'glass'];
const shell = process.env.AN_SHELL_BIN || '';
const shots = process.argv[2] === 'shots';
const pixel = process.env.AN_PIXEL === '1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The state of a chat at rest. `writing` puts the chat into the state of an answer that is
// arriving, which is the state the mark of the stop is only worn in.
function stateOf(theme, writing) {
  return {
    rev: 1, unchanged: false, theme: theme, theme_pixel: pixel,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: 'r1', name: 'Autumn Notes' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A chat with an answer going', repo: 'r1' },
    chats: [{ id: 'c1', title: 'A chat with an answer going', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'The box of the writing wears the mark of the stop.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: 'a', role: 'You', body: 'Write something about the weather.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: 'b', role: 'Model', body: 'It is turning to the west, and the sky is', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:05' },
    ],
    live: writing ? 'It is turning to the west, and the sky is clearing' : '',
    reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true, chapters: true,
      paste_limit: 6000, width: 1200, userline: false, seamless: false, background: 'none' },
    context: { used: '1,204', total: '8,192', ratio: 0.15 },
    generating: !!writing, busy: !!writing, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: writing ? 'Writing' : 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

// What is put into the page before it runs: the two bridges of the program, a book of the asks
// the page sends, and two ways for the check to move the page about - the skin it is drawn in,
// and whether an answer is going on. The look at the state is asked for at once, because a chat
// at rest is read again only after the slower pace of the poll.
function bridges(given) {
  window.__state = given;
  window.__posts = [];
  var note = function (name, body) { window.__posts.push({ name: name, body: body }); };
  window.__skin = function (word) {
    window.__state.theme = word;
    window.__state.dark = word === 'dark';
    if (window.showTheme) { window.showTheme(word); }
  };
  window.__writing = function (going) {
    window.__state.generating = !!going;
    window.__state.busy = !!going;
    window.__state.live = going ? 'It is turning to the west, and the sky is clearing' : '';
    window.__state.status = going ? 'Writing' : 'ready';
    if (typeof poll === 'function') { poll(); }
  };
  window.fetch = function (url, opts) {
    var where = String(url);
    if (where.indexOf('/api/state') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
    }
    if (where.indexOf('/api/') === 0) {
      var body = {};
      try { body = JSON.parse((opts && opts.body) || '{}'); } catch (e) { body = {}; }
      note(where.slice(5), body);
    }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(window.__state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { note((args && args.path) || '', (args && args.body) || {}); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
  window.__asks = function () { return window.__posts.map(function (p) { return p.name; }); };
}

// The button as the page paints it, read inside the page. The mark of the stop is measured where
// it is drawn: the path is sampled along its length, and every point is asked whether it lies on
// one of the two diagonals of the box and how far the middle of the box stands from it. A square
// fails both questions - a point of it is away from the middle by the half of its side and off
// the diagonals - and only a mark drawn corner to corner passes them.
//
// The colours are read as the two of them meet, and the colour of a stroke is turned out of
// `currentColor` when that is all the sheet says, so the reading is of what the eye is given.
function theLook() {
  var node = document.querySelector('#send');
  if (!node) { return { missing: true }; }
  var parse = function (text) {
    var one = String(text).trim();
    var hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(one);
    if (hex) { return { r: parseInt(hex[1], 16), g: parseInt(hex[2], 16), b: parseInt(hex[3], 16), a: 1 }; }
    var rgb = /^rgba?\(([^)]+)\)$/.exec(one);
    if (rgb) {
      var p = rgb[1].split(/[,\s/]+/).filter(function (t) { return t !== ''; }).map(parseFloat);
      return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
    }
    return null;
  };
  var over = function (front, back) {
    if (!front) { return back; }
    if (front.a >= 1) { return front; }
    var rest = 1 - front.a;
    return { r: front.r * front.a + back.r * rest, g: front.g * front.a + back.g * rest,
      b: front.b * front.a + back.b * rest, a: 1 };
  };
  var light = function (c) {
    var one = function (v) {
      var x = v / 255;
      return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * one(c.r) + 0.7152 * one(c.g) + 0.0722 * one(c.b);
  };
  var ratio = function (a, b) {
    var hi = Math.max(light(a), light(b)) + 0.05;
    var lo = Math.min(light(a), light(b)) + 0.05;
    return Math.round((hi / lo) * 100) / 100;
  };
  var hex = function (c) {
    return '#' + [c.r, c.g, c.b].map(function (v) {
      return Math.round(v).toString(16).padStart(2, '0');
    }).join('');
  };

  var css = getComputedStyle(node);
  var back = parse(css.backgroundColor) || { r: 255, g: 255, b: 255, a: 1 };
  var stop = node.querySelector('.mark-stop');
  var send = node.querySelector('.mark-send');
  var out = {
    missing: !stop || !send,
    cls: String(node.className || ''),
    tip: String(node.getAttribute('data-tip') || node.getAttribute('title')
      || node.getAttribute('aria-label') || ''),
    stopDisplay: stop ? getComputedStyle(stop).display : 'gone',
    sendDisplay: send ? getComputedStyle(send).display : 'gone',
    box: Math.round(node.getBoundingClientRect().width * 10) / 10,
    back: hex(back),
  };
  if (out.missing || out.stopDisplay === 'none') { return out; }

  var stopCss = getComputedStyle(stop);
  var said = parse(stopCss.stroke) || parse(getComputedStyle(node).color);
  out.d = String(stop.getAttribute('d') || '');
  out.stroke = said ? hex(said) : String(stopCss.stroke);
  out.strokeWidth = stopCss.strokeWidth;
  out.linecap = stopCss.strokeLinecap;
  out.fill = stopCss.fill;
  if (said) { out.read = ratio(over(said, back), back); }

  // The shape, in the units the path is drawn in, whatever size the button is painted at.
  var full = stop.getTotalLength();
  var samples = 200;
  var worst = 0, near = full, low = { x: Infinity, y: Infinity },
    high = { x: -Infinity, y: -Infinity }, first = null;
  for (var i = 0; i < samples; i++) {
    var at = stop.getPointAtLength(full * i / (samples - 1));
    if (!first) { first = { x: at.x, y: at.y }; }
    low = { x: Math.min(low.x, at.x), y: Math.min(low.y, at.y) };
    high = { x: Math.max(high.x, at.x), y: Math.max(high.y, at.y) };
    var dev = Math.abs(Math.abs(at.x - 8) - Math.abs(at.y - 8));
    if (dev > worst) { worst = dev; }
    var d = Math.sqrt((at.x - 8) * (at.x - 8) + (at.y - 8) * (at.y - 8));
    if (d < near) { near = d; }
  }
  out.length = Math.round(full * 100) / 100;
  out.offDiagonal = Math.round(worst * 100) / 100;
  out.fromCentre = Math.round(near * 100) / 100;
  out.spanX = Math.round((high.x - low.x) * 100) / 100;
  out.spanY = Math.round((high.y - low.y) * 100) / 100;
  out.start = first ? { x: first.x, y: first.y } : null;
  return out;
}

// The marks of the one reading that were not as they should be. `want` is the state the page is
// asked to be in: an answer going on, or a chat at rest.
function faultsOf(tag, look, want) {
  const bad = [];
  if (look.missing) { return [tag + ' has no send button, or no mark of the stop in it']; }
  const hasStop = /(^|\s)stop(\s|$)/.test(look.cls);
  if (want) {
    if (!hasStop) { bad.push('the button is not the button of the stop while an answer goes on (' + look.cls + ')'); }
    if (look.stopDisplay === 'none') { bad.push('the mark of the stop is not drawn while an answer goes on'); }
    if (look.sendDisplay !== 'none') { bad.push('the mark of sending is drawn as well, over the mark of the stop (' + look.cls + ' ' + look.stopDisplay + '/' + look.sendDisplay + ')'); }
    if (!/^stop/i.test(look.tip)) { bad.push('its tip says "' + look.tip + '", not of stopping'); }
    // The shape of the mark. Only a mark drawn from corner to corner of the box passes these.
    if (look.offDiagonal > 0.25) {
      bad.push('its strokes leave the diagonals of the box by ' + look.offDiagonal
        + ' units, so it is not an X');
    }
    if (look.fromCentre > 0.2) {
      bad.push('its strokes miss the middle of the mark by ' + look.fromCentre
        + ' units, so they do not cross');
    }
    if (look.spanX < 6 || look.spanY < 6) {
      bad.push('the mark reaches only ' + look.spanX + ' by ' + look.spanY
        + ' units of the 16 it is drawn in');
    }
    if (look.fill !== 'none') { bad.push('the mark is filled (' + look.fill + '), so it reads as a block'); }
    if (!(look.read >= 3)) {
      bad.push('the mark reads at ' + look.read + ':1 against the button (' + look.stroke + ' on '
        + look.back + '), under 3:1');
    }
  } else {
    if (hasStop) { bad.push('the button wears the mark of the stop at rest (' + look.cls + ')'); }
    if (look.stopDisplay !== 'none') { bad.push('the mark of the stop is drawn at rest'); }
    if (look.sendDisplay === 'none') { bad.push('the mark of sending is not drawn at rest'); }
    if (!/^send/i.test(look.tip)) { bad.push('its tip says "' + look.tip + '", not of sending'); }
  }
  return bad;
}

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const faults = [];
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 900 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(bridges, stateOf('light', false));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(700);
    if (faults.length) {
      trouble++;
      console.log('  ERROR: ' + one.name + ' page faults: ' + faults.join(' | '));
    }

    for (const theme of THEMES) {
      await page.evaluate(function (word) { window.__skin(word); }, theme);
      await sleep(300);
      const tag = one.tag + '-' + theme + (pixel ? '-pixel' : '');

      // The chat at rest: the box is the box of sending.
      await page.evaluate(function () { window.__writing(false); });
      await sleep(300);
      // A click leaves the pointer over the button, and the button answers the pointer: the
      // resting look is only read with the pointer away from it.
      await page.mouse.move(2, 2);
      await sleep(150);
      const rest = await page.evaluate(theLook);
      for (const bad of faultsOf(tag, rest, false)) { trouble++; console.log('  ERROR: ' + bad); }
      if (shots) {
        const piece = await page.$('#box');
        if (piece) { await piece.screenshot({ path: path.join(__dirname, 'n10-' + tag + '-rest.png') }); }
      }

      // An answer going on: the same place is the way to end it.
      await page.evaluate(function () { window.__writing(true); });
      await sleep(320);
      const going = await page.evaluate(theLook);
      for (const bad of faultsOf(tag, going, true)) { trouble++; console.log('  ERROR: ' + bad); }
      if (theme === 'light' || theme === 'dark') {
        console.log('  ' + tag + ' stop: d="' + going.d + '" ' + going.stroke + ' on ' + going.back
          + ' = ' + going.read + ':1 width=' + going.strokeWidth + ' cap=' + going.linecap
          + ' length=' + going.length + ' off-diagonal=' + going.offDiagonal
          + ' from middle=' + going.fromCentre + ' span=' + going.spanX + 'x' + going.spanY
          + ' tip="' + going.tip + '" h=' + going.box + 'px');
      }
      if (shots) {
        const piece = await page.$('#box');
        if (piece) { await piece.screenshot({ path: path.join(__dirname, 'n10-' + tag + '-stop.png') }); }
      }

      // The first press of the two: while an answer is being written, the box asks for the end of
      // that answer, and for nothing else. A press that sent the text of the box as well would
      // put a message into a chat that is still being answered.
      await page.evaluate(function () { window.__posts.length = 0; });
      await page.click('#send');
      await sleep(350);
      const stopped = await page.evaluate(function () { return window.__asks(); });
      if (stopped.filter(function (n) { return n !== 'cancel'; }).length) {
        trouble++;
        console.log('  ERROR: ' + tag + ' a press while an answer goes on asked for: '
          + stopped.join(', '));
      }
      if (stopped.indexOf('cancel') < 0) {
        trouble++;
        console.log('  ERROR: ' + tag + ' a press while an answer goes on did not ask for the end'
          + ' of it (it asked for: ' + (stopped.join(', ') || 'nothing') + ')');
      }
      if (theme === 'dark') { console.log('  ' + tag + ' press while writing -> ' + stopped.join(', ')); }

      // The press at rest, with a message written in the box: it goes out, and the end of an
      // answer is not asked for in a chat that is writing none.
      await page.evaluate(function () { window.__writing(false); });
      await sleep(300);
      await page.click('#draft');
      await page.type('#draft', 'Is the mark of the stop an X now?');
      await sleep(200);
      const after = await page.evaluate(theLook);
      for (const bad of faultsOf(tag, after, false)) { trouble++; console.log('  ERROR: ' + bad); }
      await page.evaluate(function () { window.__posts.length = 0; });
      await page.click('#send');
      await sleep(350);
      const sent = await page.evaluate(function () { return window.__asks(); });
      if (sent.indexOf('send') < 0) {
        trouble++;
        console.log('  ERROR: ' + tag + ' a press at rest did not send the message (it asked for: '
          + (sent.join(', ') || 'nothing') + ')');
      }
      if (sent.indexOf('cancel') >= 0) {
        trouble++;
        console.log('  ERROR: ' + tag + ' a press at rest asked for the end of an answer');
      }
      if (theme === 'dark') { console.log('  ' + tag + ' press at rest -> ' + sent.join(', ')); }
    }
    await page.close();
  }
  await browser.close();
  console.log(trouble ? 'ERRORS: ' + trouble : 'ok: the stop mark is an X in every skin of both faces');
  process.exit(trouble ? 1 : 0);
}

main().catch(function (e) { console.error('ERROR: ' + (e && e.stack || e)); process.exit(1); });
