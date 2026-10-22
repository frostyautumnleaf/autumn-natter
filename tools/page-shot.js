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
//   AN_SUMMARIES=6 node page-shot.js rich p.png   # that many handoff summaries
//   AN_NOTIMELINE=1 node page-shot.js rich p.png  # the line turned off
//   AN_BROKEN=1 node page-shot.js rich p.png      # the newest one never written
//   AN_CUT=1 node page-shot.js rich p.png         # a tool call the reader stopped
//   AN_SUMMARISING=1 node page-shot.js rich p.png # a summary being written,
//     which is written out of sight: no live answer stands in the chat
//   AN_FIELD=fCompactPrompt node page-shot.js sheet-program p.png
//     brings another field of the sheet into the picture
//   AN_SUMMARIES, AN_NOTIMELINE, AN_BROKEN and AN_SUMMARISING stand for the
//   Compaction section.
//   AN_PICK, AN_HOVER and AN_CLICK pick out parts of the page by their class.
//
// The scenes: home, chat, rich, markdown, streaming, streaming-code, writing,
// processing, previews, sheet-program, sheet-model. A name that ends in -dark draws the dark skin, and
// a name that ends in -pale draws the pale one. AN_SCROLL=top puts the talk at
// the top instead of the end.
//   AN_PROGRESS=42 AN_WHAT=image node page-shot.js processing p.png
//   AN_PROGRESS and AN_WHAT stand for the gauge above the chat box.
//   AN_PASTE=1 hangs a picture and a file of words on the chat box, as the
//   reader's Ctrl+V would, so the blocks over the words can be looked at.
//   AN_DROP=1 hangs them as a file let go over the box would, which is the
//   other road on to the box, and the one Tauri used to eat.
//   AN_PICFILE=pic.png makes the pictures of the attachments real, so the
//   blocks of a sent message can be judged as they will be seen. A second
//   picture of another shape goes to AN_PICFILE2, and is handed out to an
//   attachment whose number holds the word 'tall'. The scene 'parts' is a talk
//   made of nothing but the blocks under a message.

'use strict';

const path = require('path');
const { execSync } = require('child_process');

const page = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const useFetch = process.env.AN_FETCH === '1';

// The pictures of the attachment blocks. A single pixel is too small to tell
// whether the plate of a picture is laid out well, so AN_PICFILE names a real
// picture to hand out instead, and AN_PICFILE2 a second one of another shape;
// the bridge gives either to any block that asks for the file of an attachment.
function aPicture(where) {
  const ONE_PIXEL = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNk' +
    '+A8AAQUBAScY42YAAAAASUVORK5CYII=';
  return 'data:image/png;base64,'
    + (where ? require('fs').readFileSync(where).toString('base64') : ONE_PIXEL);
}

const ATT_PIC = aPicture(process.env.AN_PICFILE);
const ATT_TALL = aPicture(process.env.AN_PICFILE2 || process.env.AN_PICFILE);

// The handoff summaries of a talk. `count` of them are spread over the messages,
// each with the two numbers of the room, the day it stood, and a little of what
// it says: the head in the talk, the card over it, and the node on the line at
// the right are all made of these.
function addSummaries(state, count) {
  var messages = state.messages.slice();
  var handoffs = [];
  for (var n = 0; n < count; n++) {
    var at = messages.length ? Math.floor((n + 1) * messages.length / (count + 1)) : 0;
    var id = 200 + n;
    var before = 6964 + n * 438;
    var after = 412 - n * 7;
    var when = '2026-10-' + (12 + n) + ', 0' + (n % 6) + ':' + (10 + n);
    var said2 = 'Done: part ' + (n + 1) + ' of the talk was finished, and the tests '
      + 'of it stood up. Next: the part that comes after it, and the height of the '
      + 'bar beside a heading. Files: src/markdown.rs holds the parser, and '
      + 'frontend/index.html holds the page of the window.';
    messages.splice(at, 0, {
      id: id, role: 'Summary', body: said2, reasoning: '', reasoning_ms: 0,
      tool: '', mono: false, failed: false, when: '09:4' + (n % 10),
      when_full: when, context_before: before, context_after: after,
      attachments: []
    });
    handoffs.push({
      id: id, number: n + 1, state: 'done', when: when, before: before, after: after,
      snippet: said2.slice(0, 160) + '…', why: ''
    });
  }
  state.messages = messages;
  state.handoffs = handoffs;
}

// One message of the talk.
function said(id, role, body, extra) {
  return Object.assign({
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + (id % 10), attachments: []
  }, extra || {});
}

// One chat with every kind of block in it: a heading, a paragraph, a list, a
// block of code, the thinking of the model, two runs of the terminal tool, and
// a line from the program. The first run of the tool came out of a turn with
// no words of its own, so the thinking of that turn stands above the command.
const FULL = [
  // The first message of the reader hangs two files on its words, so the
  // blocks under the bubble are drawn in both skins.
  said(1, 'You', 'Can you show the plan for the parser, and run the tests?', {
    attachments: [
      { id: 'att-1', name: 'parser-diagram.png', mime: 'image/png', image: true },
      { id: 'att-2', name: 'notes.txt', mime: 'text/plain', image: false }
    ]
  }),
  said(2, 'Assistant',
    '# Plan\n\nWe change two files.\n\n'
    + '1. `markdown.rs` closes a paragraph when a list starts.\n'
    + '2. `message_view.rs` gives the bar a height.\n\n'
    + 'The tests cover the order now.\n\n'
    + '```rust\nfn flush(&mut self) {\n    self.close_para();\n}\n```',
    { reasoning: 'The user wants a **plan**. The fault is in `markdown.rs`, and not '
      + 'in the view. Two things to check first:\n\n'
      + '- the order the blocks come out in\n- the height of the bar beside a heading\n\n'
      + 'The parser keeps a paragraph open until a line cannot stand in it. A list '
      + 'is such a line, so the paragraph must be closed when the list begins, or '
      + 'the list comes out before the words that stood before it.\n\n'
      + 'The bar beside a heading takes its height from the words beside it, and '
      + 'not from a fixed number, so a heading of two lines keeps the whole bar.\n\n'
      + '```rust\nif list {\n    flush_para();\n}\n```\n\n'
      + 'The tests of the parser cover the order now, and the order is the whole '
      + 'of the fault: nothing else moved.',
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
  // A file the model changed, and a file it tried to make and did not. The head
  // of each block says which tool it was, and the mark beside the head is that
  // tool's own.
  said(7, 'Tool', 'notes/plan.md', {
    tool_name: 'edit_file',
    tool: "Edited notes/plan.md (2 replacements)\n"
      + '@@ -2,5 +2,5 @@\n'
      + ' the first line of the plan\n'
      + '-flush the paragraph\n'
      + '+close the paragraph, and open the list\n'
      + ' the last line of the plan'
  }),
  said(8, 'Tool', 'notes/todo.txt', {
    tool_name: 'create_file',
    tool: "Error: 'notes/todo.txt' already stands there. create_file makes a new file "
      + 'and never writes over one; to change it, use edit_file.',
    failed: true
  }),
  // A look about the web. The head says what was asked for, and the answer is
  // short words of each place, so the block has no whole page in it.
  said(9, 'Tool', 'largest lake in Africa by volume', {
    tool_name: 'web_search',
    tool: 'Title: Lake Victoria - Wikipedia\nURL: https://en.wikipedia.org/wiki/Lake_Victoria\n'
      + 'Words: Lake Victoria is the largest lake in Africa by area, and the second-largest '
      + 'freshwater lake by area after Lake Superior.\n\n---\n\n'
      + 'Title: Lake Tanganyika - Wikipedia\nURL: https://en.wikipedia.org/wiki/Lake_Tanganyika\n'
      + 'Words: Lake Tanganyika is the largest tropical freshwater lake by volume, and the '
      + 'second oldest and second deepest lake in the world.\n\n---\n\n'
      + 'These are short words of each place. To read a whole page, ask again with the '
      + 'address of it, as {"url": "https://..."}.'
  }),
  // A picture handed to the model. The block carries the receipt of the run and
  // never the picture itself: the bytes go to the model alone.
  said(10, 'Tool', 'notes/cat.png', {
    tool_name: 'view_image',
    tool: 'Opened a picture: notes/cat.png (PNG, 1024 by 768 pixels, 188 KiB). '
      + 'The picture itself goes to the model with these words, for this model '
      + 'can look at pictures.'
  })
];

const TALK = [
  said(1, 'You', 'What is the difference between mmap and mlock?', {
    attachments: [{ id: 'att-3', name: 'memory-map.png', mime: 'image/png', image: true }]
  }),
  said(2, 'Assistant', 'mmap lets the system page the model in and out. mlock keeps '
    + 'the whole file in the memory of the machine.\n\nUse mlock when the model must '
    + 'not be moved.'),
  said(3, 'You', 'Which one is the default?'),
  said(4, 'Assistant', 'mmap is the default. It costs less memory.'),
];

// A talk whose only point is the blocks under the words of the reader: a
// picture of the usual shape, a picture taller than it is wide, a name too
// long for a block, a message of files and no words, and a file whose name is
// made of many parts. The second picture is asked for by an attachment whose
// number holds the word 'tall', so the two shapes can stand in one row.
const PARTS = [
  said(1, 'You', 'Two pictures of the plateau, the log of the run, and a note.', {
    attachments: [
      { id: 'att-wide', name: 'plateau-east.png', mime: 'image/png', image: true },
      { id: 'att-tall', name: 'plateau-west.png', mime: 'image/png', image: true },
      { id: 'att-long', name: 'run-log-2026-10-17-afternoon-of-the-third-day.txt',
        mime: 'text/plain', image: false },
      { id: 'att-note', name: 'notes.txt', mime: 'text/plain', image: false }
    ]
  }),
  said(2, 'Assistant', 'The eastern picture holds the lake; the western one is cut off '
    + 'by the ridge.'),
  said(3, 'You', '', {
    attachments: [
      { id: 'att-only', name: 'the-plan-of-the-bar.svg', mime: 'image/svg+xml', image: false },
      { id: 'att-tall2', name: 'screenshot-of-the-sheet.png', mime: 'image/png', image: true }
    ]
  }),
];

// The name of a file a model writes in the `writing` scene, for the tool that
// works on a file. AN_TOOL names the tool; without it the scene writes a command.
const WRITING_NAMES = {
  edit_file: 'notes/plan.md',
  create_file: 'notes/todo.txt',
  web_search: 'largest lake in Africa by volume',
  view_image: 'notes/cat.png'
};

// The words a file tool is making, as the reader is given them while the call
// is written. The head of the block names the file, and the body holds these
// words, so the reader sees the change as it is made.
const WRITING_WORDS = {
  edit_file: 'the old words of the plan\n> the new words of the plan',
  create_file: 'a short plan of the day, written down so it is not lost'
};

// The words under the chat box while one of these tools is being written. They
// are the words the engine puts there, so the picture does not invent its own.
const WRITING_DOING = {
  edit_file: 'editing a file: ',
  create_file: 'making a file: ',
  web_search: 'looking a thing up: ',
  view_image: 'looking at a picture: '
};

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
    // True while the program is writing a handoff summary. The words of one are
    // never shown, so the page draws no live answer while it stands.
    summarising: false,
    // The calls of a tool the model is writing right now. A block of a command
    // being written is made of these.
    live_tools: [],
    // How far the server is with the words it was sent. The gauge above the
    // chat box stands on these three.
    progress: { percent: 0, active: false, what: 'text' },
    chapters: [],
    // The summarisations of the chat, and the Compaction section of the settings.
    handoffs: [],
    compaction: {
      type: 'handoff', by: 'percent', percent: 85, reserve: 10240, timeline: true,
      prompt: 'Write a very detailed but concise handoff summary for AI consumption '
        + '(no styling) that can be given to an AI to continue working from the '
        + 'current state of the session.',
      prompt_short: 'Write a very short handoff summary for AI consumption (no '
        + 'styling). The room of the model is nearly full, so the whole summary '
        + 'must come to an end within a few hundred words.'
    },
    thinking: {
      mode: 'off', available: true,
      modes: ['off', 'low', 'medium', 'high', 'xhigh'],
      // The model of these scenes can keep the thinking of the earlier turns,
      // and the choice stands on, as it does for a new chat.
      preserve: true, preserve_available: true
    },
    chatting: { collapse_thinking: true, grow_thinking: true,
                tools_in_thinking: true, collapse_tools: true,
                highlight: true, chapters: true },
    context: { used: '0', total: '8,192', ratio: 0.0 },
    generating: false, busy: false, code: false, search: false, sandbox: false,
    sandbox_available: true,
    sandbox_note: 'bubblewrap (bwrap) was found, so the tool can be kept inside a jail.',
    status: 'Ready', dark: !!dark, data_dir: '/home/user/.local/share/autumn-natter'
  };
  if (name === 'chat' || name === 'rich' || name === 'parts' || name === 'streaming'
    || name === 'streaming-code' || name === 'previews') {
    start.home = false;
    start.chat = { id: 'c1', title: name === 'rich' ? 'Parser order' : 'Memory of the talk' };
    start.chats = chats(name === 'previews' ? 9 : 3, 1);
    start.carousel = { index: 1, count: start.chats.length, blank: false };
    start.messages = name === 'rich' ? FULL : name === 'parts' ? PARTS : TALK;
    // AN_CUT=1 turns the run of the file tool into a call the reader stopped with
    // their own hand. The block stays in the chat, its pill says 'cancelled', and
    // under the second label stands the word of the stop and not a result.
    if (process.env.AN_CUT === '1') {
      start.messages = start.messages.map(function (m) {
        if (m.tool_name !== 'edit_file') { return m; }
        return Object.assign({}, m, {
          cancelled: true,
          failed: true,
          tool: 'the call was cancelled: you stopped the answer while the call was '
            + 'being written, so the writing was stopped and the tool never ran',
          tool_args: '{"path":"notes/plan.md","edits":[{"old_string":"the fi'
        });
      });
    }
    // The chapters the model marked, tied to the message each begins after.
    start.chapters = name === 'rich'
      ? [{ id: 'ch1', name: 'The plan for the parser', after: 1 },
         { id: 'ch2', name: 'Running the tests', after: 3 },
         { id: 'ch3', name: 'The bar beside a list', after: 4 }]
      : [];
    // The handoff summaries the model wrote when the talk came near the end of
    // its room. They stand among the words as the head of the summarisation, and
    // on the line at the right of the chat.
    if (name === 'rich') {
      addSummaries(start, 2);
    }
    // AN_SUMMARIES=N puts that many summarisations in the talk, so the line can
    // be looked at when a chat has been summarised many times. AN_NOTIMELINE=1
    // turns the switch of the Compaction section off, and AN_BROKEN=1 leaves the
    // newest summarisation with no words, which is what a server that went away
    // in the middle of a summary leaves behind.
    if (process.env.AN_SUMMARIES !== undefined) {
      addSummaries(start, Number(process.env.AN_SUMMARIES) || 0);
    }
    if (process.env.AN_NOTIMELINE === '1') { start.compaction.timeline = false; }
    if (process.env.AN_BROKEN === '1' && start.handoffs.length) {
      var lastCut = start.handoffs[start.handoffs.length - 1];
      lastCut.state = 'owed';
      lastCut.snippet = '';
      lastCut.why = 'the server went away while the summary was written. Start it again';
      for (var k = 0; k < start.messages.length; k++) {
        if (start.messages[k].id === lastCut.id) {
          start.messages[k].body = '';
          start.messages[k].tool = lastCut.why;
        }
      }
    }
    // AN_SUMMARISING=1 puts the chat in the state the engine stands in while a
    // handoff summary is being written: an answer is on the way, the mark of the
    // summarisation stands at the end of the talk with no words in it, and the
    // words of the summary are not shown. The window has no live answer to draw.
    if (process.env.AN_SUMMARISING === '1') {
      start.generating = true;
      start.summarising = true;
      start.live = '';
      start.reasoning = '';
      start.live_tools = [];
      start.status = 'The handoff summary is being written';
      var writing = (start.handoffs || []).length + 1;
      start.messages = start.messages.concat([{
        id: 399, role: 'Summary', body: '', reasoning: '', reasoning_ms: 0,
        tool: 'the handoff summary is being written ...', mono: false, failed: false,
        when: '11:02', when_full: '2026-10-21, 11:02',
        context_before: 6964, context_after: 0, attachments: []
      }]);
      start.handoffs = (start.handoffs || []).concat([{
        id: 399, number: writing, state: 'writing', when: '2026-10-21, 11:02',
        before: 6964, after: 0, snippet: '', why: ''
      }]);
    }
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
    // The width of the chat, as the Chatting section of the settings gives it.
    if (process.env.AN_CHATWIDTH) { start.chatting.width = parseInt(process.env.AN_CHATWIDTH, 10) || 780; }
    if (process.env.AN_NOTHINKGROW === '1') {
      start.chatting.grow_thinking = false;
      CONFIG.chat_grow_thinking = false;
    }
    if (process.env.AN_NOTOOLSTHINK === '1') {
      start.chatting.tools_in_thinking = false;
      CONFIG.chat_tools_in_thinking = false;
    }
    start.context = { used: '3,204', total: '8,192', ratio: 0.39 };
    start.code = name === 'rich';
    start.search = name === 'rich';
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
    var tool = process.env.AN_TOOL || 'run_terminal';
    var named = WRITING_NAMES[tool] !== undefined;
    // A file tool names its file in the head of the block, and makes its words
    // in the body, so the two stand apart here as they do in the engine.
    var fileTool = tool === 'edit_file' || tool === 'create_file';
    // The badge of the web search stands on for a search, and the badge of the
    // tools stands on for the tools that can reach the machine.
    start.search = tool === 'web_search';
    // The badge of the tools stands on for every tool that can reach the
    // machine, which is all of them but the look about the web.
    start.code = tool !== 'web_search';
    var name = named ? WRITING_NAMES[tool] : '';
    // The words the call is making: the words of a file tool, the name of the
    // thing another tool looks at, or the command of a run.
    var full = fileTool ? WRITING_WORDS[tool] : (named ? name : WRITING);
    // A name of a file is short, so the whole of it stands unless the number of
    // characters was asked for.
    var have = process.env.AN_WRITE === undefined ? (named ? full.length : 12)
      : Math.max(0, Number(process.env.AN_WRITE) || 0);
    start.live_tools = [{
      name: tool,
      command: full.slice(0, have),
      target: fileTool ? name : '',
      // The reason comes after the command, as the model writes it. A tool that
      // works on a file never asked for a reason.
      purpose: !named && have > WRITING.length
        ? WRITING_WHY.slice(0, have - WRITING.length) : ''
    }];
    if (named) {
      // A reader of the picture should not wonder why the chat box asks for a
      // command over the block of a file tool, nor why a look about the web
      // stands under a word about files.
      var asked = {
        web_search: 'Look up how large the largest lake in Africa is, and tell me what you found.',
        view_image: 'Look at the picture of the cat in the chat, and tell me what stands in it.'
      };
      start.messages = [said(1, 'You', asked[tool] ||
        'Put the plan in a file of the chat, and tell me what you changed.')];
      start.status = (WRITING_DOING[tool] || WRITING_DOING.create_file) + name;
    }
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
  chat_grow_thinking: true,
  chat_tools_in_thinking: true,
  chat_collapse_tools: true,
  chat_date_new_chat: true,
  chat_date_every_message: false,
  chat_highlight: true,
  chat_chapters: true,
  // The two rooms for steps stand unlimited, as they do for a new set of settings.
  chat_max_tool_steps: -1,
  chat_max_answer_steps: -1,
  // A paste of many words becomes a file at six thousand characters.
  pasted_text_char_limit: 6000,
  // Compaction: the handoff summary, at a share of the room, with the line of
  // the summarisations standing at the right of the chat.
  chat_compaction: 'handoff',
  chat_compact_by: 'percent',
  chat_compact_percent: 85,
  chat_compact_reserve: 10240,
  chat_compact_prompt: 'Write a very detailed but concise handoff summary for AI '
    + 'consumption (no styling) that can be given to an AI to continue working from '
    + 'the current state of the session. Include full context of the session. You '
    + 'must include what has been done so far and what needs to be done. The AI will '
    + 'only receive the first initial prompt that the user sent, along with this '
    + 'handoff summary. If applicable, include a list of file paths that are '
    + 'relevant to the session / tasks / project with a short explanation of what '
    + 'each file is used for.',
  chat_compact_prompt_short: 'Write a very short handoff summary for AI consumption '
    + '(no styling). The room of the model is nearly full, so the whole summary must '
    + 'come to an end within a few hundred words. Say what was done, what stands in '
    + 'hand right now, and what must be done next. Name the files that matter with '
    + 'one short line each.',
  chat_timeline: true
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
  // AN_PRESERVE=0 takes the choice to keep the thinking of the earlier turns
  // away from the model, so the switch of the menu can be looked at off, and
  // AN_NOPRESERVE=1 leaves the model without the choice at all.
  if (process.env.AN_PRESERVE === '0') { state.thinking.preserve = false; }
  if (process.env.AN_NOPRESERVE === '1') {
    state.thinking.preserve_available = false;
    delete state.thinking.preserve;
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

  await view.evaluateOnNewDocument(function (fetchMode, held, config, model, pic, pic2) {
    if (fetchMode) {
      // The page of the remote mode asks a small HTTP server. The records are
      // the same, so the same scenes can be drawn.
      window.fetch = function (url) {
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(held))); } });
        }
        if (String(url).indexOf('/api/attachment') === 0) {
          // One attachment record, as the small server of the remote mode
          // hands it to the block that asks for its file.
          var asked = String(url).indexOf('tall') < 0 ? pic : pic2;
          return Promise.resolve({ json: function () {
            return Promise.resolve({ name: 'memory-map.png', mime: 'image/png',
              data: asked, text: '' }); } });
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
          if (command === 'get_attachment') {
            // One attachment record, as the window hands it to the block
            // that asks for its file.
            return Promise.resolve({ name: 'memory-map.png', mime: 'image/png',
              data: String((args && args.id) || '').indexOf('tall') < 0 ? pic : pic2,
              text: '' });
          }
          return Promise.resolve({ ok: true });
        }
      }
    };
  }, useFetch, state, CONFIG, MODEL, ATT_PIC, ATT_TALL);

  await view.goto('file://' + path.resolve(page), { waitUntil: 'networkidle0' });
  await new Promise(function (r) { setTimeout(r, 700); });
  // AN_PASTE=1 hangs a picture and a file of words on the chat box the way
  // Ctrl+V would, so the blocks over the words of the box can be looked at.
  if (process.env.AN_PASTE === '1') {
    await view.evaluate(function (picUrl) {
      var bin = atob(picUrl.split(',')[1]);
      var bytes = new Uint8Array(bin.length);
      for (var n = 0; n < bin.length; n++) { bytes[n] = bin.charCodeAt(n); }
      var transfer = new DataTransfer();
      transfer.items.add(new File([bytes], 'dropped-photo.png', { type: 'image/png' }));
      transfer.items.add(new File(['the pasted words of a note'], 'note.txt',
        { type: 'text/plain' }));
      var draft = document.getElementById('draft');
      draft.dispatchEvent(new ClipboardEvent('paste', {
        clipboardData: transfer, bubbles: true, cancelable: true
      }));
    }, ATT_PIC);
    await new Promise(function (r) { setTimeout(r, 400); });
  }

  // AN_DROP=1 lets a picture and a file of words go over the chat box, the way
  // a drag from the file manager of the machine would. The window took these
  // from the page before, so this is the look of the road that was shut.
  if (process.env.AN_DROP === '1') {
    await view.evaluate(function (picUrl) {
      var bin = atob(picUrl.split(',')[1]);
      var bytes = new Uint8Array(bin.length);
      for (var n = 0; n < bin.length; n++) { bytes[n] = bin.charCodeAt(n); }
      var transfer = new DataTransfer();
      transfer.items.add(new File([bytes], 'let-go-photo.png', { type: 'image/png' }));
      transfer.items.add(new File(['the words of a file let go'], 'let-go.md',
        { type: 'text/markdown' }));
      var box = document.getElementById('box');
      box.dispatchEvent(new DragEvent('dragover', {
        dataTransfer: transfer, bubbles: true, cancelable: true
      }));
      box.dispatchEvent(new DragEvent('drop', {
        dataTransfer: transfer, bubbles: true, cancelable: true
      }));
    }, ATT_PIC);
    await new Promise(function (r) { setTimeout(r, 400); });
  }

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
    // The sheet is taller than the window, so bring a field of it into the
    // picture: the last field of the Chatting section, or the field named by
    // AN_FIELD, which is how the Compaction section is looked at.
    var field = process.env.AN_FIELD || 'fPasteLimit';
    await view.evaluate(function (which) {
      var f = document.getElementById(which);
      if (f) { f.scrollIntoView({ block: 'center' }); }
    }, field);
    await new Promise(function (r) { setTimeout(r, 300); });
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
  // AN_HOVER=selector puts the mouse over one thing, so a look that only
  // shows when the reader hovers over it can be seen. It comes before AN_CLICK,
  // so a click may land on a thing that stands in a card under the pointer.
  // AN_PICK=selector draws a picture of each thing that matches, of its own,
  // instead of the page.
  if (process.env.AN_HOVER) {
    const over = await view.$(process.env.AN_HOVER);
    if (over) {
      await over.hover();
      await new Promise(function (r) { setTimeout(r, 400); });
    }
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
