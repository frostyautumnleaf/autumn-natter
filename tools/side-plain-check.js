// The two buttons at the head of the side bar are rows of the bar, not blocks laid over it. They
// stood out too much: a filling of the accent under one, a border under both, the word set in the
// middle of the block. What the report asked for is the plain look of the rows under them - no
// filling, no border, the word at the left - and a pointer answer as faint as the answer a chat
// row gives. This tool reads the two buttons as the page paints them, in every one of the five
// skins, on both faces: it asks whether anything of the box is left, whether the word still reads
// against the bar, and whether the left edge of the mark stands where the head of a group stands.
//
//   node side-plain-check.js          # the six reading of both faces
//   node side-plain-check.js shots    # the same, with an image of every skin
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'web', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const THEMES = ['light', 'dark', 'silver', 'corporate', 'neon'];
const shell = process.env.AN_SHELL_BIN || '';
const shots = process.argv[2] === 'shots';
const pixel = process.env.AN_PIXEL === '1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme, theme_pixel: pixel,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A chat of that repository' },
    chats: [{ id: 'c1', title: 'A chat of that repository', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'Two buttons at the head of the bar.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Do those two buttons still have a box?', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Model', body: 'They are rows of the bar now.', reasoning: '',
        reasoning_ms: 0, attachments: [], tool: '', mono: false, failed: false, when: '09:05' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true, chapters: true,
      paste_limit: 6000, width: 1200, userline: false, seamless: false, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

// The lists of the top bar, as the program sends them: the buttons above are read against the
// rows below, so the rows have to be there. They are handed to the stub beside the state, for
// the stub is carried into the page as words of its own and knows nothing of what stands here.
function theLists() {
  return {
    repositories: [{ id: 'r1', name: 'Autumn Notes', chats: [
      { id: 'c1', title: 'Two buttons at the head', when: '09:04', active: true },
      { id: 'c2', title: 'A chat being answered', when: '09:05' },
      { id: 'c3', title: 'One pinned beside the rest', when: 'Yesterday', pinned: true },
    ] }],
    loose: [{ id: 'c4', title: 'A chat of no repository', when: 'Mon' }],
    archived: [{ id: 'c5', title: 'What was put away', when: 'Last week' }],
  };
}

function stubs(state, LISTS) {
  window.__actions = [];
  var copy = Object.assign({}, state);
  // The page reads the theme out of the state it asks for again and again, so a skin is put on
  // by changing what the state says and asking the page to look at it once.
  window.__skin = function (word, pix) {
    copy.theme = word;
    copy.theme_pixel = !!pix;
    if (window.showTheme) { window.showTheme(word); }
    if (window.showPixelArt) { window.showPixelArt(!!pix); }
  };
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    window.__actions.push({ name: name });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(copy); } }); }
    if (name === 'lists') { return Promise.resolve({ json: function () { return Promise.resolve(LISTS); } }); }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(copy); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') {
          const name = String((args && args.path) || '');
          window.__actions.push({ name: name });
          if (name === 'lists') { return Promise.resolve(LISTS); }
        }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// What the page paints for one of the two buttons: every part of a box it could still carry, the
// colour of its word, and where the mark of it stands. The colour of the bar behind it is taken
// from the element the button belongs to, so the reading is of the two colours as they meet.
function readOf(which) {
  const node = document.querySelector(which);
  if (!node) { return { missing: true }; }
  const css = getComputedStyle(node);
  const bar = document.querySelector('#side') || node.parentElement;
  // A colour written in oklab, or mixed out of another, is not given back as red and green and
  // blue, and the numbers of red and green and blue are what the reading of the eye is worked
  // out from. The three forms this page colours are written in are turned by hand.
  const make = function (r, g, b, a) {
    const stock = function (v) {
      const x = Math.min(1, Math.max(0, v));
      return Math.round(255 * (x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055));
    };
    return { r: stock(r), g: stock(g), b: stock(b), a: a };
  };
  const parse = function (text) {
    const one = String(text).trim();
    const hex = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(one);
    if (hex) {
      return { r: parseInt(hex[1], 16), g: parseInt(hex[2], 16), b: parseInt(hex[3], 16), a: 1 };
    }
    const rgb = /^rgba?\(([^)]+)\)$/.exec(one);
    if (rgb) {
      const part = rgb[1].split(/[,\s/]+/).filter(function (t) { return t !== ''; })
        .map(function (t) { return parseFloat(t); });
      return { r: part[0], g: part[1], b: part[2], a: part.length > 3 ? part[3] : 1 };
    }
    const last = one.indexOf('/') > 0 ? parseFloat(one.slice(one.indexOf('/') + 1)) : 1;
    const part = function () {
      const inner = one.replace(/^[a-z-]+\(([^)/]*)[)/]/i, '$1').trim();
      return inner.split(/[\s/]+/).filter(function (t) { return t !== ''; })
        .map(function (t) { return parseFloat(t); });
    };
    if (/^oklab\(/.test(one)) {
      const n = part();
      const L = n[0], u = n[1], v = n[2];
      const l = Math.pow(L + 0.3963377774 * u + 0.2158037573 * v, 3);
      const m = Math.pow(L - 0.1055613458 * u - 0.0638541728 * v, 3);
      const s = Math.pow(L - 0.0894841775 * u - 1.2914855480 * v, 3);
      return make(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s, last);
    }
    if (/^oklch\(/.test(one)) {
      const n = part();
      const h = (n[2] || 0) * Math.PI / 180;
      return parse('oklab(' + n[0] + ' ' + (Math.cos(h) * n[1]) + ' ' + (Math.sin(h) * n[1]) + ')');
    }
    if (/^color\(/.test(one)) {
      // The name of the colour space stands first inside the brackets, and is not a number. The
      // red and green and blue of the wide display-p3 space are read as those of the narrow one,
      // which is near enough for a reading of the eye.
      const n = part().slice(1);
      return make(n[0], n[1], n[2], n.length > 3 ? n[3] : last);
    }
    return null;
  };
  const over = function (front, back) {
    if (!front) { return back; }
    if (front.a >= 1) { return front; }
    const at = 1 - front.a;
    return { r: front.r * front.a + back.r * at, g: front.g * front.a + back.g * at,
      b: front.b * front.a + back.b * at, a: 1 };
  };
  const light = function (c) {
    const one = function (v) {
      const x = v / 255;
      return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * one(c.r) + 0.7152 * one(c.g) + 0.0722 * one(c.b);
  };
  const back = parse(getComputedStyle(bar).backgroundColor) || { r: 255, g: 255, b: 255, a: 1 };
  const wash = over(parse(css.backgroundColor), back);
  const word = over(parse(css.color), wash);
  const ratio = function (a, b) {
    const hi = Math.max(light(a), light(b)) + 0.05;
    const lo = Math.min(light(a), light(b)) + 0.05;
    return Math.round((hi / lo) * 100) / 100;
  };
  const edge = function (sel) {
    const at = document.querySelector(sel);
    return at ? Math.round(at.getBoundingClientRect().left * 10) / 10 : null;
  };
  const box = node.getBoundingClientRect();
  return {
    missing: false,
    text: String(node.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 30),
    fill: css.backgroundColor, fillImage: css.backgroundImage,
    border: [css.borderTopWidth, css.borderRightWidth, css.borderBottomWidth,
      css.borderLeftWidth].join(' '),
    line: css.borderTopStyle + '/' + css.borderBottomStyle,
    shadow: css.boxShadow,
    align: css.textAlign, justify: css.justifyContent,
    size: css.fontSize, weight: css.fontWeight, radius: css.borderTopLeftRadius,
    word: css.color, wash: css.backgroundColor, bar: getComputedStyle(bar).backgroundColor,
    readWord: '#' + [word.r, word.g, word.b].map(function (v) {
      return Math.round(v).toString(16).padStart(2, '0');
    }).join(''),
    readWash: '#' + [wash.r, wash.g, wash.b].map(function (v) {
      return Math.round(v).toString(16).padStart(2, '0');
    }).join(''),
    pressed: node.getAttribute('aria-pressed'),
    contrast: ratio(word, wash),
    left: edge(which), mark: edge(which + ' svg'), head: edge('.sgroup .shead'),
    chev: edge('.sgroup .shead .chevbtn .chev'), rowtitle: edge('.srow .stitle'),
    height: Math.round(box.height * 10) / 10,
    rowheight: (function () {
      const r = document.querySelector('.srow');
      return r ? Math.round(r.getBoundingClientRect().height * 10) / 10 : null;
    })(),
  };
}

// The skin of the page, as the page itself puts it on the body: through the state, so that the
// next reading of the state leaves the skin where it was put.
async function skinOf(page, theme) {
  await page.evaluate(function (which, pix) {
    window.__skin(which, pix);
  }, theme, pixel);
  await sleep(260);
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
    await page.setViewport({ width: 1280, height: 860 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf('light')) + ', '
      + JSON.stringify(theLists()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(700);
    if (faults.length) { trouble++; console.log('  ERROR: ' + one.name + ' page faults: ' + faults.join(' | ')); }

    for (const theme of THEMES) {
      await skinOf(page, theme);
      // A click leaves the pointer where it clicked, and a button under the pointer answers with
      // its wash: the resting look is only read with the pointer away from the bar.
      await page.mouse.move(2, 2);
      await sleep(220);
      const tag = one.tag + '-' + theme + (pixel ? '-pixel' : '');
      for (const which of ['#sideNew', '#sideArch']) {
        const at = await page.evaluate(readOf, which);
        if (at.missing) {
          trouble++;
          console.log('  ERROR: ' + tag + ' has no ' + which);
          continue;
        }
        const bad = [];
        const filled = at.fillImage !== 'none'
          || !/rgba\(0, 0, 0, 0\)|transparent/.test(at.fill);
        if (filled) { bad.push('a filling is left under it: ' + at.fill + ' ' + at.fillImage); }
        if (!/^0px 0px 0px 0px$/.test(at.border)) { bad.push('a border is left on it: ' + at.border); }
        if (at.shadow !== 'none') { bad.push('it carries a shadow: ' + at.shadow); }
        if (at.align !== 'left') { bad.push('its word is ' + at.align + ', not at the left'); }
        if (at.justify !== 'flex-start' && at.justify !== 'normal' && at.justify !== 'start') {
          bad.push('its mark and word are ' + at.justify);
        }
        if (at.contrast < 4.5) {
          bad.push('the word reads at ' + at.contrast + ':1 (' + at.readWord + ' on '
            + at.readWash + '), under 4.5:1');
        }
        // The mark of the button stands where the mark of a group head stands, so the two read
        // as one list and not as a heading of another shape.
        if (at.chev !== null && Math.abs(at.mark - at.chev) > 1) {
          bad.push('its mark stands at ' + at.mark + 'px, where the mark of a group stands at '
            + at.chev + 'px');
        }
        if (bad.length) {
          trouble++;
          console.log('  ERROR: ' + tag + ' ' + which + ' [' + at.text + ']: ' + bad.join('; '));
        }
        if (theme === 'light' || theme === 'dark') {
          console.log('  ' + tag + ' ' + which + ' "' + at.text + '" fill=' + at.fill
            + ' border=' + at.border + ' align=' + at.align + ' ' + at.size + '/' + at.weight
            + ' r=' + at.radius + ' ' + at.readWord + ' on ' + at.readWash + ' = ' + at.contrast
            + ':1 mark=' + at.mark + 'px (group ' + at.chev + 'px) row title=' + at.rowtitle
            + 'px h=' + at.height + 'px (row ' + at.rowheight + 'px)');
        }
      }
      if (shots) {
        const dir = path.join(__dirname, 'n6-' + tag);
        await page.screenshot({ path: dir + '.png' });
        const piece = await page.$('#side');
        if (piece) { await piece.screenshot({ path: dir + '-side.png' }); }
      }
      // The answer to the pointer: the same faint wash a chat row answers with, and no box.
      await page.mouse.move(2, 2);
      await sleep(120);
      await page.hover('#sideNew');
      await sleep(240);
      const washed = await page.evaluate(readOf, '#sideNew');
      const noWash = /rgba\(0, 0, 0, 0\)|transparent/.test(washed.fill);
      if (noWash) {
        trouble++;
        console.log('  ERROR: ' + tag + ' #sideNew answers nothing under the pointer');
      }
      if (!/^0px 0px 0px 0px$/.test(washed.border)) {
        trouble++;
        console.log('  ERROR: ' + tag + ' #sideNew puts a border back under the pointer: '
          + washed.border);
      }
      if (washed.contrast < 4) {
        // 4.5:1 is asked of a word at rest. The answer to the pointer lasts a moment, and every
        // row of the page answers the same way, so only a bad reading is called out here.
        trouble++;
        console.log('  ERROR: ' + tag + ' #sideNew reads at only ' + washed.contrast
          + ':1 under the pointer: ' + washed.readWord + ' on ' + washed.readWash);
      }
      if (shots) {
        await page.screenshot({ path: path.join(__dirname, 'n6-' + tag + '-hover.png') });
      }
      // The archive, while it is what is looked at, is known by its colour and by nothing else.
      await page.click('#sideArch');
      await sleep(300);
      await page.mouse.move(2, 2);
      await sleep(460);
      const on = await page.evaluate(readOf, '#sideArch');
      if (!on.missing && (!/rgba\(0, 0, 0, 0\)|transparent/.test(on.fill)
        || !/^0px 0px 0px 0px$/.test(on.border))) {
        trouble++;
        console.log('  ERROR: ' + tag + ' the archive button takes a box back when it is on: '
          + on.fill + ' ' + on.border);
      }
      if (!on.missing) {
        console.log('  ' + tag + ' archive on: fill=' + on.fill + ' border=' + on.border
          + ' ' + on.readWord + ' on ' + on.readWash + ' = ' + on.contrast + ':1'
          + ' aria-pressed=' + on.pressed);
      }
      if (!on.missing && on.contrast < 4.5) {
        trouble++;
        console.log('  ERROR: ' + tag + ' the archive, when it is on, reads at only '
          + on.contrast + ':1 (' + on.readWord + ' on ' + on.readWash + ')');
      }
      if (!on.missing && on.pressed !== 'true') {
        trouble++;
        console.log('  ERROR: ' + tag + ' the archive is on but does not say so beside the '
          + 'colour (aria-pressed=' + on.pressed + ')');
      }
      if (!on.missing && shots) {
        await page.screenshot({ path: path.join(__dirname, 'n6-' + tag + '-archive-on.png') });
      }
      // The archive is looked away again before the next skin, so each skin starts the same way.
      await page.click('#sideArch');
      await sleep(300);
      const off = await page.evaluate(readOf, '#sideArch');
      if (!off.missing && off.pressed !== 'false') {
        trouble++;
        console.log('  ERROR: ' + tag + ' the archive is off but still says it is on'
          + ' (aria-pressed=' + off.pressed + ')');
      }
    }
    await page.close();
  }
  await browser.close();
  console.log((pixel ? 'pixel form; ' : '') + 'failures: ' + trouble);
}

main().catch(function (e) { console.log('FAIL', String(e)); process.exit(1); });
