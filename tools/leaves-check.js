// The leaves that fall behind the words of the chat, looked at in both faces of
// the program with no engine behind them.
//
// Six things are looked at, in each face:
//   * the background canvas stands in the chat stage, under the thread, and does
//     not catch clicks;
//   * the leaf is the program's icon: the path data of the leaf body, the stem and
//     the veins equals the path data in `frontend/assets/autumn-natter.svg`, the
//     paths are drawn with Path2D, and the drawn shape has the proportions of the
//     icon and grows with the size it is asked for;
//   * the colour of a leaf is read out of the skin - darker than the ground of a
//     pale skin, lighter than the ground of a dark one, and grey where the skin
//     is grey;
//   * nothing falls while the setting says nothing, however long an answer is
//     being written;
//   * leaves fall while an answer is being written and the setting asks for them,
//     they are drawn, and they go out again when the answer stops;
//   * a machine asked for few moves is given no falling at all, and the marks of
//     the sheet of the settings stand at the word the settings hold.
//
//   node leaves-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node leaves-check.js
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

// The mark of the program, which the falling leaves are drawn from.
const MARK_SVG = path.join(__dirname, '..', 'frontend', 'assets', 'autumn-natter.svg');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme || 'light',
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A chat with leaves behind it' },
    chats: [{ id: 'c1', title: 'A chat with leaves behind it', repo: '', when: 'Today 09:04',
      active: true, preview: 'Let the leaves fall.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Let the leaves fall.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Assistant', body: 'The leaves are of the ground, a shade off it.',
        reasoning: '', reasoning_ms: 0, attachments: [], tool: '', mono: false,
        failed: false, when: '09:05' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none' },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// What is put into the page before it runs, so the measures below can be taken.
const LOOK = `
// How light a colour is. The colour is read by the page's own reader, so a
// setting that holds it in hex or in rgb() form is read the same way here as it is
// where the leaves are coloured.
function lightOf(words) {
  var one = fallNumbers(words);
  return one ? (one[0] * 299 + one[1] * 587 + one[2] * 114) / 1000 : NaN;
}
// The squares of an offscreen canvas a leaf covers when it is drawn, how many of
// them are drawn into, and how many of those are partly transparent (the soft edge
// of an anti-aliased path). The leaf is drawn through fallDraw, so what is measured
// is what the background draws.
function leafPaint(one, tone) {
  if (!fall.tones) { fallTones(); }
  var canvas = document.createElement('canvas');
  canvas.width = 200; canvas.height = 200;
  var ctx = canvas.getContext('2d');
  var kept = fall.ctx;
  one.seen = 1; one.swing = 0; one.wide = 0; one.lean = 1; one.angle = 0;
  one.x = 100; one.y = 100;
  fall.ctx = ctx;
  fallDraw(one, tone || fall.tones[0]);
  fall.ctx = kept;
  var data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  var out = { lit: 0, soft: 0, minX: 1e9, maxX: -1, minY: 1e9, maxY: -1 };
  for (var y = 0; y < canvas.height; y++) {
    for (var x = 0; x < canvas.width; x++) {
      var a = data[(y * canvas.width + x) * 4 + 3];
      if (a > 8) {
        out.lit += 1;
        if (a < 247) { out.soft += 1; }
        if (x < out.minX) { out.minX = x; }
        if (x > out.maxX) { out.maxX = x; }
        if (y < out.minY) { out.minY = y; }
        if (y > out.maxY) { out.maxY = y; }
      }
    }
  }
  out.wide = out.maxX - out.minX + 1;
  out.tall = out.maxY - out.minY + 1;
  // The leaf is stood with the middle of its body at the middle of the canvas, so
  // the pixel there says whether a leaf is drawn solid or drawn thin.
  out.core = data[(100 * canvas.width + 100) * 4 + 3];
  return out;
}
// How many squares of the canvas the program has drawn into.
function painted() {
  var canvas = document.getElementById('fall');
  if (!canvas || !canvas.width || !canvas.height) { return 0; }
  var ctx = canvas.getContext('2d');
  var given = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  var lit = 0;
  for (var i = 3; i < given.length; i += 4) { if (given[i] > 8) { lit += 1; } }
  return lit;
}
`;

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const page = await browser.newPage();
    await page.setViewport({ width: 1280, height: 860 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(LOOK);
    // Both bridges are stood in, so either face may be read the same way. The
    // state is kept on the page, so a step of this check may say whether an answer
    // is being written and let the page notice it for itself, as it does in use.
    await page.evaluateOnNewDocument(function (given) {
      window.__state = given;
      window.__work = function (on) { window.__state.generating = !!on; };
      window.fetch = function (url) {
        if (String(url).indexOf('state') === 1 || String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
      window.__TAURI__ = {
        core: {
          invoke: function (command) {
            if (command === 'get_state') { return Promise.resolve(window.__state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, stateOf('light'));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(400);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  FAULT: ' + what); };

    // Where the canvas stands.
    const placed = await page.evaluate(() => {
      const canvas = document.getElementById('fall');
      const stage = document.getElementById('stage');
      const thread = document.getElementById('thread');
      const look = getComputedStyle(canvas);
      return {
        inStage: canvas.parentNode === stage,
        first: stage.firstElementChild === canvas,
        overThread: Number(look.zIndex) < Number(getComputedStyle(thread).zIndex),
        clicks: look.pointerEvents,
        ground: getComputedStyle(document.body).getPropertyValue('--bg').trim(),
        wide: canvas.clientWidth, tall: canvas.clientHeight,
      };
    });

    // The leaf itself: the path data the face draws with, and what one leaf looks
    // like when it is drawn. The path data is compared with the mark on disk below.
    const shapes = await page.evaluate(() => {
      const one = (size) => ({ size: size, tone: 0, turned: false });
      const small = leafPaint(one(LEAF_SIZES[0]));
      const large = leafPaint(one(LEAF_SIZES[LEAF_SIZES.length - 1]));
      const paths = fallPaths();
      return {
        mark: LEAF_MARK,
        body: String(LEAF_BODY),
        stalk: String(LEAF_STALK.d),
        stalkWide: LEAF_STALK.wide,
        veins: LEAF_VEINS.map((v) => ({ d: String(v.d), wide: v.wide })),
        sizes: LEAF_SIZES.slice(),
        least: LEAF_LEAST, most: LEAF_MOST,
        isPath: typeof Path2D === 'function'
          && paths.body instanceof Path2D && paths.stalk instanceof Path2D
          && paths.veins.every((v) => v.path instanceof Path2D),
        veinCount: paths.veins.length,
        cached: paths === fallPaths(),
        small: small, large: large,
      };
    });

    // The path data on disk, as one string of space-separated numbers per path.
    // The body of the mark is drawn twice on disk (the colour and the shading of
    // it), so the leaf is compared with both of them being the same path.
    const markText = fs.readFileSync(MARK_SVG, 'utf8');
    const join = (d) => String(d).replace(/\s+/g, ' ').trim();
    const markPaths = (markText.match(/\bd="([^"]+)"/g) || [])
      .map((attr) => join(attr.slice(3, -1)));
    const markBody = markPaths.filter((d) => / z$/i.test(d));
    const markStalk = markPaths.filter((d) => /^M64 86 C/.test(d));
    const markVeins = markPaths.filter((d) => /^M\d+ \d+ L/.test(d));

    // The colour of a leaf, in the two skins of the program, and in the two of
    // grey. A leaf has to be of the ground it is seen against, and off it by a
    // shade the way the ground asks for.
    const tones = await page.evaluate(() => {
      const given = [];
      ['light', 'dark', 'silver', 'corporate'].forEach((word) => {
        showTheme(word);
        fallTones();
        const ground = lightOf(getComputedStyle(document.body).getPropertyValue('--bg'));
        const body = lightOf(fall.tones[0].body);
        const strong = lightOf(fall.tones[0].stalk);
        const raw = fallNumbers(fall.tones[0].body);
        given.push({
          word: word,
          ground: ground,
          body: body,
          // The stalk has to stand off the ground more than the body of the leaf
          // does, or it is a part of the leaf and not its stalk.
          stronger: Math.abs(strong - ground) > Math.abs(body - ground),
          grey: Math.abs(raw[0] - raw[1]) < 3 && Math.abs(raw[1] - raw[2]) < 3,
          span: Math.abs(ground - body),
        });
      });
      showTheme('light');
      fallTones();
      return given;
    });

    // Nothing falls while the setting says nothing, however long the answer is.
    const silent = await page.evaluate(() => {
      fallWant('none');
      window.__work(true);
      return { want: fall.want };
    });
    await sleep(220);
    const stillSilent = await page.evaluate(() => ({
      raf: fall.raf, leaves: fall.leaves.length, painted: painted(),
    }));

    // The word of the setting is carried in with the state: the page is told by
    // the engine, and not by this check.
    await page.evaluate(() => { window.__state.chatting.background = 'leaves'; });
    await sleep(700);
    const told = await page.evaluate(() => ({
      want: fall.want, held: chatting.background, raf: fall.raf,
      busy: fall.busy, asked: fallAsked(), size: [fall.w, fall.h],
    }));

    // And now they fall, while an answer is being written. The word came in with
    // the state above, so it is the page that lets them go, and not this check.
    await sleep(400);
    // A screen of leaves: how many there are, and whether they differ from one
    // another in angle, in size and in which way up they are turned.
    const falling = await page.evaluate(() => {
      const angles = fall.leaves.map((one) => one.angle);
      const sizes = fall.leaves.map((one) => one.size);
      const many = [];
      for (var i = 0; i < 60; i++) { many.push(fallSeed(true)); }
      return {
        raf: fall.raf, leaves: fall.leaves.length, painted: painted(),
        wanted: fallWanted(),
        seen: fall.leaves.map((one) => one.seen),
        spin: fall.leaves.some((one) => one.spin !== 0),
        angleSpan: angles.length > 1 ? Math.max.apply(null, angles) - Math.min.apply(null, angles) : 0,
        sizeKinds: sizes.filter((size, at) => sizes.indexOf(size) === at).length,
        seedSizes: many.map((one) => one.size).filter((size, at, all) => all.indexOf(size) === at).length,
        seedTurned: many.filter((one) => one.turned).length,
        seedAngles: many.map((one) => one.angle).filter((angle, at, all) => all.indexOf(angle) === at).length,
      };
    });

    // And now they stop: the answer is over, and the leaves go out again.
    await page.evaluate(() => { window.__work(false); });
    await sleep(1600);
    const stopped = await page.evaluate(() => ({
      raf: fall.raf, leaves: fall.leaves.length, painted: painted(),
    }));

    // A machine asked for few moves is given none of this.
    const calm = await page.evaluate(() => {
      reduce = true;
      fallWant('none');
      fallWant('leaves');
      window.__work(true);
      return { raf: fall.raf, leaves: fall.leaves.length };
    });
    await sleep(250);
    const stillCalm = await page.evaluate(() => ({
      raf: fall.raf, leaves: fall.leaves.length, painted: painted(),
    }));
    await page.evaluate(() => {
      reduce = false;
      fallWant('none');
      window.__work(false);
    });

    // The marks of the sheet of the settings, and the word they stand at.
    const sheet = await page.evaluate(() => {
      showBackgroundButtons('leaves');
      const marks = {
        word: backgroundWord(),
        none: document.getElementById('fBackgroundNone').classList.contains('on'),
        leaves: document.getElementById('fBackgroundLeaves').classList.contains('on'),
        group: !!document.getElementById('fBackground'),
      };
      document.getElementById('fBackgroundNone').click();
      marks.after = backgroundWord();
      marks.dirty = progDirty;
      showBackgroundButtons(undefined);
      marks.fallback = backgroundWord();
      return marks;
    });

    // A page that never saw the skin move, as a page opened under the skin it
    // already had: the colours of the leaves are then read at the first frame, and
    // the first leaf drawn is drawn of the skin the page stands in.
    await page.evaluate(() => {
      fall.tones = null;
      fall.paths = null;
      window.__work(true);
      fallBusy(true);
      fallWant('leaves');
    });
    await sleep(600);
    const fresh = await page.evaluate(() => ({
      tones: !!fall.tones, leaves: fall.leaves.length, raf: !!fall.raf, painted: painted(),
    }));
    await page.evaluate(() => {
      window.__work(false);
      fallBusy(false);
      fallWant('none');
    });
    await sleep(1300);
    const cleared = await page.evaluate(() => ({
      raf: !!fall.raf, leaves: fall.leaves.length, painted: painted(),
    }));

    await page.close();

    if (faults.length) { note('page faults: ' + faults.join(' | ')); }
    if (!placed.inStage || !placed.first) { note('the canvas does not stand first in the place of the chat'); }
    if (!placed.overThread) { note('the canvas is not under the thread'); }
    if (placed.clicks !== 'none') { note('the canvas catches the hand: ' + placed.clicks); }
    if (!placed.wide || !placed.tall) { note('the canvas has no size: ' + placed.wide + ' by ' + placed.tall); }
    if (shapes.mark !== 128) { note('the leaf is not drawn in the square of the mark: ' + shapes.mark); }
    if (markBody.length < 1 || markBody.some((d) => d !== markBody[0])
      || join(shapes.body) !== markBody[0]) {
      note('the body of the leaf is not the body of the mark on disk');
    }
    if (markStalk.length !== 1 || join(shapes.stalk) !== markStalk[0]) {
      note('the stalk of the leaf is not the stalk of the mark on disk');
    }
    if (shapes.veinCount !== markVeins.length || shapes.veins.length !== markVeins.length) {
      note('the leaf does not carry the veins of the mark: ' + shapes.veinCount
        + ' drawn, ' + markVeins.length + ' in the mark');
    } else {
      shapes.veins.forEach((vein, at) => {
        if (join(vein.d) !== markVeins[at]) { note('vein ' + (at + 1) + ' is not the vein of the mark'); }
      });
    }
    if (!shapes.isPath) { note('the leaf is not drawn as a Path2D'); }
    if (!shapes.cached) { note('the paths of the leaf are made again every frame'); }
    if (!shapes.sizes.length || shapes.sizes.length < 2) {
      note('the leaf has only one size: ' + JSON.stringify(shapes.sizes));
    }
    if (!(shapes.least > 0 && shapes.most > shapes.least)) {
      note('the number of leaves that may fall is not a range: ' + shapes.least + '-' + shapes.most);
    }
    if (!shapes.small.lit || !shapes.large.lit) {
      note('a leaf drawn on its own paints nothing: ' + JSON.stringify([shapes.small, shapes.large]));
    }
    if (shapes.small.soft < 20 || shapes.large.soft < 40) {
      note('the edge of a leaf is not anti-aliased: ' + shapes.small.soft + ' and '
        + shapes.large.soft + ' partly drawn squares');
    }
    if (shapes.small.core < 250 || shapes.large.core < 250) {
      note('a leaf is drawn thin, and not solid: ' + shapes.small.core + ' and '
        + shapes.large.core + ' of 255 in the middle of its body');
    }
    if (shapes.large.lit <= shapes.small.lit * 1.4) {
      note('a bigger leaf is not bigger: ' + shapes.small.lit + ' against ' + shapes.large.lit);
    }
    const square = shapes.large.wide / shapes.large.tall;
    if (!(square > 0.6 && square < 1.5)) {
      note('a leaf is not the shape of the mark it is drawn from: ' + shapes.large.wide
        + ' by ' + shapes.large.tall);
    }
    tones.forEach((tone) => {
      const needed = tone.word === 'light' ? tone.body < tone.ground : tone.body > tone.ground;
      if (!needed) {
        note('a leaf of the ' + tone.word + ' skin is not off its ground the right way: ground '
          + tone.ground.toFixed(1) + ', leaf ' + tone.body.toFixed(1));
      }
      if (tone.span < 12 || tone.span > 60) {
        note('a leaf of the ' + tone.word + ' skin is too far off its ground, or not far enough: '
          + tone.span.toFixed(1));
      }
      if (tone.stronger !== true) { note('the stalk of a leaf is not the stronger part'); }
      const grey = tone.word === 'silver' || tone.word === 'corporate';
      if (grey && !tone.grey) { note('a leaf of the ' + tone.word + ' skin is not grey'); }
      if (!grey && tone.grey) { note('a leaf of the ' + tone.word + ' skin lost its warmth'); }
    });
    if (silent.raf || silent.leaves || stillSilent.painted) {
      note('leaves fell while the setting said nothing: ' + JSON.stringify(stillSilent));
    }
    if (told.want !== 'leaves' || told.held !== 'leaves') {
      note('the word of the setting was not carried in: ' + JSON.stringify(told));
    }
    if (told.size[0] < 100 || told.size[1] < 100) {
      note('the place the leaves fall over has no size: ' + JSON.stringify(told.size));
    }
    if (!falling.raf) { note('nothing is going while an answer is being written'); }
    if (falling.leaves < 6) {
      note('only ' + falling.leaves + ' leaves fall, of ' + falling.wanted + ' the place is worth');
    }
    if (falling.painted < 200) {
      note('the leaves are barely drawn: ' + falling.painted + ' squares of the canvas');
    }
    if (!falling.seen.length || Math.max.apply(null, falling.seen) <= 0) {
      note('no leaf may be seen while they fall');
    }
    if (!falling.spin) { note('the leaves do not turn as they come down'); }
    if (falling.angleSpan < 0.4) {
      note('the leaves stand at nearly the same angle: ' + falling.angleSpan.toFixed(2));
    }
    if (falling.sizeKinds < 2 && falling.leaves > 3) {
      note('every leaf that falls is of one size: ' + JSON.stringify(falling.sizeKinds));
    }
    if (falling.seedSizes < 2) {
      note('the sizes of the leaves are not chosen: ' + falling.seedSizes);
    }
    if (falling.seedTurned === 0 || falling.seedTurned === 60) {
      note('the leaves are not turned over at random: ' + falling.seedTurned + ' of 60');
    }
    if (falling.seedAngles < 50) {
      note('the angle a leaf starts at is not random: ' + falling.seedAngles + ' of 60');
    }
    if (stopped.raf || stopped.leaves || stopped.painted) {
      note('the leaves did not go out when the answer stopped: ' + JSON.stringify(stopped));
    }
    if (calm.raf || stillCalm.painted || stillCalm.leaves) {
      note('a machine asked for few moves was given leaves: ' + JSON.stringify(stillCalm));
    }
    if (sheet.word !== 'leaves' || sheet.none || !sheet.leaves || !sheet.group) {
      note('the marks of the sheet do not stand at the word: ' + JSON.stringify(sheet));
    }
    if (sheet.after !== 'none') { note('a click on Nothing did not move the mark'); }
    if (sheet.dirty !== true) { note('the sheet was not marked as holding words never written'); }
    if (sheet.fallback !== 'none') { note('no word in the settings did not give Nothing'); }
    if (!fresh.tones || !fresh.leaves || !fresh.raf || fresh.painted < 200) {
      note('a page whose skin never moved drew nothing: ' + JSON.stringify(fresh));
    }
    if (cleared.raf || cleared.leaves || cleared.painted) {
      note('the leaves of that page did not go out again: ' + JSON.stringify(cleared));
    }

    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' faces'); }
  console.log('the leaves of the chat background fall, and stop, in both faces');
}

main().catch((e) => { console.error(e); process.exit(1); });
