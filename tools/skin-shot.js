// The five themes, and the pixel-art form of them, drawn out for the eye icon: the chat
// with its blocks, and the settings panel open at the Program tab, in each
// form of each theme.
//
//   node skin-shot.js                  # every theme, both forms, both pages
//   AN_PAGE=web/page.html node skin-shot.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';
const WORDS = ['light', 'dark', 'silver', 'corporate', 'neon'];
const ONLY = process.env.AN_SKIN ? process.env.AN_SKIN.split(',') : null;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function said(id, role, body, extra) {
  const one = {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', tool_name: '', mono: false, failed: false,
    cancelled: false, stopped: false, can_continue: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
  };
  return Object.assign(one, extra || {});
}

function stateOf(theme, pixel) {
  return {
    rev: 1, unchanged: false, theme: theme, theme_pixel: !!pixel,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: 'r1', name: 'autumn-natter' },
    repositories: [{ id: 'r1', name: 'autumn-natter' }, { id: 'r2', name: 'another-work' }],
    chat: { id: 'c1', title: 'A chat in every theme', planning: false, planned: false },
    chats: [
      { id: 'c1', title: 'A chat in every theme', repo: '', when: 'Today 09:04',
        active: true, preview: 'Five themes, two forms.' },
      { id: 'c2', title: 'One being answered', repo: 'r1', when: 'Today 09:02',
        active: false, preview: 'The squares hold.', working: true },
    ],
    commands: [{ name: '/plan', words: 'Plan the work before any of it is done.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('1', 'You', 'Draw all of the interface out of squares.'),
      said('2', 'Assistant', 'The colours are those of the theme; only the form moves.', {
        reasoning: 'The corner of every part is zero, a shadow is a flat block, and the '
          + 'edges of a marker are not smoothed, so a diagonal is a stair of squares.',
        reasoning_ms: 2100,
      }),
      said('3', 'Tool', 'theme: pale, dark, silver, corporate, neon\nchecked: 5 themes, 2 forms', {
        tool: 'command', tool_name: 'command', mono: true,
      }),
      said('4', 'Assistant', 'The file is `web/page.html`, and the form is `pixel`.\n\n'
        + '```c\nint square = 1;\n```\n\n- the corner is zero\n- the shadow is a block', {
        reasoning: 'A fixed-width face is the face a pixel-art screen is written in.',
        reasoning_ms: 900,
      }),
      said('5', 'You', 'And the neon one?'),
      said('6', 'Assistant', 'A night of blue-black, with the hairlines lit.', {
        reasoning: 'The accent is cyan, the leaf magenta, and an error is a red of its own.',
        reasoning_ms: 700,
      }),
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [{ id: 'ch1', title: 'The five themes', from: 0, to: 3, summary: 'Five themes, and the forms of them.' }],
    live_tools: [],
    mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: false, collapse_tools: false, highlight: true,
      chapters: true, paste_limit: 6000, background: 'none', rain_splash: true },
    context: { used: '2,412', total: '8,192', ratio: 0.29 },
    generating: false, busy: false, code: true, sandbox: true,
    sandbox_available: true, sandbox_note: 'bubblewrap works.', working: ['c2'],
    status: 'ready', dark: theme === 'dark' || theme === 'corporate' || theme === 'neon',
    data_dir: '/tmp/autumn-natter',
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
      window.__state = state;
      window.fetch = function (url, init) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true, lists: { models: [], repositories: [] }, config: {} }); } });
      };
    }, state);
  } else {
    await page.evaluateOnNewDocument(function (state) {
      window.__state = state;
      window.__TAURI__ = {
        core: {
          invoke: function (command, args) {
            if (command === 'get_state') { return Promise.resolve(window.__state); }
            if (command === 'get_config') {
              return Promise.resolve({ theme: window.__state.theme,
                theme_pixel: window.__state.theme_pixel, chat_rain_splash: true });
            }
            if (command === 'list_repositories') {
              return Promise.resolve(window.__state.repositories);
            }
            return Promise.resolve({ ok: true });
          }
        },
        event: { listen: function () { return Promise.resolve(function () {}); } }
      };
    }, state);
  }
  await page.setViewport({ width: 1180, height: 820 });
  await page.goto('file://' + path.resolve(PAGE));
  // The page face asks for the program mark at "/autumn-natter.svg", which the served page
  // answers. Under file:// nothing answers an address from the root of the disk, so the mark is
  // pointed at the file of the asset in this repository.
  if (process.env.AN_PAGE) {
    await page.evaluate(function () {
      var marks = document.querySelectorAll('img.mark[src^="/"]');
      for (var i = 0; i < marks.length; i++) {
        marks[i].src = '../assets/icons/autumn-natter.svg';
      }
    });
  }
  await sleep(400);
  return { browser: browser, page: page, errors: errors };
}

async function main() {
  const tag = process.env.AN_PAGE ? 'page' : 'window';
  const skins = ONLY || WORDS;
  const run = await start(stateOf(skins[0], false));
  for (const skin of skins) {
    for (const form of [false, true]) {
      await run.page.evaluate((given) => {
        window.__state = given;
        if (typeof showTheme === 'function') { showTheme(given.theme); }
        if (typeof showPixelArt === 'function') { showPixelArt(!!given.theme_pixel); }
        if (typeof poll === 'function') { poll(); }
      }, stateOf(skin, form));
      await sleep(500);
      const name = 'skinshot-' + tag + '-' + skin + (form ? '-pixel' : '');
      await run.page.screenshot({ path: path.join(__dirname, name + '.png') });
      // The settings panel, at the Program tab, where the markers of the five themes and of the
      // two forms remain.
      await run.page.evaluate(function () {
        if (typeof openSettings === 'function') { openSettings('program'); }
      });
      await sleep(400);
      await run.page.screenshot({ path: path.join(__dirname, name + '-sheet.png') });
      await run.page.evaluate(function () {
        if (typeof closeSettings === 'function') { closeSettings(); }
      });
      await sleep(200);
    }
  }
  if (run.errors.length) { console.log('page faults: ' + run.errors.slice(0, 3).join(' | ')); }
  await run.browser.close();
  console.log('shot ' + skins.join(', ') + ' (both forms)');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
