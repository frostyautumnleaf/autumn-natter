// The badges that hold a drop-down answer two places of themselves. The right of
// the badge, from its arrow to its edge, is the way into the menu; the rest of
// the badge is the switch of the thing it stands for, and where that thing is a
// group of switches with none of them on, the badge opens the menu instead of
// switching nothing. The right button of the mouse opens the menu anywhere over
// the badge. The jail is put on and off from its badge and is no line in the
// menu of it, and the menu of the tools holds no line for the badge itself.
//
//   node badge-check.js
//   AN_PAGE=web/page.html AN_FETCH=1 node badge-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const USE_FETCH = process.env.AN_FETCH === '1';

// The state of the engine, as the record of web::state. `tools` says whether the
// kinds of tool stand switched on, and `all` whether the badge of the tools is.
function stateOf(tools, all) {
  const held = {};
  ['commands', 'files', 'picture', 'hand_out', 'display', 'ask', 'suggest',
   'infographics'].forEach(function (part) {
    held[part] = tools;
  });
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'Badges' },
    chats: [{ id: 'c1', title: 'Badges', repo: '', when: 'Today 09:04', active: true,
      preview: 'Which badge' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [], live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [],
    mcp: { servers: [{ id: 's1', slug: 'one', name: 'One server', icon: 2, on: false,
      shown: true, tools: ['read_a_thing'] }] },
    thinking: { mode: 'off', available: true, modes: ['off', 'low'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: all, sandbox: false,
    tool_switches: held,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
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
      window.fetch = function (url, init) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () { return Promise.resolve(state); } });
        }
        if (init && init.body) {
          try { window.__posted.push({ path: String(url), body: JSON.parse(init.body) }); }
          catch (e) { window.__posted.push({ path: String(url) }); }
        }
        return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
      };
    }, state);
  } else {
    await page.evaluateOnNewDocument(function (state) {
      window.__posted = [];
      window.__TAURI__ = {
        core: {
          invoke: function (command, args) {
            if (command === 'get_state') { return Promise.resolve(state); }
            if (command === 'get_config') { return Promise.resolve({}); }
            if (command === 'action') {
              window.__posted.push({ path: '/api/' + ((args && args.path) || ''),
                body: (args && args.body) || {} });
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
  await new Promise(function (r) { setTimeout(r, 400); });
  return { browser: browser, page: page, errors: errors };
}

// Where the two places of a badge are: the left of it, over its mark, and the
// right of it, over its arrow and the air to the edge of the badge.
async function places(page, sel) {
  return page.evaluate(function (sel) {
    var badge = document.querySelector(sel);
    var b = badge.getBoundingClientRect();
    var d = badge.querySelector('.down').getBoundingClientRect();
    return { y: b.top + b.height / 2, left: b.left + 6, arrow: d.left + d.width / 2,
      edge: (d.right + b.right) / 2 };
  }, sel);
}

const open = function (page, id) {
  return page.evaluate(function (id) {
    var menu = document.getElementById(id);
    return !!menu && !menu.classList.contains('hidden');
  }, id);
};
const words = function (page, id) {
  return page.evaluate(function (id) {
    var held = [];
    var lines = document.querySelectorAll('#' + id + ' .word');
    for (var n = 0; n < lines.length; n++) { held.push(lines[n].textContent); }
    return held;
  }, id);
};
const posted = function (page) {
  // The page asks for the lists of models and repositories when it comes up, so
  // those are no answer to what a badge did.
  return page.evaluate(function () {
    return window.__posted.filter(function (one) {
      return one.path !== '/api/lists' && one.path !== '/api/state';
    });
  });
};

async function main() {
  // The chat that offers its tools: the badge has something to switch.
  let run = await start(stateOf(true, true));
  const where = await places(run.page, '#code');
  await run.page.mouse.click(where.left, where.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  let went = await posted(run.page);
  if (went.length !== 1 || went[0].path !== '/api/code' || went[0].body.on !== false) {
    throw new Error('the badge of the tools did not switch all of them: ' + JSON.stringify(went));
  }
  if (await open(run.page, 'codemenu')) { throw new Error('the menu came up over a switch'); }

  await run.page.mouse.click(where.arrow, where.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'codemenu')) { throw new Error('the arrow did not open the menu'); }
  went = await posted(run.page);
  if (went.length !== 1) { throw new Error('the arrow switched as well as opened: ' + JSON.stringify(went)); }
  const lines = await words(run.page, 'codemenu');
  if (lines.indexOf('All the tools') >= 0) { throw new Error('the badge is a line of its own menu'); }
  // Every kind of tool of the program is a line of the menu, in the order the
  // list of the lines gives them. The line of the handoff summary stands only
  // while a summary is chosen, so it is not looked for here.
  const named = ['Terminal commands', 'File management', 'View images', 'Offer files',
    'Show files', 'Ask questions', 'Suggest next steps', 'Create infographics'];
  if (lines.join('|') !== named.join('|')) {
    throw new Error('the menu of the tools is not named for what it lets through: ' + lines);
  }

  await run.page.mouse.click(5, 5);
  await new Promise(function (r) { setTimeout(r, 120); });
  await run.page.mouse.click(where.edge, where.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'codemenu')) {
    throw new Error('the right of the badge, past its arrow, is not the way into the menu');
  }
  await run.page.mouse.click(5, 5);
  await new Promise(function (r) { setTimeout(r, 120); });
  await run.page.mouse.click(where.left, where.y, { button: 'right' });
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'codemenu')) { throw new Error('the right button did not open the menu'); }
  const think = await places(run.page, '#think');
  await run.page.mouse.click(think.left, think.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'thinkmenu')) { throw new Error('the thinking badge lost its way'); }
  const servers = await places(run.page, '#mcp');
  await run.page.mouse.click(servers.left, servers.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'mcpmenu')) { throw new Error('the badge of the servers lost its way'); }
  if (run.errors.length) { throw new Error('page faults: ' + run.errors.join(' | ')); }
  await run.browser.close();

  // The jail: the badge puts it on and off whatever its three settings say, the
  // right of the badge opens the settings, and the jail is no line in the menu.
  run = await start(stateOf(true, true));
  const sb = await places(run.page, '#sandbox');
  await run.page.mouse.click(sb.left, sb.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  went = await posted(run.page);
  if (went.length !== 1 || went[0].path !== '/api/sandbox' || went[0].body.on !== true) {
    throw new Error('the badge of the sandbox did not put the jail on: ' + JSON.stringify(went));
  }
  if (await open(run.page, 'sandboxmenu')) {
    throw new Error('the badge of the sandbox opened its menu instead of switching');
  }
  await run.page.mouse.click(sb.arrow, sb.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  if (!await open(run.page, 'sandboxmenu')) { throw new Error('the arrow of the sandbox lost its way'); }
  const sandboxLines = await words(run.page, 'sandboxmenu');
  if (sandboxLines.indexOf('Sandbox') >= 0) { throw new Error('the jail is a line of its own menu'); }
  if (sandboxLines.indexOf('Network access') < 0) { throw new Error('the gates are gone from the menu'); }
  if (run.errors.length) { throw new Error('page faults: ' + run.errors.join(' | ')); }
  await run.browser.close();

  // The chat whose tools are all taken away: the badge has nothing to switch, so
  // it opens the menu where the switches stand.
  run = await start(stateOf(false, true));
  const none = await places(run.page, '#code');
  await run.page.mouse.click(none.left, none.y);
  await new Promise(function (r) { setTimeout(r, 120); });
  went = await posted(run.page);
  if (went.length) { throw new Error('the badge switched with nothing switched on: ' + JSON.stringify(went)); }
  if (!await open(run.page, 'codemenu')) {
    throw new Error('the badge did not open its menu with none of its tools on');
  }
  if (run.errors.length) { throw new Error('page faults: ' + run.errors.join(' | ')); }
  await run.browser.close();

  console.log('the right of a badge opens its menu and the rest of it switches,'
    + ' the right button opens the menu, the jail is switched from its badge and is'
    + ' no line of its menu, and a badge with nothing switched on opens the menu');
}

main().catch(function (e) { console.error(String(e)); process.exit(1); });
