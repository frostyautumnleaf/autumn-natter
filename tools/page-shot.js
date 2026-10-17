// Pictures of the two pages of Autumn Natter, for the machine that has no
// desktop.
//
// The window cannot be opened in a build box: there is no display. This tool
// draws the pages in a headless browser instead, with a fake bridge, and writes
// the pictures as PNG. The pages are read as they stand, so a fault of the
// layout is seen here and not on the screen of the user.
//
// The core is not touched: no server is started, no model is read. The records
// below stand for the state of the engine, and the bridge answers as Tauri or
// as the small HTTP server of the remote mode.
//
//   npm install puppeteer           # once, in this folder, needs the network
//   node page-shot.js rich-pale rich.png 1180 1450
//   node page-shot.js home-dark home.png 1180 760
//   AN_PAGE=../web/page.html AN_FETCH=1 node page-shot.js chat-pale page.png
//   AN_PICK='.think .thead' AN_HOVER='.turn' node page-shot.js rich p.png
//   AN_CLICK='#think' node page-shot.js chat p.png
//   AN_CHAPTERS=12 node page-shot.js rich p.png   # that many marks on the line
//   AN_NOCHAPTERS=1 node page-shot.js rich p.png  # the Chatting switch turned off
//   AN_PICK, AN_HOVER and AN_CLICK pick out parts of the page by their class.
//
// The scenes: home, chat, rich, markdown, streaming, streaming-code, writing,
// processing, previews, sheet-program, sheet-model. A name that ends in -dark draws the dark skin, and
// a name that ends in -pale draws the pale one. AN_SCROLL=top puts the talk at
// the top instead of the end.
//   AN_PROGRESS=42 AN_WHAT=image node page-shot.js processing p.png
//   AN_PROGRESS and AN_WHAT stand for the gauge above the chat box.

'use strict';

const path = require('path');
const { execSync } = require('child_process');

const page = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const useFetch = process.env.AN_FETCH === '1';

// One message of the talk.
function said(id, role, body, extra) {
  return Object.assign({
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + (id % 10)
  }, extra || {});
}

// One chat with every kind of block in it: a heading, a paragraph, a list, a
// block of code, the thinking of the model, two runs of the terminal tool, and
// a line from the program. The first run of the tool came out of a turn with
// no words of its own, so the thinking of that turn stands above the command.
const FULL = [
  said(1, 'You', 'Can you show the plan for the parser, and run the tests?'),
  said(2, 'Assistant',
    '# Plan\n\nWe change two files.\n\n'
    + '1. `markdown.rs` closes a paragraph when a list starts.\n'
    + '2. `message_view.rs` gives the bar a height.\n\n'
    + 'The tests cover the order now.\n\n'
    + '```rust\nfn flush(&mut self) {\n    self.close_para();\n}\n```',
    { reasoning: 'The user wants a **plan**. The fault is in `markdown.rs`, and not '
      + 'in the view. Two things to check first:\n\n'
      + '- the order the blocks come out in\n- the height of the bar beside a heading\n\n'
      + '```rust\nif list {\n    flush_para();\n}\n```',
      // How long the model thought for, as the head of the panel says it.
      reasoning_ms: 80000 }),
  said(3, 'Tool', 'cargo test --release markdown',
    { reasoning: 'The plan is written. Run the tests of the parser before the '
      + 'answer goes out, so the plan is checked and not only promised.',
      reasoning_ms: 320000,
      tool: 'running 6 tests\ntest markdown::tests::list_after_paragraph ... ok\n'
      + 'all 6 tests passed' }),
  said(4, 'Assistant', 'The tests pass. Two notes:\n\n'
    + '- The bar takes its height from the words beside it.\n'
    + '- A list that follows a paragraph keeps its place.\n\n'
    + '```python\n'
    + '# the bar takes a height from the words beside it\n'
    + 'def height(words, size=15):\n'
    + '    line = words.count("\\n") + 1\n'
    + '    return 1.55 * size * line   # points\n'
    + '```'),
  said(5, 'Note', 'cannot reach the model: connection refused', { failed: true }),
  said(6, 'Tool', 'bwrap --die-with-parent ls -1 /nope',
    { tool: 'exit 1: no such file', failed: true }),
];

const TALK = [
  said(1, 'You', 'What is the difference between mmap and mlock?'),
  said(2, 'Assistant', 'mmap lets the system page the model in and out. mlock keeps '
    + 'the whole file in the memory of the machine.\n\nUse mlock when the model must '
    + 'not be moved.'),
  said(3, 'You', 'Which one is the default?'),
  said(4, 'Assistant', 'mmap is the default. It costs less memory.'),
];

// The command the model writes in the `writing` scene, and the reason it gives
// for it. AN_WRITE counts the characters of both, one after the other.
const WRITING = 'cargo test --release markdown -- --nocapture';
const WRITING_WHY = 'see whether the order of the blocks is mended';
// What the model says before the command, for AN_SAYS.
const WRITING_SAYS = 'I will write a short greeting to a file called hello.txt, '
  + 'and I will read it back to you.';

// The chats of the place, with one of them open.
function chats(count, open) {
  const out = [];
  for (let n = 0; n < count; n++) {
    out.push({
      id: 'c' + n, title: n === open ? 'Parser order' : 'Chat ' + (n + 1),
      repo: '', when: 'Today 09:0' + (n % 10), active: n === open,
      preview: n === open ? 'Nothing written yet' : 'The last line of that chat, said short'
    });
  }
  return out;
}

// The state of the engine, as the record of web::state.
function scene(name, dark) {
  const start = {
    rev: 1, unchanged: false,
    model: { id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF' },
    models: [
      { id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF' },
      { id: 'm2', name: 'DeepSeek-R1-7B-Q4_K_M', kind: 'GGUF' },
      { id: 'm3', name: 'gemma-3-4b-it', kind: 'Safetensors' }
    ],
    repository: { id: '', name: 'No repository' },
    repositories: [{ id: 'r1', name: 'notes' }, { id: 'r2', name: 'kernel-ports' }],
    chat: { id: 'c1', title: 'No chat' },
    chats: [], home: true, greeting: 'Still here, still ready.', overview: false,
    carousel: { index: 0, count: 0, blank: true },
    messages: [], live: '', reasoning: '', reasoning_ms: 0, reasoning_done: false,
    // The calls of a tool the model is writing right now. A block of a command
    // being written is made of these.
    live_tools: [],
    // How far the server is with the words it was sent. The gauge above the
    // chat box stands on these three.
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [],
    thinking: {
      mode: 'off', available: true,
      modes: ['off', 'low', 'medium', 'high', 'xhigh']
    },
    chatting: { collapse_thinking: true, collapse_tools: true, highlight: true,
                chapters: true },
    context: { used: '0', total: '8,192', ratio: 0.0 },
    generating: false, busy: false, code: false, sandbox: false,
    sandbox_available: true,
    sandbox_note: 'bubblewrap (bwrap) was found, so the tool can be kept inside a jail.',
    status: 'Ready', dark: !!dark, data_dir: '/home/user/.local/share/autumn-natter'
  };
  if (name === 'chat' || name === 'rich' || name === 'streaming' || name === 'streaming-code'
    || name === 'previews') {
    start.home = false;
    start.chat = { id: 'c1', title: name === 'rich' ? 'Parser order' : 'Memory of the talk' };
    start.chats = chats(name === 'previews' ? 9 : 3, 1);
    start.carousel = { index: 1, count: start.chats.length, blank: false };
    start.messages = name === 'rich' ? FULL : TALK;
    // The chapters the model marked, tied to the message each begins after.
    start.chapters = name === 'rich'
      ? [{ id: 'ch1', name: 'The plan for the parser', after: 1 },
         { id: 'ch2', name: 'Running the tests', after: 3 },
         { id: 'ch3', name: 'The bar beside a list', after: 4 }]
      : [];
    // AN_CHAPTERS=N puts that many marks on the line, spread over the talk, so
    // the line can be looked at when a chat has more chapters than room. Zero
    // means no chapter at all. AN_NOCHAPTERS=1 leaves the marks in the chat but
    // turns the switch of the Chatting section off.
    if (process.env.AN_CHAPTERS !== undefined) {
      var many = Number(process.env.AN_CHAPTERS) || 0;
      var ids = start.messages.map(function (m) { return m.id; });
      start.chapters = [];
      for (var n = 0; many > 0 && n < many; n++) {
        start.chapters.push({
          id: 'ch' + n, name: 'Chapter ' + (n + 1) + ' of the long talk',
          after: ids.length ? ids[Math.floor(n * ids.length / many)] : 0
        });
      }
    }
    if (process.env.AN_NOCHAPTERS === '1') { start.chatting.chapters = false; }
    start.context = { used: '3,204', total: '8,192', ratio: 0.39 };
    start.code = name === 'rich';
    start.sandbox = name === 'rich';
  }
  if (name === 'streaming-code') {
    // A block of code that is still being written, so the colours of it are
    // painted again while it grows.
    start.generating = true;
    start.live = 'The reader of the fence is small:\n\n'
      + '```python\ndef read(line):\n    # a fence opens the block\n'
      + '    return line.startswith("```")  # and closes it';
    // The fence is left open on purpose: the block is still being written.
    start.reasoning = 'Show a fence coming in, so the repaint of a growing block is seen.';
    start.status = 'Answering';
    start.context = { used: '5,980', total: '8,192', ratio: 0.73 };
  }
  if (name === 'processing') {
    // The words of the answer have not begun, so the gauge above the chat box
    // is the only thing that says the program is at work.
    start.home = false;
    start.chat = { id: 'c1', title: 'Memory of the talk' };
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.messages = [
      said(1, 'You', 'What is the difference between mmap and mlock?'),
      said(2, 'Assistant', 'mmap lets the system page the model in and out. mlock '
        + 'keeps the whole file in the memory of the machine.\n\nUse mlock when the '
        + 'model must not be moved.'),
      said(3, 'You', 'And which one costs less?'),
    ];
    start.generating = true;
    start.status = 'thinking';
    start.progress = { percent: 42, active: true, what: 'text' };
    start.context = { used: '3,204', total: '8,192', ratio: 0.39 };
  }
  if (name === 'writing') {
    // The model is writing a command. The block stands in the chat while the
    // words of it come, and not only when the command runs.
    start.home = false;
    start.chat = { id: 'c1', title: 'Memory of the talk' };
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.messages = [
      said(1, 'You', 'What stands in the folder of the chat? Run a command and tell me.'),
    ];
    start.generating = true;
    start.status = 'the model writes a command';
    // The words of the answer are nothing by default: the model spends them on
    // the call. AN_SAYS=<n> puts that many words of an answer before the call,
    // so the room between them and the block of the command can be looked at.
    start.live = '';
    if (process.env.AN_SAYS !== undefined) {
      var told = Math.max(0, Number(process.env.AN_SAYS) || 0);
      start.live = WRITING_SAYS.slice(0, Math.min(WRITING_SAYS.length, told));
    }
    start.reasoning = 'Look in the folder first, then say what stands in it.';
    var have = process.env.AN_WRITE === undefined ? 12
      : Math.max(0, Number(process.env.AN_WRITE) || 0);
    start.live_tools = [{
      name: 'run_terminal',
      command: WRITING.slice(0, have),
      // The reason comes after the command, as the model writes it.
      purpose: have > WRITING.length ? WRITING_WHY.slice(0, have - WRITING.length) : ''
    }];
    start.context = { used: '1,180', total: '8,192', ratio: 0.14 };
  }
  if (name === 'streaming') {
    start.generating = true;
    start.live = 'The bar takes its height from the words beside it, and the list '
      + 'keeps its place after the paragraph.';
    start.reasoning = 'Look at the order of the blocks first.';
    // The thinking is over while the answer is still coming, so the head of
    // the panel says how long the model thought for.
    start.reasoning_ms = 12400;
    start.reasoning_done = true;
    start.status = 'Answering';
    start.context = { used: '5,980', total: '8,192', ratio: 0.73 };
  }
  if (name === 'markdown') {
    // A talk made of the blocks that the marks of the markdown make: a table,
    // whose columns stand as the colons under its head said they would, a quote
    // with a list and a block of code standing inside it, the marks across the
    // page that divide two parts of an answer, and links. A link gives its words
    // and keeps its address in itself; an address that is not one to follow is
    // read as words. The last message is a table of one column, which is still a
    // table.
    start.home = false;
    start.chat = { id: 'c1', title: 'The shape of the blocks' };
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.messages = [
      said(1, 'You', 'Put the sizes of the two blocks in a table, and quote the rule.'),
      said(2, 'Assistant',
        'The heights the two blocks took, measured in points:\n\n'
        + '| block | before | after |\n'
        + '| :--- | ---: | :-: |\n'
        + '| a heading | 14 | 16 |\n'
        + '| a list, one item | 6 | 8 |\n'
        + '| a list, two items | 12 | 8 |\n\n'
        + 'The rule the two blocks stand by:\n\n'
        + '> The bar takes its height from the words beside it, and never from '
        + 'the block that stands under it.\n'
        + '>\n'
        + '> - a heading, and\n'
        + '> - a list\n'
        + '>\n'
        + '> ```rust\n> fn height(words: &Words) -> f32 {\n>     lines(words) * 1.55\n> }\n> ```\n\n'
        + 'A line of words of its own, after all of that.',
        // The thoughts of an answer go through the same blocks, so a table and a
        // quote stand in the panel of the thinking as well.
        { reasoning: 'The table wants the two heights that were measured, not the '
          + 'ones that were planned.\n\n| block | planned | measured |\n| --- | --- | --- |\n'
          + '| a heading | 15 | 16 |\n\n> The bar takes its height from the words '
          + 'beside it.\n\nSo the bar is not touched.',
          reasoning_ms: 64000 }),
      said(3, 'You', 'And a table of one column?'),
      said(4, 'Assistant',
        '| the whole of it |\n| :- |\n| one cell, standing in the middle |\n\n'
        + 'Nothing else. A table of one column is a table still.'),
      said(5, 'You', 'And the marks across the page, with a link?'),
      said(6, 'Assistant',
        'The rule divides one part of an answer from the next. It is a hairline of '
        + 'the neutral, for the accent is kept for the marks of the program.\n\n'
        + '---\n\n'
        + 'A link gives its words and keeps its address in itself: [the llama.cpp '
        + 'page](https://github.com/ggml-org/llama.cpp), and an address written '
        + 'whole <https://autumn.example/notes>.\n\n'
        + '- an item with [a link of its own](https://autumn.example/one)\n'
        + '- an item with an address that is not one to follow: [run it]'
        + '(javascript:alert(1))\n\n'
        + '# A heading with [a link](https://autumn.example/head) in it',
        { reasoning: 'The rule wants a hairline of the neutral, and the words of a '
          + 'link the bright accent of the skin.\n\n---\n\nSee [the palette]'
          + '(https://autumn.example/palette), '
          + 'and read <https://autumn.example/two> after it.',
          reasoning_ms: 3100 }),
      said(7, 'You', 'And a table of seven columns, in a chat this wide?'),
      said(8, 'Assistant',
        'The words of a cell break to fit, so the chat keeps the width it had.\n\n'
        + '| file | what it holds | lines | before | after | took | left |\n'
        + '| :--- | :--- | ---: | ---: | ---: | ---: | ---: |\n'
        + '| markdown.rs | the cutting of the blocks into kinds | 390 | 14 | 16 | 3 min | none |\n'
        + '| page.html | the page of the remote mode | 3207 | 6 | 8 | 1 min | none |'),
    ];
    start.context = { used: '2,040', total: '8,192', ratio: 0.25 };
  }
  return start;
}

const CONFIG = {
  llama_dir: '/home/user/src/llama.cpp/build/bin',
  models_dir: '/home/user/models',
  repositories_dir: '',
  remote_port: 8723,
  upnp_enabled: false,
  dark_mode: false,
  chat_collapse_thinking: true,
  chat_collapse_tools: true,
  chat_highlight: true,
  chat_chapters: true,
  // The two rooms for steps stand unlimited, as they do for a new set of settings.
  chat_max_tool_steps: -1,
  chat_max_answer_steps: -1
};

const MODEL = {
  id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF',
  settings: {
    ctx_size: 8192, kv_index: 1, load_index: 1, spec_index: 0, draft_model: '',
    draft_tokens: 4, parallel: 1, batch_size: 512, ubatch_size: 128, vision: false,
    tensor_parallel: false,
    temperature: 0.7, top_p: 0.95, top_k: 20, min_p: 0, repeat_penalty: 1,
    presence_penalty: 0, max_tokens: -1, seed: -1,
    ctx_checkpoints: 32, cache_ram: 8192, cache_prompt: true,
    image_min_tokens: 0, image_max_tokens: 0, no_context_shift: false,
    mmproj_cpu: false,
    reasoning_budget: -1, reasoning_message: '', chat_template: '', extra_args: '',
    vram_budget: 0, gpu_enabled: [true, false]
  },
  gpus: [
    { device: '0', name: 'NVIDIA GeForce RTX 3060', memory_mb: 12288, enabled: true,
      is_offload: true, memory_text: '12,288 MiB' },
    { device: '1', name: 'llamaCPU-AVX2', memory_mb: 32768, enabled: false,
      is_offload: false, memory_text: '32,768 MiB' }
  ],
  kinds: ['f16', 'bf16', 'q8_0'],
  loads: ['none', 'mmap', 'mlock', 'mmap+mlock', 'directio'],
  specs: ['none', 'ngram', 'ngram-mod', 'lookahead']
};

async function main() {
  let name = process.argv[2] || 'home';
  const out = process.argv[3] || (name + '.png');
  const width = Number(process.argv[4] || 1180);
  const height = Number(process.argv[5] || 760);
  let dark = false;
  if (name.endsWith('-dark')) { dark = true; name = name.slice(0, -5); }
  else if (name.endsWith('-pale')) { name = name.slice(0, -5); }
  const state = scene(name === 'sheet-program' || name === 'sheet-model' ? 'chat' : name, dark);

  // AN_FOLD=0 turns the two fold switches of the Chatting section off, so a
  // picture can be taken of a panel that is written out as it comes in.
  // AN_HL=0 turns the colours of a code block off.
  if (process.env.AN_FOLD === '0') {
    state.chatting.collapse_thinking = false;
    state.chatting.collapse_tools = false;
    CONFIG.chat_collapse_thinking = false;
    CONFIG.chat_collapse_tools = false;
  }
  if (process.env.AN_HL === '0') {
    state.chatting.highlight = false;
    CONFIG.chat_highlight = false;
  }
  // AN_PROGRESS=<0..100> puts the gauge above the chat box at that number, and
  // AN_WHAT says what the program is working on: text, image or images.
  if (process.env.AN_PROGRESS !== undefined) {
    state.progress = {
      percent: Math.max(0, Math.min(100, Number(process.env.AN_PROGRESS) || 0)),
      active: true,
      what: state.progress.what
    };
  }
  if (process.env.AN_WHAT) { state.progress.what = process.env.AN_WHAT; }

  const puppeteer = require('puppeteer');
  let shell = '';
  try {
    shell = execSync('find "' + process.env.HOME + '/.cache/puppeteer" '
      + '-name chrome-headless-shell -type f | head -1').toString().trim();
  } catch (e) { shell = ''; }

  const browser = await puppeteer.launch({
    headless: shell ? 'shell' : true,
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu',
      '--font-render-hinting=none', '--force-color-profile=srgb']
  });
  const view = await browser.newPage();
  await view.setViewport({ width: width, height: height, deviceScaleFactor: 2 });
  view.on('pageerror', function (e) { console.log('PAGEERROR', String(e)); });
  view.on('console', function (m) { if (m.type() === 'error') { console.log('CONSOLE', m.text()); } });

  await view.evaluateOnNewDocument(function (fetchMode, held, config, model) {
    if (fetchMode) {
      // The page of the remote mode asks a small HTTP server. The records are
      // the same, so the same scenes can be drawn.
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(held))); } });
        }
        return Promise.resolve({ json: function () {
          return Promise.resolve({ ok: true }); } });
      };
      return;
    }
    window.__TAURI__ = {
      core: {
        invoke: function (command, args) {
          if (command === 'get_state') {
            return Promise.resolve(JSON.parse(JSON.stringify(held)));
          }
          if (command === 'get_config') {
            return Promise.resolve(JSON.parse(JSON.stringify(config)));
          }
          if (command === 'list_repositories') {
            return Promise.resolve(held.repositories.map(function (r) {
              return { id: r.id, name: r.name }; }));
          }
          if (command === 'get_model_settings') {
            return Promise.resolve(JSON.parse(JSON.stringify(model)));
          }
          if (command === 'preview_command') {
            return Promise.resolve('llama-server -m model.gguf --jinja -c 8192 '
              + '-ngl 999 --fit on --fit-target 400');
          }
          return Promise.resolve({ ok: true });
        }
      }
    };
  }, useFetch, state, CONFIG, MODEL);

  await view.goto('file://' + path.resolve(page), { waitUntil: 'networkidle0' });
  await new Promise(function (r) { setTimeout(r, 700); });

  if (name === 'previews') {
    await view.evaluate(function () {
      var b = document.getElementById('chatsbtn');
      if (b) { b.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 1400); });
  }
  if (name === 'sheet-program') {
    await view.evaluate(function () {
      var b = document.getElementById('settingsbtn');
      if (b) { b.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 600); });
  }
  if (name === 'sheet-model') {
    await view.evaluate(function () {
      var b = document.getElementById('modelbtn');
      if (b) { b.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 600); });
  }
  if (process.env.AN_SCROLL === 'top') {
    await view.evaluate(function () {
      var box = document.getElementById('thread');
      if (box) { box.scrollTop = 0; }
    });
    await new Promise(function (r) { setTimeout(r, 250); });
  }
  if (process.env.AN_SCROLL === 'sheet') {
    // Look at the foot of the sheet of settings, where the last sections stand.
    await view.evaluate(function () {
      var box = document.querySelector('.sheet-body');
      if (box) { box.scrollTop = box.scrollHeight; }
    });
    await new Promise(function (r) { setTimeout(r, 250); });
  }
  // AN_CLICK=selector clicks one thing, so a menu that opens on a click can
  // be seen open. The chat glides when a chapter is picked, so the wait is
  // long enough for the glide to come to rest.
  if (process.env.AN_CLICK) {
    const hit = await view.$(process.env.AN_CLICK);
    if (hit) {
      await hit.click();
      await new Promise(function (r) { setTimeout(r, 700); });
    }
  }
  // AN_HOVER=selector puts the mouse over one thing, so a look that only
  // shows when the reader hovers over it can be seen. AN_PICK=selector draws a
  // picture of each thing that matches, of its own, instead of the page.
  if (process.env.AN_HOVER) {
    const over = await view.$(process.env.AN_HOVER);
    if (over) {
      await over.hover();
      await new Promise(function (r) { setTimeout(r, 400); });
    }
  }
  // AN_MEASURE=selector prints where the things that match stand, in the
  // measure of the page, so a layout can be checked without looking at pixels.
  // It is read after a click or a hover was asked for, so the picture a click
  // leaves behind can be measured too.
  if (process.env.AN_MEASURE) {
    const boxes = await view.evaluate(function (selector) {
      var out = [];
      var found = document.querySelectorAll(selector);
      for (var n = 0; n < found.length; n++) {
        var b = found[n].getBoundingClientRect();
        out.push({ n: n, cls: found[n].className, left: Math.round(b.left),
          top: Math.round(b.top), width: Math.round(b.width),
          height: Math.round(b.height) });
      }
      return out;
    }, process.env.AN_MEASURE);
    console.log('MEASURE ' + JSON.stringify(boxes));
  }
  const picked = process.env.AN_PICK ? await view.$$(process.env.AN_PICK) : [];
  if (picked.length) {
    for (let n = 0; n < picked.length; n++) {
      const piece = out.replace(/\.png$/, '') + '-' + (n + 1) + '.png';
      try {
        await picked[n].screenshot({ path: piece });
        console.log('wrote ' + piece);
      } catch (e) { console.log('skipped ' + piece); }
    }
  } else {
    await view.screenshot({ path: out });
  }
  await browser.close();
  console.log('wrote ' + out);
}

main().catch(function (e) { console.log('FAIL', String(e)); process.exit(1); });
