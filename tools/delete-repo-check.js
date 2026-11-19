// Taking a repository away is requested by name, so one stray click cannot take a whole
// repository out of the side bar. The menu of a repository holds a delete entry; it opens the
// window of the request with a hint line that shows the name, and the button stays dead until
// the box holds that name. Typing anything else and pressing Enter leaves the window open and
// markers the box. Once the name is right, accepting posts one action for the engine, and the
// engine takes the record, the chats, the saved context, the plans and the tasks out and moves
// the folder aside rather than destroying it.
//
// Both pages are checked: the window of the program and the page served by --remote carry the
// same menu and the same window of the request.
//
// node delete-repo-check.js AN_PAGE=web/page.html AN_FETCH=1 node delete-repo-check.js
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const REPO = { id: 'r1', name: 'Autumn Notes', pinned: false };

// The page requests its state and for its lists on its own. Only the actions a user of this
// check cares about are counted, and the counting is done in the page, where the list of them
// remains.

function stateOf() {
  return {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: 'r1', name: 'Autumn Notes' },
    repositories: [{ id: 'r1', name: 'Autumn Notes', path: '/work/autumn_notes_r1', pinned: false, last_used: 1 }],
    chat: { id: 'c1', title: 'A chat of that repository' },
    chats: [{ id: 'c1', title: 'A chat of that repository', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'Take the repository away.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Take the repository away.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// Both bridges are stubbed, so the real `post` of each page runs and lands in the same list of
// actions. The names are brought to one form: the window speaks of `delete_repo`, the page of
// `/api/delete_repo`.
function stubs(state) {
  window.__actions = [];
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    let body = {};
    try { body = JSON.parse((init && init.body) || '{}'); } catch (e) { /* left as nothing */ }
    window.__actions.push({ name: name, body: body });
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: function () { return Promise.resolve(state); } }); }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') {
          const name = String((args && args.path) || '').replace(/^\//, '').replace(/^api\//, '');
          window.__actions.push({ name: name, body: (args && args.body) || {} });
        }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

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
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString());
    await page.evaluateOnNewDocument('window.__stubs(' + JSON.stringify(stateOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(400);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    const opened = await page.evaluate((repo) => {
      side.pointerX = 40; side.pointerY = 40;
      sideOpenMenu('repo', repo, null, null);
      const menu = document.getElementById('sideMenu');
      const bad = Array.from(menu.querySelectorAll('button.bad'));
      const words = Array.from(menu.querySelectorAll('button')).map((b) => b.textContent);
      let hit = null;
      bad.forEach((b) => { if (b.textContent === 'Delete repository') { hit = b; } });
      const ask = document.getElementById('sideAsk');
      const hint = document.getElementById('askHint');
      const ok = document.getElementById('askOk');
      if (hit) { hit.click(); }
      return {
        hasDelete: !!hit,
        menuWords: words,
        open: ask.classList.contains('on'),
        asking: ask.classList.contains('asking'),
        mode: document.getElementById('askInput').dataset.ask,
        title: document.getElementById('askTitle').textContent,
        hintOn: hint.classList.contains('on'),
        hintText: hint.textContent,
        okLabel: ok.textContent,
        okDisabled: ok.disabled,
      };
    }, REPO);

    // A wrong name keeps the entry point shut, and the Enter key is answered with the window
    // staying open rather than with the repository going.
    const wrong = await page.evaluate(() => {
      const input = document.getElementById('askInput');
      input.value = 'Autumn';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      const stillShut = document.getElementById('askOk').disabled;
      sideAskGo(true);
      const ask = document.getElementById('sideAsk');
      return {
        stillShut: stillShut,
        open: ask.classList.contains('on'),
        wrong: input.classList.contains('wrong'),
        hintBad: document.getElementById('askHint').classList.contains('bad'),
        actions: window.__actions.filter((a) => a.name.indexOf('state') !== 0 && a.name !== 'lists'),
      };
    });

    // The right name, written in another case and with spaces around it, opens the entry point,
    // and accepting requests one item.
    const right = await page.evaluate(() => {
      const input = document.getElementById('askInput');
      input.value = '  AUTUMN notes  ';
      input.dispatchEvent(new Event('input', { bubbles: true }));
      const ok = document.getElementById('askOk');
      const before = { enabled: !ok.disabled, wrong: input.classList.contains('wrong') };
      ok.click();
      const ask = document.getElementById('sideAsk');
      return {
        enabled: before.enabled,
        wrongGone: !before.wrong,
        open: ask.classList.contains('on'),
        actions: window.__actions.filter((a) => a.name.indexOf('state') !== 0 && a.name !== 'lists'),
      };
    });

    // The window is shared with the other asks, so the name mode has to put the window back: a
    // later request text comes with a live button and no hint.
    const plain = await page.evaluate(() => {
      sideAskText('Name this chat', 'The name is in the side bar.', 'A chat', function () {});
      const hint = document.getElementById('askHint');
      const ok = document.getElementById('askOk');
      const named = {
        asking: document.getElementById('sideAsk').classList.contains('asking'),
        hintOn: hint.classList.contains('on'),
        hintText: hint.textContent,
        okDisabled: ok.disabled,
        bad: ok.classList.contains('bad'),
      };
      document.getElementById('askInput').value = 'Renamed';
      document.getElementById('askOk').click();
      const yes = { asking: false };
      sideAskYes('Delete this chat?', 'The text is removed.', 'Delete', function () {
        post('/api/delete_chat', { id: 'c1' });
      });
      yes.shown = document.getElementById('sideAsk').classList.contains('on');
      yes.bad = document.getElementById('askOk').classList.contains('bad');
      yes.disabled = document.getElementById('askOk').disabled;
      document.getElementById('askOk').click();
      return { named: named, yes: yes, actions: window.__actions.filter((a) => a.name.indexOf('state') !== 0 && a.name !== 'lists') };
    });

    await page.close();

    if (faults.length) { note('page faults: ' + faults.join(' | ')); }
    if (!opened.hasDelete) { note('no delete entry in the menu of a repository'); }
    if (opened.menuWords && opened.menuWords.indexOf('Work in this repository') < 0) {
      note('the menu of a repository lost its other entries: ' + opened.menuWords.join(', '));
    }
    if (!opened.open || !opened.asking) { note('the name ask did not open'); }
    if (opened.mode !== 'name') { note('the ask is in mode "' + opened.mode + '", not name'); }
    if (!opened.hintOn || opened.hintText.indexOf(REPO.name) < 0) {
      note('the hint does not say the name: "' + opened.hintText + '"');
    }
    if (opened.okLabel !== 'Delete repository') { note('the button says "' + opened.okLabel + '"'); }
    if (!opened.okDisabled) { note('the button lives while the box is empty'); }
    if (!wrong.stillShut) { note('the button lives while the name is wrong'); }
    if (!wrong.open) { note('a wrong name closed the window'); }
    if (!wrong.wrong || !wrong.hintBad) { note('a wrong name is not flagged as one'); }
    if (wrong.actions.length) { note('a wrong name posted: ' + JSON.stringify(wrong.actions)); }
    if (!right.enabled) { note('the right name did not start the button'); }
    if (!right.wrongGone) { note('the marker of a wrong name stayed on after typing it right'); }
    if (right.open) { note('the window stayed open after the name was accepted'); }
    const made = right.actions.filter((a) => a.name === 'delete_repo');
    if (made.length !== 1) {
      note('accepted by name, and ' + made.length + ' delete_repo actions were posted: '
        + JSON.stringify(right.actions));
    } else if (!made[0].body || made[0].body.id !== REPO.id) {
      note('the action carries ' + JSON.stringify(made[0].body) + ', not the id of the repository');
    }
    if (!plain.named.asking) { note('the request for text is no longer in text mode'); }
    if (plain.named.hintOn || plain.named.hintText) { note('the hint line was left in the next ask'); }
    if (plain.named.okDisabled) { note('the button of the next ask is dead'); }
    if (plain.named.bad) { note('the button of a naming ask is flagged as damaging'); }
    if (!plain.yes.shown || !plain.yes.bad || plain.yes.disabled) {
      note('the yes ask appeared wrong: ' + JSON.stringify(plain.yes));
    }
    const chats = plain.actions.filter((a) => a.name === 'delete_chat');
    if (chats.length !== 1) { note('the chat delete posted ' + chats.length + ' times'); }

    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' faces'); }
  console.log('the name of a repository is requested before it goes, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
