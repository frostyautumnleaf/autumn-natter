// The five skins of the pages, and the pixel-art form of every one of them, looked
// at in both faces of the program with no engine behind it.
//
// Seven things are looked at, in each face:
//   * the sheet offers five skins and two forms, and the list the bar steps
//     through holds the five;
//   * the fifth skin goes on when its mark is clicked, its colour tokens answer,
//     and the call it makes carries both words of the skin;
//   * the pixel-art form keeps the colours of the skin it is laid over, so the
//     ground and the words are the same numbers with the form on as off;
//   * the form squares every corner of the page: a part that stood round in the
//     standard form stands square in the pixel-art one, and no part of what is
//     seen holds a corner of more than nought;
//   * the form is drawn from the state, so the two pages follow the one setting,
//     and the words of the mark of the bar say which form the skin stands in;
//   * the marks of the two forms make the call that writes them, and the whole of
//     the settings is written with both words of the skin in it;
//   * the welcome screen offers the fifth skin, and the mark of the form puts the
//     form on the screen it stands on.
//
//   node skin-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node skin-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';
const SHOTS = process.env.AN_SKIN_SHOTS === '1';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const WORDS = ['light', 'dark', 'silver', 'corporate', 'neon'];

function said(id, role, body) {
  return {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', tool_name: '', mono: false, failed: false,
    cancelled: false, stopped: false, can_continue: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
  };
}

// The state of the engine, as the record of web::state, for a chat with a thought
// and a run of a tool in it, so that the two forms have parts to be laid over.
function stateOf(theme, pixel) {
  return {
    rev: 1, unchanged: false, theme: theme || 'light', theme_pixel: !!pixel,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'A repository of the skin' }],
    chat: { id: 'c1', title: 'A chat in a skin', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat in a skin', repo: '', when: 'Today 09:04',
      active: true, preview: 'Five skins.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('1', 'You', 'Draw me in squares.'),
      said('2', 'Assistant', 'The same colours, out of squares.', {
        reasoning: 'The colours are those of the skin; only the shape of the parts moves.',
        reasoning_ms: 1800,
      }),
      said('3', 'Tool', 'skin: pale, dark, silver, corporate, neon', {
        tool: 'command', tool_name: 'command', mono: true,
      }),
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [],
    mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: false, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none', rain_splash: true },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.', working: [],
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

async function start(state) {
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
  if (USE_FETCH) {
    await page.evaluateOnNewDocument(function (state) {
      window.__posted = [];
      window.__state = state;
      window.__set = function (given) { window.__state = given; };
      window.fetch = function (url, init) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
        }
        if (init && init.body) {
          try { window.__posted.push({ path: String(url), body: JSON.parse(init.body) }); }
          catch (e) { window.__posted.push({ path: String(url) }); }
          // A skin and a form that were written are answered by the next ask, as
          // the engine answers them.
          try {
            var said = JSON.parse(init.body);
            if (String(url).indexOf('/api/theme') === 0) {
              if (said.theme) {
                window.__state.theme = said.theme;
                window.__state.dark = said.theme === 'dark';
              }
              if (typeof said.pixel === 'boolean') { window.__state.theme_pixel = said.pixel; }
            }
          } catch (e) { /* the words of the call were looked at once already */ }
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
    }, state);
  } else {
    await page.evaluateOnNewDocument(function (state) {
      window.__posted = [];
      window.__state = state;
      window.__set = function (given) { window.__state = given; };
      window.__TAURI__ = {
        core: {
          invoke: function (command, args) {
            if (command === 'get_state') { return Promise.resolve(window.__state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            if (command === 'action') {
              window.__posted.push({ path: '/api/' + ((args && args.path) || ''),
                body: (args && args.body) || {} });
              // A skin and a form that were written are answered by the next ask,
              // as the engine answers them.
              if (args && args.path === 'theme' && args.body) {
                if (args.body.theme) {
                  window.__state.theme = args.body.theme;
                  window.__state.dark = args.body.theme === 'dark';
                }
                if (typeof args.body.pixel === 'boolean') {
                  window.__state.theme_pixel = args.body.pixel;
                }
              }
            }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, state);
  }
  await page.setViewport({ width: 1100, height: 800 });
  await page.goto('file://' + path.resolve(PAGE));
  await sleep(400);
  return { browser: browser, page: page, errors: errors };
}

// The posts that answer to a skin, which are the ones that say what a click did.
const skinsPosted = function (page) {
  return page.evaluate(function () {
    return (window.__posted || []).filter(function (one) {
      return one.path === '/api/theme';
    });
  });
};

async function main() {
  const faces = process.env.AN_PAGE
    ? [{ tag: 'face', name: 'the page named' }]
    : [{ tag: 'window', name: 'the window' }, { tag: 'page', name: 'the browser page' }];
  let trouble = 0;
  for (const one of faces) {
    const notes = [];
    const note = (words) => { notes.push(words); console.log('  FAULT ' + one.name + ': ' + words); };

    const run = await start(stateOf('light', false));
    const page = run.page;

    // The five skins and the two forms are offered, and the bar steps through five.
    const offered = await page.evaluate(function (words) {
      const marks = words.map(function (word) {
        const id = 'fTheme' + word.charAt(0).toUpperCase() + word.slice(1);
        return !!document.getElementById(id);
      });
      return {
        skins: marks,
        list: SKINS.slice(),
        forms: [!!document.getElementById('fThemeStandard'),
          !!document.getElementById('fThemePixel')],
      };
    }, WORDS);
    if (offered.list.length !== 5 || offered.list.join('|') !== WORDS.join('|')) {
      note('the list of the skins is not the five: ' + JSON.stringify(offered.list));
    }
    if (offered.skins.indexOf(false) >= 0) {
      note('a skin has no mark of its own in the sheet: ' + JSON.stringify(offered.skins));
    }
    if (offered.forms.indexOf(false) >= 0) {
      note('a form of the skin has no mark of its own: ' + JSON.stringify(offered.forms));
    }

    // The fifth skin: the mark puts it on, its tokens answer, and the call that
    // goes out holds both words of the skin.
    const neon = await page.evaluate(async function () {
      document.getElementById('fThemeNeon').click();
      await new Promise(function (r) { setTimeout(r, 60); });
      const look = getComputedStyle(document.body);
      return {
        on: document.body.classList.contains('neon'),
        others: ['dark', 'silver', 'corporate'].some(function (c) {
          return document.body.classList.contains(c);
        }),
        word: themeWord,
        ground: look.getPropertyValue('--bg').trim(),
        accent: look.getPropertyValue('--accent').trim(),
        scheme: look.colorScheme,
        marked: document.getElementById('fThemeNeon').classList.contains('on'),
      };
    });
    if (!neon.on || neon.others || neon.word !== 'neon' || !neon.marked) {
      note('the fifth skin is not put on by its mark: ' + JSON.stringify(neon));
    }
    if (neon.scheme !== 'dark') {
      note('the fifth skin does not tell the view that it is dark: ' + neon.scheme);
    }
    if (neon.ground.toLowerCase() !== '#05060f') {
      note('the ground of the fifth skin is not its own: ' + neon.ground);
    }
    if (neon.accent.toLowerCase() !== '#22c8ff') {
      note('the accent of the fifth skin is not its own: ' + neon.accent);
    }
    let went = await skinsPosted(page);
    if (!went.length || went[went.length - 1].body.theme !== 'neon'
      || went[went.length - 1].body.pixel !== false) {
      note('the fifth skin did not go out with both of its words: ' + JSON.stringify(went));
    }

    // The colours are kept under the form: the ground and the words are the same
    // numbers with the form on as with it off.
    const kept = await page.evaluate(async function () {
      const before = {};
      const look = getComputedStyle(document.body);
      ['--bg', '--text', '--accent', '--line', '--bubble'].forEach(function (key) {
        before[key] = look.getPropertyValue(key).trim();
      });
      document.getElementById('fThemePixel').click();
      await new Promise(function (r) { setTimeout(r, 60); });
      const after = {};
      const look2 = getComputedStyle(document.body);
      ['--bg', '--text', '--accent', '--line', '--bubble'].forEach(function (key) {
        after[key] = look2.getPropertyValue(key).trim();
      });
      return {
        before: before, after: after,
        on: document.body.classList.contains('pixel'),
        marked: document.getElementById('fThemePixel').classList.contains('on'),
        standard: document.getElementById('fThemeStandard').classList.contains('on'),
        face: look2.fontFamily.toLowerCase(),
        radius: look2.getPropertyValue('--r-m').trim(),
      };
    });
    if (!kept.on || !kept.marked || kept.standard) {
      note('the form is not put on by its mark: ' + JSON.stringify(kept));
    }
    for (const key of Object.keys(kept.before)) {
      if (kept.before[key] !== kept.after[key]) {
        note('the form moved a colour of the skin (' + key + '): '
          + kept.before[key] + ' -> ' + kept.after[key]);
      }
    }
    if (kept.face.indexOf('mono') < 0) {
      note('the words of the pixel-art form are not in a fixed-width face: ' + kept.face);
    }
    if (kept.radius !== '0px') {
      note('the corner of the pixel-art form is not nought: ' + kept.radius);
    }
    went = await skinsPosted(page);
    const last = went[went.length - 1];
    if (!last || last.body.pixel !== true || last.body.theme !== 'neon') {
      note('the form did not go out beside the skin: ' + JSON.stringify(last));
    }

    // Every corner of the page is squared. What stood round in the standard form is
    // looked for by name, and then the whole of what is seen is counted again.
    const corners = await page.evaluate(async function () {
      const names = ['#bar', '#box', '.btn', '.seg', '.seg button', '.think', '.tool',
        '.bubble', '.side-menu', '.ask-card', '.meter', '.srow', 'select', 'textarea'];
      const rounded = function (node) {
        const s = getComputedStyle(node);
        const four = [s.borderTopLeftRadius, s.borderTopRightRadius,
          s.borderBottomLeftRadius, s.borderBottomRightRadius];
        for (let i = 0; i < four.length; i++) {
          const given = parseFloat(four[i]);
          if (given > 0) { return four.join(' '); }
          if (/%/.test(four[i]) && !/^0%/.test(four[i])) { return four.join(' '); }
        }
        return '';
      };
      const count = function () {
        let round = 0, seen = 0;
        const all = document.querySelectorAll('body *');
        for (let i = 0; i < all.length; i++) {
          const s = getComputedStyle(all[i]);
          if (s.display === 'none') { continue; }
          seen += 1;
          if (rounded(all[i])) { round += 1; }
        }
        return { round: round, seen: seen };
      };
      const byName = function () {
        const out = {};
        for (const name of names) {
          const node = document.querySelector(name);
          if (node) { out[name] = rounded(node) || '0px'; }
        }
        return out;
      };
      document.getElementById('fThemeStandard').click();
      await new Promise(function (r) { setTimeout(r, 60); });
      const standard = { round: count(), byName: byName() };
      document.getElementById('fThemePixel').click();
      await new Promise(function (r) { setTimeout(r, 60); });
      const pixel = { round: count(), byName: byName() };
      return { standard: standard, pixel: pixel };
    });
    if (corners.standard.round.round === 0) {
      note('nothing stood round in the standard form, so the squaring says nothing');
    }
    if (corners.pixel.round.round !== 0) {
      note(corners.pixel.round.round + ' part(s) of ' + corners.pixel.round.seen
        + ' hold a corner in the pixel-art form');
    }
    for (const name of Object.keys(corners.pixel.byName)) {
      if (corners.pixel.byName[name] !== '0px') {
        note('a part keeps its corner in the pixel-art form (' + name + '): '
          + corners.pixel.byName[name]);
      }
    }
    if (corners.pixel.round.seen < 40) {
      note('the page drew only ' + corners.pixel.round.seen + ' parts, so the look is thin');
    }

    // The state carries both words, so a page that is opened again stands where the
    // settings do, and the mark of the bar says what the skin is drawn in.
    await page.evaluate((given) => {
      if (typeof window.__set === 'function') { window.__set(given); }
      if (typeof poll === 'function') { poll(); }
    }, stateOf('silver', true));
    await sleep(400);
    const fromState = await page.evaluate(function () {
      return {
        skin: document.body.classList.contains('silver'),
        form: document.body.classList.contains('pixel'),
        marked: document.getElementById('fThemeSilver').classList.contains('on'),
        formMarked: document.getElementById('fThemePixel').classList.contains('on'),
        title: document.getElementById('themebtn')
          ? (document.getElementById('themebtn').getAttribute('data-tip') || '') : '',
      };
    });
    if (!fromState.skin || !fromState.form || !fromState.marked || !fromState.formMarked) {
      note('the state did not put on both of its words: ' + JSON.stringify(fromState));
    }
    if (fromState.title.indexOf('pixel-art') < 0) {
      note('the mark of the bar does not say the form: ' + fromState.title);
    }

    // The whole of the settings is written with both words of the skin in it.
    const written = await page.evaluate(function () {
      showTheme('corporate');
      showPixelArt(true);
      const cfg = readProgram();
      return { theme: cfg.theme, pixel: cfg.theme_pixel, dark: cfg.dark_mode };
    });
    if (written.theme !== 'corporate' || written.pixel !== true || written.dark !== false) {
      note('the whole of the settings is not written with both words: ' + JSON.stringify(written));
    }

    // The welcome screen: the fifth skin among the cards, and the mark of the form
    // putting the form on the screen it stands on.
    const welcome = await page.evaluate(async function () {
      welcomeChosen = null;
      showWelcome();
      await new Promise(function (r) { setTimeout(r, 80); });
      const cards = [];
      const names = document.querySelectorAll('#welcome .wcard .wname');
      for (let i = 0; i < names.length; i++) { cards.push(names[i].textContent); }
      const marks = document.querySelectorAll('#welcome input[type=checkbox]');
      let form = null;
      for (let i = 0; i < marks.length; i++) {
        const row = marks[i].parentNode.textContent;
        if (row.indexOf('Pixel-art') === 0) {
          form = marks[i];
          marks[i].checked = true;
          marks[i].dispatchEvent(new Event('change'));
        }
      }
      await new Promise(function (r) { setTimeout(r, 60); });
      const out = {
        cards: cards, hasForm: !!form, on: document.body.classList.contains('pixel'),
        chosen: welcomeChosen.theme_pixel,
      };
      hideWelcome();
      return out;
    });
    if (welcome.cards.indexOf('Neon') < 0) {
      note('the welcome screen does not offer the fifth skin: ' + JSON.stringify(welcome.cards));
    }
    if (!welcome.hasForm) {
      note('the welcome screen holds no mark of the pixel-art form');
    } else if (!welcome.on || welcome.chosen !== true) {
      note('the mark of the form does not put it on the welcome screen: '
        + JSON.stringify(welcome));
    }

    if (SHOTS) {
      for (const skin of WORDS) {
        for (const form of [false, true]) {
          await page.evaluate((given) => {
            if (typeof window.__set === 'function') { window.__set(given); }
            if (typeof poll === 'function') { poll(); }
            showTheme(given.theme);
            showPixelArt(!!given.theme_pixel);
          }, stateOf(skin, form));
          await sleep(300);
          await page.screenshot({
            path: path.join(__dirname, 'skin-' + one.tag + '-' + skin
              + (form ? '-pixel' : '') + '.png'),
          });
        }
      }
    }

    if (run.errors.length) {
      note('page faults: ' + run.errors.slice(0, 3).join(' | '));
    }
    if (notes.length) { trouble += 1; }
    await run.browser.close();
  }
  console.log('\n' + (trouble ? trouble + ' face(s) in trouble' : 'ok: nothing in trouble')
    + (SHOTS ? ' (shots taken)' : ''));
  if (trouble) { process.exit(1); }
}

main().catch((e) => { console.error(String(e)); process.exit(1); });
