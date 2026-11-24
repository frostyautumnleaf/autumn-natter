// The badge of retrieval, the '+' badge beside the tools, the window of the sources and the
// section of the embedding model, checked in both pages of the program with no engine behind it.
//
// Nine items are checked, in each page:
// * the '+' badge is the first badge of the row, to the left of the badge of the tools, and the
//   badge of retrieval stands immediately to the right of it;
// * the menu of the '+' badge holds the two items, named as they must be named, and the field that
//   asks the system for an attachment is a file field of several files;
// * the badge of retrieval reports the switch, the number of the sources, and a folder with nothing
//   in it, and it posts the switch of the chat when it is clicked;
// * clicking the badge while no embedding model is installed opens the settings at the section of
//   the embedding model, and marks it;
// * the section holds the three choice lists of the model, its quantization and its vision
//   projector, with the sizes of the files, and a choice posts the choice;
// * the button of the download posts the install with the three words of the choice;
// * the window of the sources reports the folder, the count of the files and of their pieces, the
//   state of each file, and the reason a file is not counted;
// * a file given to the window is sent with the path it keeps inside `.sources`, and a folder of
//   more files than one ask takes is sent in batches, one after the job of the last has ended;
// * a file taken out of the index posts its path, and the window closes without a fault.
//
// node rag-check.js AN_PAGE=web/page.html AN_FETCH=1 node rag-check.js
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const PAGES = process.env.AN_PAGE
  ? [{ name: 'the page named', tag: 'face', file: path.resolve(path.join(__dirname, '..'), process.env.AN_PAGE) }]
  : [
    { name: 'the window', tag: 'window', file: path.join(__dirname, '..', 'frontend', 'index.html') },
    { name: 'the browser page', tag: 'page', file: path.join(__dirname, '..', 'web', 'page.html') }
  ];

const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function said(id, role, body) {
  return {
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0,
    attachments: [], tool: '', tool_name: '', mono: false, failed: false,
    cancelled: false, stopped: false, can_continue: false, when: '09:0' + id,
    downloads: [], displayed: [], suggestions: [],
  };
}

// A chat of a repository, with the sources off and no embedding model installed: the state of a
// program that was started and never asked about retrieval.
function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme || 'light', theme_pixel: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: 'r1', name: 'Autumn Notes' },
    repositories: [{ id: 'r1', name: 'Autumn Notes' }],
    chat: { id: 'c1', title: 'A chat with sources', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat with sources', repo: 'r1', when: 'Today 09:04',
      active: true, preview: 'Search the sources.' }],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      said('1', 'You', 'Search the sources for the token rule.'),
      said('2', 'Assistant', 'The token lasts an hour, as the auth note says.', { parts: [] }),
    ],
    live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [], live_tools: [], mcp: { servers: [] },
    thinking: { mode: 'off', available: true, modes: ['off'] },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
      chapters: true, paste_limit: 6000 },
    context: { used: '412', total: '8,192', ratio: 0.05 },
    generating: false, busy: false, code: true, search: false, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    // The badge of retrieval, as the state carries it: the switch of the chat, the counts of the
    // index, and whether anything at all can count the words of a source.
    rag: { enabled: false, files: 0, chunks: 0, pending: 0, ready: false, vision: false },
    queue: { chat: 'c1', held: false, items: [], planned: [] },
    status: 'ready', dark: theme === 'dark', data_dir: '/tmp/autumn-natter',
  };
}

// The answer of the section at the top of the Program tab: one model of the catalog, six
// quantizations of it, three projectors, and the download that is not going on.
function embedOf() {
  const quants = [
    { label: 'BF16', file: 'embeddinggemma-2-BF16.gguf', size: '558 MB' },
    { label: 'F16', file: 'embeddinggemma-2-F16.gguf', size: '558 MB' },
    { label: 'Q8_0', file: 'embeddinggemma-2-Q8_0.gguf', size: '310 MB' },
    { label: 'Q6_K_XL', file: 'embeddinggemma-2-UD-Q6_K_XL.gguf', size: '249 MB' },
    { label: 'Q5_K_XL', file: 'embeddinggemma-2-UD-Q5_K_XL.gguf', size: '210 MB' },
    { label: 'Q4_K_XL', file: 'embeddinggemma-2-UD-Q4_K_XL.gguf', size: '176 MB' },
  ].map((one, at) => Object.assign({}, one, { installed: at === 0, bytes: at === 0 ? 558000000 : 0 }));
  const mmprojs = [
    { label: 'BF16', file: 'mmproj-BF16.gguf', size: '982 MB' },
    { label: 'F16', file: 'mmproj-F16.gguf', size: '981 MB' },
    { label: 'Q8_0', file: 'mmproj-Q8_0.gguf', size: '555 MB' },
  ].map((one) => Object.assign({}, one, { installed: false, bytes: 0 }));
  return {
    ok: true, model: 'embeddinggemma-2', name: 'Embedding Gemma 2',
    note: 'Counts the words of a page.', quant: 'BF16', mmproj: 'none',
    models: [{ id: 'embeddinggemma-2', name: 'Embedding Gemma 2', note: 'Counts the words of a page.',
      repo: 'unsloth/embeddinggemma-2-GGUF', quants: quants, mmprojs: mmprojs }],
    quants: quants, mmprojs: mmprojs,
    installed: false, path: '', projector_used: '',
    server: { running: false, url: '', note: '' },
    run: { cpu_only: true, batch: 2048, threads: 0, ctx: 4096,
      limits: { batch_min: 64, batch_max: 16384, threads_max: 256, ctx_min: 512, ctx_max: 32768 } },
    job: { active: false, stage: '', file: '', done: 0, total: 0, note: '', failed: false, percent: 0 },
    dir: '/home/user/.config/autumn-natter/embedding',
  };
}

// The window of the sources: two files of a tree, one of them counted and one of them not, with
// the reason the second one is not.
function sourcesOf() {
  return {
    ok: true, enabled: true, folder: '/home/user/repos/notes_7/.sources',
    files: [
      { path: 'notes/auth.md', kind: 'text', bytes: 1224, words: 1180, chunks: 4,
        counted: true, fault: '', when: 'Today 09:04' },
      { path: 'notes/api/tokens.md', kind: 'text', bytes: 812, words: 720, chunks: 2,
        counted: false, fault: 'the counting of the file gave nothing', when: 'Today 09:05' },
      { path: 'pictures/board.png', kind: 'image', bytes: 40960, words: 0, chunks: 0,
        counted: false, fault: '', when: 'Today 09:06' },
    ],
    counts: { files: 3, chunks: 6, pending: 2 },
    job: { active: false, stage: '', file: '', done: 0, total: 0, note: '', failed: false, percent: 0 },
    rag: { files: 3, chunks: 6, pending: 2, ready: true, vision: false },
  };
}

// The three answers of the program the panels read, and the actions the page sends. The state is
// kept where the check may change it between the items.
function stubs(state, embed, sources) {
  // The running of the counting as the program answers with it: the four words the page sent, taken
  // back into their bounds, with the bounds themselves beside them. The helper lives inside `stubs`
  // because the stubs are carried into the page on their own, without the rest of this file.
  function runAfter(sent, before) {
    const limits = ((before && before.limits) || {});
    const low = (limits.batch_min || 64);
    const high = (limits.batch_max || 16384);
    const raw = Number(sent.batch);
    let batch = Number.isFinite(raw) ? Math.min(Math.max(raw, low), high) : 2048;
    batch = Math.floor(batch / 8) * 8;
    const threads = Math.min(Number(sent.threads) || 0, limits.threads_max || 256);
    const ctxLow = (limits.ctx_min || 512);
    const ctx = Math.floor(Math.min(Math.max(Number(sent.ctx) || 4096, ctxLow),
      limits.ctx_max || 32768) / 64) * 64;
    return {
      cpu_only: sent.cpu_only !== false,
      batch: batch,
      threads: threads,
      ctx: ctx,
      limits: {
        batch_min: low, batch_max: high,
        threads_max: limits.threads_max || 256,
        ctx_min: ctxLow, ctx_max: limits.ctx_max || 32768,
      },
    };
  }

  window.__rag = {
    state: state,
    embed: embed,
    sources: sources,
    actions: [],
  };
  window.__post = function (patch) {
    Object.assign(window.__rag.state, patch);
    window.__rag.state.rev += 1;
    window.__rag.state.unchanged = false;
  };
  window.__log = function (name, body) {
    window.__rag.actions.push({ name: name, body: body || {} });
  };
  window.__take = function () {
    const made = window.__rag.actions;
    window.__rag.actions = [];
    return made;
  };
  window.fetch = function (url, init) {
    const name = String(url).replace(/^\//, '').replace(/^api\//, '');
    let body = {};
    try { body = init && init.body ? JSON.parse(init.body) : {}; } catch (e) { body = {}; }
    window.__log(name, body);
    if (name.indexOf('state') === 0) { return Promise.resolve({ json: () => Promise.resolve(window.__rag.state) }); }
    if (name === 'embed_state') { return Promise.resolve({ json: () => Promise.resolve(window.__rag.embed) }); }
    if (name === 'sources_list') { return Promise.resolve({ json: () => Promise.resolve(window.__rag.sources) }); }
    if (name === 'embed_choose' || name === 'embed_install' || name === 'embed_run') {
      // The program writes the choice into its settings, and the section is read again from there.
      if (name === 'embed_choose') {
        if (body.model) { window.__rag.embed.model = body.model; }
        if (body.quant) { window.__rag.embed.quant = body.quant; }
        if (body.mmproj) { window.__rag.embed.mmproj = body.mmproj; }
      }
      // The running of the counting is written into the settings the same way, and the answer
      // carries it back within its bounds.
      if (name === 'embed_run') { window.__rag.embed.run = runAfter(body, window.__rag.embed.run); }
      if (name !== 'embed_run') {
        window.__rag.embed.installed = true;
        window.__rag.embed.path = '/home/user/.config/autumn-natter/embedding/model.gguf';
      }
    }
    return Promise.resolve({ json: () => Promise.resolve({ ok: true }) });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(window.__rag.state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'get_embed_state') { return Promise.resolve(window.__rag.embed); }
        if (command === 'get_sources') { return Promise.resolve(window.__rag.sources); }
        if (command === 'action') {
          const body = (args && args.body) || {};
          window.__log(String((args && args.path) || ''), body);
          if (args && args.path === 'embed_choose') {
            if (body.model) { window.__rag.embed.model = body.model; }
            if (body.quant) { window.__rag.embed.quant = body.quant; }
            if (body.mmproj) { window.__rag.embed.mmproj = body.mmproj; }
          }
          if (args && args.path === 'embed_install') {
            window.__rag.embed.installed = true;
            window.__rag.embed.path = '/home/user/.config/autumn-natter/embedding/model.gguf';
          }
          if (args && args.path === 'embed_run') {
            window.__rag.embed.run = runAfter(body, window.__rag.embed.run);
          }
        }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// The place of a thing in the page, and the words standing in it.
function badges() {
  const at = (id) => {
    const node = document.getElementById(id);
    if (!node) { return null; }
    const box = node.getBoundingClientRect();
    // The page moves the text of a `title` into `data-tip` for the tooltips of its own, so both
    // names of the text are read.
    const told = node.getAttribute('data-tip') || node.getAttribute('title') || '';
    return { left: Math.round(box.left), right: Math.round(box.right), top: Math.round(box.top),
      width: Math.round(box.width), shown: box.width > 0 && box.height > 0,
      text: (node.textContent || '').replace(/\s+/g, ' ').trim(),
      title: told,
      hidden: node.classList.contains('hidden') };
  };
  return { plus: at('plus'), code: at('code'), rag: at('rag'), search: at('search'),
    menu: at('plusmenu'), overlay: at('srcOverlay'), sheet: at('overlay') };
}

const WORDS = 'Drop files or folders here to add them to the sources';

async function main() {
  let trouble = 0;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  for (const one of PAGES) {
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };
    const faults = [];
    const page = await browser.newPage();
    await page.setViewport({ width: 1180, height: 820 });
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument('window.__stubs = ' + stubs.toString()
      + '; window.__badges = ' + badges.toString()
      + '; window.__WORDS = ' + JSON.stringify(WORDS) + ';');
    await page.evaluateOnNewDocument(
      'window.__stubs(' + JSON.stringify(stateOf('light')) + ','
      + JSON.stringify(embedOf()) + ',' + JSON.stringify(sourcesOf()) + ');');
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(450);
    if (faults.length) { note('page faults: ' + faults.join(' | ')); }

    // 1 and 2. the two new badges stand where they belong in the row of badges.
    let made = await page.evaluate('window.__badges()');
    if (!made.plus || !made.plus.shown) {
      note('the + badge of the chat composer is not there, so nothing was read of it');
    } else if (made.rag && made.rag.left < made.plus.right) {
      note('the + badge is not the first of the row: the badge of retrieval starts at '
        + made.rag.left + ' and the + badge ends at ' + made.plus.right);
    }
    if (!made.rag || !made.rag.shown) {
      note('the badge of retrieval is not in the row of badges');
    } else if (made.rag.left < made.code.right) {
      note('the badge of retrieval is not beside the badge of the tools: it starts at '
        + made.rag.left + ' where the badge of the tools ends at ' + made.code.right);
    }
    // The badge reports an empty folder of sources before any is added.
    if (made.rag && made.rag.text !== 'RAG') {
      note('the badge of retrieval reads "' + made.rag.text + '" where no source is added yet');
    }
    if (made.rag && made.rag.title.indexOf('No source is in the folder') < 0) {
      note('the badge of retrieval does not say that the folder holds no source: "'
        + made.rag.title + '"');
    }

    // 3. the menu of the + badge, and the field of the attachments.
    await page.click('#plus');
    await sleep(160);
    made = await page.evaluate('window.__badges()');
    if (made.menu.hidden) {
      note('the menu of the + badge did not open');
    } else {
      const lines = await page.evaluate(function () {
        return Array.prototype.slice.call(document.querySelectorAll('#plusmenu button'))
          .map(function (node) { return node.textContent.replace(/\s+/g, ' ').trim(); });
      });
      if (lines.join('|') !== 'Add attachments|Add RAG sources') {
        note('the menu of the + badge holds "' + lines.join('", "') + '"');
      }
    }
    const field = await page.evaluate(function () {
      const node = document.getElementById('pickfiles');
      return node ? { multiple: !!node.multiple, type: node.type, hidden: node.hidden } : null;
    });
    if (!field) {
      note('the field that asks the system for a file is not in the page');
    } else if (!field.multiple || field.type !== 'file' || !field.hidden) {
      note('the field of the attachments is a "' + field.type + '" field, multiple='
        + field.multiple + ', hidden=' + field.hidden);
    }
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-1-menu.png') });

    // 4. the badge of retrieval, with nothing installed to count the words of a source.
    await page.click('#rag');
    await sleep(420);
    let actions = await page.evaluate('window.__take()');
    let ragged = actions.filter((one) => one.name === 'rag');
    if (ragged.length !== 1) {
      note('clicking the badge of retrieval sent ' + ragged.length + ' actions of the switch');
    } else if (ragged[0].body.on !== true) {
      note('clicking the badge of retrieval sent on=' + ragged[0].body.on + ' instead of on=true');
    }
    made = await page.evaluate('window.__badges()');
    if (made.sheet.hidden) {
      note('with no embedding model installed, the badge of retrieval did not open the settings');
    }
    const aimed = await page.evaluate(function () {
      const node = document.getElementById('secEmbedding');
      return node
        ? { shown: !node.hidden, words: node.textContent.trim(), marked: node.classList.contains('aim') }
        : null;
    });
    if (!aimed) {
      note('the section of the embedding model is not in the Program tab');
    } else {
      if (aimed.words !== 'Embedding model') {
        note('the section of the embedding model is named "' + aimed.words + '"');
      }
      if (!aimed.marked) {
        note('the section of the embedding model was not marked out when the badge asked for it');
      }
    }
    // 5. the three choice lists, with the sizes of the files.
    const lists = await page.evaluate(function () {
      const lines = function (id) {
        const node = document.getElementById(id);
        if (!node) { return null; }
        return Array.prototype.slice.call(node.options).map(function (one) {
          return { value: one.value, text: one.textContent };
        });
      };
      return { model: lines('fEmbedModel'), quant: lines('fEmbedQuant'), mmproj: lines('fEmbedMmproj'),
        state: (document.getElementById('fEmbedState') || {}).textContent,
        folder: (document.getElementById('fEmbedFolder') || {}).textContent };
    });
    if (!lists.model || lists.model.length !== 1
        || lists.model[0].text !== 'Embedding Gemma 2') {
      note('the model list of the section is not at Embedding Gemma 2');
    }
    if (!lists.quant || lists.quant.length !== 6 || lists.quant[0].value !== 'BF16') {
      note('the quantization list holds ' + (lists.quant ? lists.quant.length : 0) + ' lines');
    } else {
      if (lists.quant[0].text.indexOf('558 MB') < 0) {
        note('the size of the file is not in the line of a quantization: "'
          + lists.quant[0].text + '"');
      }
      if (lists.quant[0].text.indexOf('installed') < 0) {
        note('a quantization that is in the folder is not named as one: "'
          + lists.quant[0].text + '"');
      }
    }
    if (!lists.mmproj) {
      note('the list of the vision projectors is not in the section');
    } else {
      const last = lists.mmproj[lists.mmproj.length - 1];
      if (!last || last.value !== 'none' || last.text !== 'No vision projector') {
        note('the vision projector may not be done without: the last line is "'
          + (last ? last.text : 'nothing') + '"');
      }
    }
    if (!lists.state || lists.state.indexOf('not installed') < 0) {
      note('the line of the state does not say the model is missing: "' + lists.state + '"');
    }
    if (!lists.folder || lists.folder.indexOf('embedding') < 0) {
      note('the folder of the embedding model is not reported: "' + lists.folder + '"');
    }
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-2-embedding.png') });

    // 5b. the running of the counting: the four fields are in the section, they hold the words the
    // program answered with, the bounds of the program are on them, and the line of the state says
    // what the counting was given.
    const running = await page.evaluate(function () {
      const field = function (id) {
        const node = document.getElementById(id);
        if (!node) { return null; }
        return node.type === 'checkbox'
          ? { checked: node.checked, shown: node.getBoundingClientRect().width > 0 }
          : { value: node.value, min: node.min, max: node.max,
              shown: node.getBoundingClientRect().width > 0 };
      };
      return {
        cpu: field('fEmbedCpu'), batch: field('fEmbedBatch'), threads: field('fEmbedThreads'),
        ctx: field('fEmbedCtx'),
        state: (document.getElementById('fEmbedState') || {}).textContent,
      };
    });
    if (!running.cpu || !running.batch || !running.threads || !running.ctx) {
      note('the section of the embedding model does not hold the four fields of the running');
    } else {
      if (!running.cpu.shown || !running.batch.shown || !running.threads.shown
          || !running.ctx.shown) {
        note('a field of the running is not drawn');
      }
      if (!running.cpu.checked) {
        note('the counting is not kept on the cores of the computer by default');
      }
      if (running.batch.value !== '2048' || running.batch.min !== '64'
          || running.batch.max !== '16384') {
        note('the batch of the counting is "' + running.batch.value + '" with the bounds '
          + running.batch.min + ' and ' + running.batch.max);
      }
      if (running.threads.value !== '0' || running.threads.max !== '256') {
        note('the threads of the counting are "' + running.threads.value + '" with the top '
          + running.threads.max);
      }
      if (running.ctx.value !== '4096' || running.ctx.min !== '512' || running.ctx.max !== '32768') {
        note('the length of one ask is "' + running.ctx.value + '" with the bounds '
          + running.ctx.min + ' and ' + running.ctx.max);
      }
      if (!running.state || running.state.indexOf('batch of 2048') < 0) {
        note('the line of the state says nothing of the running: "' + running.state + '"');
      }
    }

    // A change of one of the four numbers is written at once, and the answer of the program is read
    // back into the fields. The field being typed in is left alone, so the number is not written
    // over the typing.
    await page.evaluate(function () {
      const node = document.getElementById('fEmbedBatch');
      if (node) { node.focus(); node.value = '512'; }
    });
    await page.evaluate(function () {
      const node = document.getElementById('fEmbedBatch');
      if (node) { node.dispatchEvent(new Event('change', { bubbles: true })); }
    });
    await sleep(220);
    actions = await page.evaluate('window.__take()');
    let ran = actions.filter((one) => one.name === 'embed_run');
    if (ran.length !== 1) {
      note('a change of the batch sent ' + ran.length + ' actions of the running');
    } else if (ran[0].body.batch !== 512 || ran[0].body.cpu_only !== true
        || ran[0].body.ctx !== 4096) {
      note('the running sent was "' + JSON.stringify(ran[0].body) + '"');
    }
    // The toggle of the card is written the same way, and the line of the state answers with the
    // card once it is off.
    await page.evaluate(function () {
      const node = document.getElementById('fEmbedCpu');
      if (node) { node.checked = false; node.dispatchEvent(new Event('change', { bubbles: true })); }
    });
    await sleep(220);
    actions = await page.evaluate('window.__take()');
    ran = actions.filter((one) => one.name === 'embed_run');
    if (ran.length !== 1 || ran[0].body.cpu_only !== false || ran[0].body.batch !== 512) {
      note('the toggle of the card sent "' + JSON.stringify((ran[0] || {}).body) + '"');
    }
    const told = await page.evaluate(function () {
      const state = (document.getElementById('fEmbedState') || {}).textContent || '';
      const cpu = document.getElementById('fEmbedCpu');
      return { state: state, cpu: cpu ? cpu.checked : null,
        batch: (document.getElementById('fEmbedBatch') || {}).value };
    });
    if (told.cpu !== false) {
      note('the toggle of the card was put back on when the program answered with it off');
    }
    if (told.batch !== '512') {
      note('the batch of the counting reads "' + told.batch + '" after the program answered');
    }
    if (told.state.indexOf('the card') < 0 || told.state.indexOf('batch of 512') < 0) {
      note('the line of the state does not follow the running: "' + told.state + '"');
    }
    await page.evaluate(function () {
      const node = document.getElementById('fEmbedCtx');
      if (node) { node.scrollIntoView({ block: 'center' }); }
    });
    await sleep(150);
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-6-run.png') });
    // The four words are put back as the program settled them, so the items after this one read the
    // state they were written for.
    await page.evaluate(function () {
      const batch = document.getElementById('fEmbedBatch');
      if (batch) { batch.blur(); batch.value = '2048'; batch.dispatchEvent(new Event('change', { bubbles: true })); }
      const cpu = document.getElementById('fEmbedCpu');
      if (cpu) { cpu.checked = true; cpu.dispatchEvent(new Event('change', { bubbles: true })); }
    });
    await sleep(220);
    await page.evaluate('window.__take()');

    // 6. a choice of the list is written at once, and the download is a press of its own.
    await page.select('#fEmbedQuant', 'Q8_0');
    await sleep(220);
    actions = await page.evaluate('window.__take()');
    let chosen = actions.filter((one) => one.name === 'embed_choose');
    if (chosen.length !== 1) {
      note('a change of the quantization sent ' + chosen.length + ' actions of the choice');
    } else if (chosen[0].body.quant !== 'Q8_0' || !chosen[0].body.model) {
      note('the choice sent was "' + chosen[0].body.quant + '" of model "'
        + chosen[0].body.model + '"');
    }
    await page.click('#embedInstall');
    await sleep(220);
    actions = await page.evaluate('window.__take()');
    const installed = actions.filter((one) => one.name === 'embed_install');
    if (installed.length !== 1) {
      note('the button of the download sent ' + installed.length + ' actions');
    } else if (installed[0].body.quant !== 'Q8_0' || installed[0].body.mmproj !== 'none') {
      note('the download was asked for "' + installed[0].body.quant + '" and projector "'
        + installed[0].body.mmproj + '"');
    }
    // The settings stay the settings of the chat after a badge opened them: they are closed again
    // for the window of the sources.
    await page.evaluate(function () { closeSettings(); });
    await sleep(120);

    // 7. the window of the sources, opened from the menu of the + badge.
    await page.click('#plus');
    await sleep(120);
    await page.click('#plussources');
    await sleep(320);
    made = await page.evaluate('window.__badges()');
    if (made.overlay.hidden) {
      note('the window of the sources did not open');
    } else {
      const seen = await page.evaluate(function () {
        const at = function (id) { return (document.getElementById(id) || {}).textContent || ''; };
        return { say: at('srcDrop').replace(/\s+/g, ' ').trim(), meta: at('srcMeta'),
          count: at('srcCount'), rows: document.querySelectorAll('#srcList .src-row').length,
          faults: Array.prototype.slice.call(document.querySelectorAll('#srcList .meta .wait'))
            .map(function (node) { return node.textContent.replace(/\s+/g, ' ').trim(); }),
          again: (document.getElementById('srcRecount') || {}).disabled,
          clear: (document.getElementById('srcClear') || {}).disabled };
      });
      if (seen.say.indexOf(WORDS) < 0) {
        note('the drop zone of the sources reads "' + seen.say.slice(0, 90) + '"');
      }
      if (seen.meta.indexOf('.sources') < 0 || seen.meta.indexOf('3 files') < 0) {
        note('the line of the window of the sources reads "' + seen.meta + '"');
      }
      if (seen.rows !== 3) {
        note('the window of the sources shows ' + seen.rows + ' rows where three files are kept');
      }
      if (seen.faults.length !== 2) {
        note('the window of the sources reports ' + seen.faults.length
          + ' files that wait or failed, where two do');
      } else if (seen.faults[0].indexOf('the counting of the file gave nothing') < 0) {
        note('the reason a file is not counted is not in its row: "' + seen.faults[0] + '"');
      }
      if (seen.count.indexOf('2 waiting to be counted') < 0) {
        note('the line of the count reads "' + seen.count + '"');
      }
      if (seen.again !== false || seen.clear !== false) {
        note('the two buttons of the window are not both to hand: count again disabled=' + seen.again
          + ', take all out disabled=' + seen.clear);
      }
    }
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-3-sources.png') });

    // 8. a file of the page is sent with the path it keeps inside the folder, and a big folder is
    // sent in batches, one after the job of the last has ended.
    await page.evaluate(function () {
      const file = new File(['# Auth\n\nThe token lasts an hour.\n'], 'auth.md',
        { type: 'text/markdown' });
      srcGive([{ file: file, path: 'notes/auth.md' }]);
    });
    await sleep(240);
    actions = await page.evaluate('window.__take()');
    let added = actions.filter((one) => one.name === 'sources_add');
    if (added.length !== 1) {
      note('one file given to the window sent ' + added.length + ' actions of adding');
    } else {
      const files = (added[0].body.files || []);
      if (files.length !== 1 || files[0].path !== 'notes/auth.md'
          || String(files[0].data).indexOf('data:') !== 0) {
        note('the file was sent as ' + JSON.stringify(files[0] || {}).slice(0, 120));
      }
    }
    // Twenty-five files at once: the first ask takes twenty, and the rest follow when the program
    // has done with the job of them.
    await page.evaluate(function () {
      window.__rag.sources.job = { active: true, stage: 'storing', file: 'f1.txt', done: 1, total: 20,
        note: '', failed: false, percent: 5 };
      const items = [];
      for (let n = 1; n <= 25; n++) {
        items.push({ file: new File(['text of file ' + n], 'f' + n + '.txt', { type: 'text/plain' }),
          path: 'deep/f' + n + '.txt' });
      }
      srcGive(items);
    });
    await sleep(400);
    actions = await page.evaluate('window.__take()');
    added = actions.filter((one) => one.name === 'sources_add');
    if (added.length !== 1 || (added[0].body.files || []).length !== 20) {
      note('while a job is going on, ' + added.length + ' ask(s) of adding came, the first of '
        + ((added[0] && added[0].body.files) || []).length + ' files');
    }
    await page.evaluate(function () {
      window.__rag.sources.job = { active: false, stage: 'storing', file: '', done: 20, total: 20,
        note: '20 files are in the sources', failed: false, percent: 100 };
    });
    await sleep(1600);
    actions = await page.evaluate('window.__take()');
    added = actions.filter((one) => one.name === 'sources_add');
    if (added.length !== 1 || (added[0].body.files || []).length !== 5) {
      note('the rest of the folder came as ' + added.length + ' ask(s) of '
        + ((added[0] && added[0].body.files) || []).length + ' files');
    }
    // The note of the job that ended is read in the window, for it tells how many arrived.
    const noted = await page.evaluate(function () {
      const node = document.getElementById('srcNote');
      return node ? { text: node.textContent.replace(/\s+/g, ' ').trim(),
        hidden: node.classList.contains('hidden') } : null;
    });
    if (!noted || noted.hidden || noted.text.indexOf('20 files are in the sources') < 0) {
      note('the window of the sources does not report how the job ended: "'
        + (noted ? noted.text : 'nothing') + '"');
    }

    // A file is taken out of the index by the button of its row, and the window closes.
    await page.evaluate(function () {
      const row = document.querySelector('#srcList .src-row .acts button');
      if (row) { row.click(); }
    });
    await sleep(220);
    actions = await page.evaluate('window.__take()');
    const gone = actions.filter((one) => one.name === 'sources_remove');
    if (gone.length !== 1 || (gone[0].body.paths || []).join() !== 'notes/auth.md') {
      note('the button of a row sent "' + JSON.stringify((gone[0] || {}).body || {}) + '"');
    }
    await page.click('#srcClose');
    await sleep(160);
    made = await page.evaluate('window.__badges()');
    if (!made.overlay.hidden) {
      note('the window of the sources did not close');
    }

    // The badge of retrieval, with the index filled and a model installed: the count is in the
    // badge, and the switch is a switch.
    await page.evaluate(function () {
      window.__post({ rag: { enabled: true, files: 3, chunks: 6, pending: 0, ready: true, vision: true } });
    });
    await sleep(320);
    made = await page.evaluate('window.__badges()');
    if (made.rag.text !== 'RAG 3') {
      note('the badge of retrieval reads "' + made.rag.text + '" where three files are indexed');
    }
    if (made.rag.title.indexOf('3 files') < 0 || made.rag.title.indexOf('6 pieces') < 0) {
      note('the badge of retrieval does not tell the count of the index: "' + made.rag.title + '"');
    }
    if (made.rag.title.indexOf('Pictures are counted') < 0) {
      note('a counting model with a vision projector is not reported by the badge: "'
        + made.rag.title + '"');
    }
    await page.click('#rag');
    await sleep(240);
    actions = await page.evaluate('window.__take()');
    const off = actions.filter((one) => one.name === 'rag');
    if (off.length !== 1 || off[0].body.on !== false) {
      note('the badge of retrieval with a model installed sent on='
        + ((off[0] || {}).body || {}).on);
    }
    made = await page.evaluate('window.__badges()');
    if (!made.sheet.hidden) {
      note('the settings were opened although an embedding model is installed');
    }

    // The dark theme of the same three places.
    await page.evaluate(function () {
      window.__post({ theme: 'dark', dark: true });
    });
    await sleep(320);
    await page.click('#plus');
    await sleep(160);
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-4-dark-menu.png') });
    await page.click('#plussources');
    await sleep(320);
    await page.screenshot({ path: path.join(__dirname, 'rag-' + one.tag + '-5-dark-sources.png') });
    if (faults.length) { note('page faults at the end: ' + faults.join(' | ')); }
    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' pages'); }
  console.log('the badge of retrieval, the + badge, the window of the sources and the section of the'
    + ' embedding model answer as they should, in both pages');
}

main().catch((e) => { console.error(e); process.exit(1); });
