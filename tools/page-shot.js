// Images of the two pages of Autumn Natter, for the computer that has no
// desktop.
//
// The window cannot be opened in a build box: there is no display. This tool
// draws the pages in a headless browser instead, with a fake bridge, and writes
// the images as PNG. The pages are read as they are now, so an error in the
// layout is seen here and not on the screen of the user.
//
// The core is not touched: no server is started, no model is read. The records
// below mean the state of the engine, and the bridge answers as Tauri or
// as the small HTTP server of the remote mode.
//
//   npm install puppeteer           # once, in this folder, needs the network
//   node page-shot.js rich-pale rich.png 1180 1450
//   node page-shot.js home-dark home.png 1180 760
//   AN_PAGE=../web/page.html AN_FETCH=1 node page-shot.js chat-pale page.png
//   AN_PICK='.think .thead' AN_HOVER='.turn' node page-shot.js rich p.png
//   AN_CLICK='#think' node page-shot.js chat p.png
//   AN_CHAPTERS=12 node page-shot.js rich p.png   # that many markers on the line
//   AN_USERMSGS=40 node page-shot.js rich p.png  # that many messages of the user
//     in the conversation, so the line of the dashes can be checked when it is full
//   AN_NOCHAPTERS=1 node page-shot.js rich p.png  # the Chatting switch turned off
//   AN_NOCOLOUR=1 node page-shot.js markdown p.png
//     the switch of the colours the model requests, turned off: the text between
//     the four markers then are in the colour of the text about them
//   AN_SUMMARIES=6 node page-shot.js rich p.png   # that many handoff summaries
//   AN_NOTIMELINE=1 node page-shot.js rich p.png  # the line turned off
//   AN_BROKEN=1 node page-shot.js rich p.png      # the newest one never written
//   AN_CUT=1 node page-shot.js rich p.png         # a tool call the user stopped
//   AN_LONGTOOL=4000 node page-shot.js rich p.png # a run whose result is that many lines long,
//     which is what reading a large file with one command brings back
//   AN_RUNMARKS=1 node page-shot.js rich p.png    # a command at work with time to
//     run out: the clock, the marker that adds five minutes, and the stop marker
//   AN_QUEUE=1 node page-shot.js rich p.png       # the list of the messages
//     waiting to go, as the program keeps it. AN_QUEUEHELD=1 markers the list as
//     stopped. Use AN_CLICK='#queuehead' to see the rows open. AN_GOAL=1 puts
//     an active goal above that list, as it is while one is being pursued.
//   AN_SIDE=1 node page-shot.js rich p.png        # the side bar, open
//   AN_PIXEL=1 node page-shot.js rich p.png       # the pixel-art form of the theme
//   AN_GIFTS=1 node page-shot.js rich p.png       # the conversation ends with the blocks
//     of the files the download tool proposed, with their buttons
//   AN_SUGGEST=1 node page-shot.js rich p.png     # the conversation ends with the button
//     that opens the window over the files shown, and with the buttons of the
//     paths the suggestion tool proposed for what comes next. Remain it with
//     AN_GIFTS=1 to see the three types of the end of an answer together
//   AN_SUMMARISING=1 node page-shot.js rich p.png # a summary being written,
//     which is written of sight: no live answer is in the chat
//   AN_FIELD=fCompactPrompt node page-shot.js sheet-program p.png
//     brings another field of the settings panel into the image
//   AN_LOGSEARCH=error node page-shot.js sheet-logs p.png
//     writes text in the field over the log, which is how a search is checked
//   AN_RATE=manual node page-shot.js sheet-logs p.png
//     puts the pace of the log on manual, which is how the button of Refresh is
//     checked remaining live; the other two paces leave it greyed
//   AN_THEME=corporate node page-shot.js home-pale p.png
//     draws the scene in one of the six themes: light, dark, silver, corporate,
//     neon or glass. A name that ends in -dark, -silver, -corporate, -neon or -glass does the same,
//     and -pale asks for the pale one
//   AN_LOGKIND=program node page-shot.js sheet-logs p.png
//     reads the log of the program instead of the output of llama.cpp, which is
//     how the switch at the header of the Logs tab is checked
//   AN_SUMMARIES, AN_NOTIMELINE, AN_BROKEN and AN_SUMMARISING mean the
//   Compaction section.
//   AN_PICK, AN_HOVER and AN_CLICK pick out parts of the page by their class.
//   AN_CURSOR=0.4,0.45 node page-shot.js rich p.png   # the cursor resting in the chat, so the
//     field of digits it uncovers is seen at full
//   AN_NOCURSORDIGITS=1 node page-shot.js rich p.png  # the Chatting switch of those digits off
//
// The scenes: home, chat, rich, markdown, streaming, streaming-code, writing,
// processing, previews, sheet-program, sheet-model, sheet-logs, sheet-mcp,
// mcp-menu, corp, corplive and corptools. The last three mean the corporate display: one
// block for an answer, holding thoughts, progress updates, and tool outcomes,
// with the final answer outside the card.
// AN_CORP=1 puts that display over any other scene. AN_NOKIND=1 leaves the type of the editor of the MCP tab as it
// remains, so the empty field of the code can be checked. A name that ends in -dark draws the dark theme, and
// a name that ends in -pale draws the pale one. AN_SCROLL=top puts the conversation at
// the top instead of the end.
//   AN_PROGRESS=42 AN_WHAT=image node page-shot.js processing p.png
//   AN_PROGRESS and AN_WHAT mean the meter above the chat composer.
//   AN_PASTE=1 hangs an image and a text file on the chat composer, as the
//   user's Ctrl+V would, so the blocks over the text can be checked.
//   AN_DROP=1 hangs them as a file let go over the box would, which is the
//   other path on to the box, and the one Tauri used to eat.
//   AN_PICFILE=pic.png makes the images of the attachments real, so the
//   blocks of a sent message can be judged as they will be seen. A second
//   image of another form goes to AN_PICFILE2, and is handed out to an
//   attachment whose number holds the word 'tall'. The scene 'parts' is a conversation
//   made of nothing but the blocks under a message.

'use strict';

const path = require('path');
const { execSync } = require('child_process');

const page = process.env.AN_PAGE || path.join(__dirname, '..', 'frontend', 'index.html');
const useFetch = process.env.AN_FETCH === '1';

// The images of the attachment blocks. A single pixel is too small to tell whether the plate of
// an image is decompressed well, so AN_PICFILE names a real image to hand out instead, and
// AN_PICFILE2 a second one of another form; the bridge gives either to any block that requests
// the file of an attachment.
function aPicture(where) {
  const ONE_PIXEL = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mNk' +
    '+A8AAQUBAScY42YAAAAASUVORK5CYII=';
  return 'data:image/png;base64,'
    + (where ? require('fs').readFileSync(where).toString('base64') : ONE_PIXEL);
}

const ATT_PIC = aPicture(process.env.AN_PICFILE);
const ATT_TALL = aPicture(process.env.AN_PICFILE2 || process.env.AN_PICFILE);

// The text a server of the program itself begins with, extracted from the program. The MCP tab
// of an image shows the example the program really sends, so the fixture is not a guess of it.
function theTemplate() {
  const words = require('fs').readFileSync(path.join(__dirname, '..', 'src', 'mcp.rs'), 'utf8');
  const head = 'pub const TEMPLATE: &str = r##"\n';
  const from = words.indexOf(head);
  if (from < 0) { return ''; }
  const rest = words.slice(from + head.length);
  const end = rest.indexOf('\n"##;');
  return end < 0 ? '' : rest.slice(0, end);
}

const MCP_TEMPLATE = theTemplate();

// The servers of the fake user, for the MCP tab and the menu of the badge.
const MCP_HELD = { ok: true, python: 'python3', template: MCP_TEMPLATE, servers: [
  { id: 'mcpdemo0001', name: 'Bookshop', kind: 'stdio', icon: 7, shown: true, on: true,
    tools: ['look_up', 'reserve'], fault: '' },
  { id: 'mcpdemo0002', name: 'Clock', kind: 'internal', icon: 2, shown: true, on: false,
    tools: ['now'], fault: '' },
  { id: 'mcpdemo0003', name: 'Archive', kind: 'http', icon: 9, shown: false, on: false,
    tools: [], fault: 'The server did not answer' }
] };

// The handoff summaries of a conversation. `count` of them are spread over the messages, each
// with the two numbers of the space, the day it was, and a little of what it reports: the
// header in the conversation, the card over it, and the node on the line at the right are all
// made of these.
function addSummaries(state, count) {
  var messages = state.messages.slice();
  var handoffs = [];
  for (var n = 0; n < count; n++) {
    var at = messages.length ? Math.floor((n + 1) * messages.length / (count + 1)) : 0;
    var id = 200 + n;
    var before = 6964 + n * 438;
    var after = 412 - n * 7;
    var when = '2026-10-' + (12 + n) + ', 0' + (n % 6) + ':' + (10 + n);
    var said2 = 'Done: part ' + (n + 1) + ' of the conversation was finished, and the tests '
      + 'of it was up. Next: the part that comes after it, and the height of the '
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

// One message of the conversation.
function said(id, role, body, extra) {
  return Object.assign({
    id: id, role: role, body: body, reasoning: '', reasoning_ms: 0, tool: '',
    mono: false, failed: false, when: '09:4' + (id % 10), attachments: []
  }, extra || {});
}

// One chat with every type of block in it: a heading, a paragraph, a list, a block of code, the
// thinking of the model, two runs of the terminal tool, and a line from the program. The first
// run of the tool came out of a turn with no text of its own, so the thinking of that turn is
// above the command.
const FULL = [
  // The first message of the user hangs two files on its text, so the blocks under the bubble
  // are drawn in both themes.
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
    { reasoning: 'The user wants a **plan**. The error is in `markdown.rs`, and not '
      + 'in the view. Two items to check first:\n\n'
      + '- the order the blocks come out in\n- the height of the bar beside a heading\n\n'
      + 'The parser keeps a paragraph open until a line cannot be in it. A list '
      + 'is such a line, so the paragraph must be closed when the list begins, or '
      + 'the list comes out before the text that was before it.\n\n'
      + 'The top bar beside a heading takes its height from the text beside it, and '
      + 'not from a fixed number, so a heading of two lines keeps the whole bar.\n\n'
      + '```rust\nif list {\n    flush_para();\n}\n```\n\n'
      + 'The tests of the parser cover the order now, and the order is the whole '
      + 'of the error: nothing else moved.',
      // How long the model thought for, as the header of the panel reports it.
      reasoning_ms: 80000 }),
  said(3, 'Tool', 'cargo test --release markdown',
    { reasoning: 'The plan is written. Run the tests of the parser before the '
      + 'answer is sent, so the plan is checked and not only promised.',
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
  // A file the model changed, and a file it tried to make and did not. The header of each block
  // reports which tool it was, and the marker beside the header is that tool's own.
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
    tool: "Error: 'notes/todo.txt' already is there. create_file makes a new file "
      + 'and never writes over one; to change it, use edit_file.',
    failed: true
  }),
  // A look about the web. The header reports what was requested, and the answer is short text
  // from each place, so the block has no whole page in it.
  said(9, 'Tool', 'largest lake in Africa by volume', {
    tool_name: 'web_search',
    tool: 'Title: Lake Victoria - Wikipedia\nURL: https://en.wikipedia.org/wiki/Lake_Victoria\n'
      + 'Text: Lake Victoria is the largest lake in Africa by area, and the second-largest '
      + 'freshwater lake by area after Lake Superior.\n\n---\n\n'
      + 'Title: Lake Tanganyika - Wikipedia\nURL: https://en.wikipedia.org/wiki/Lake_Tanganyika\n'
      + 'Text: Lake Tanganyika is the largest tropical freshwater lake by volume, and the '
      + 'second oldest and second deepest lake in the world.\n\n---\n\n'
      + 'These are short summaries of each result. To read a whole page, call again '
      + 'with its address, as {"url": "https://..."}.'
  }),
  // An image handed to the model. The block carries the result of the run and the image itself:
  // the bytes go to the model with the result, and the user checks the same image, under an eye
  // that shows the model can see it.
  said(10, 'Tool', 'notes/cat.png', {
    tool_name: 'view_image',
    tool: 'Opened an image: notes/cat.png (PNG, 1024 by 768 pixels, 188 KiB). '
      + 'The image itself is sent to the model with this text, so you may be '
      + 'asked what it shows.',
    attachments: [{ id: 'att-cat', name: 'cat.png', mime: 'image/png', image: true }]
  })
];

const TALK = [
  said(1, 'You', 'What is the difference between mmap and mlock?', {
    attachments: [{ id: 'att-3', name: 'memory-map.png', mime: 'image/png', image: true }]
  }),
  said(2, 'Assistant', 'mmap lets the system page the model in and out. mlock keeps '
    + 'the whole file in the memory of the computer.\n\nUse mlock when the model must '
    + 'not be moved.'),
  said(3, 'You', 'Which one is the default?'),
  said(4, 'Assistant', 'mmap is the default. It costs less memory.'),
];

// A conversation whose only point is the blocks under the text of the user: an image of the
// usual form, an image taller than it is wide, a name too long for a block, a message of files
// and no text, and a file whose name is made of many parts. The second image is requested by an
// attachment whose number holds the word 'tall', so the two forms can be in one row.
const PARTS = [
  said(1, 'You', 'Two images of the plateau, the log of the run, and a note.', {
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

// The name of a file a model writes in the `writing` fixture, for the tool that works on a
// file. AN_TOOL names the tool; without it the fixture writes a command.
const WRITING_NAMES = {
  edit_file: 'notes/plan.md',
  create_file: 'notes/todo.txt',
  web_search: 'largest lake in Africa by volume',
  view_image: 'notes/cat.png',
  summarise_chat: 'the conversation of this chat'
};

// The text a file tool is making, as the user is given it while the call is written. The
// header of the block names the file, and the body holds this text, so the user sees the change
// as it is made.
const WRITING_WORDS = {
  edit_file: 'the old words of the plan\n> the new words of the plan',
  create_file: 'a short plan of the day, written down so it is not lost'
};

// The text under the chat composer while one of these tools is being written. They are the text
// the engine puts there, so the image does not invent its own.
const WRITING_DOING = {
  edit_file: 'Editing a file: ',
  create_file: 'Creating a file: ',
  web_search: 'Searching the web: ',
  view_image: 'Looking at an image: ',
  summarise_chat: 'Summarising '
};

// The command the model writes in the `writing` fixture, and the reason it gives for it.
// AN_WRITE counts the characters of both, one after the other.
const WRITING = 'cargo test --release markdown -- --nocapture';
const WRITING_WHY = 'see whether the order of the blocks is mended';
// What the model reports before the command, for AN_SAYS.
const WRITING_SAYS = 'I will write a short greeting to a file called hello.txt, '
  + 'and I will read it back to you.';

// The chats of the place, with one of them open.
function chats(count, open) {
  const out = [];
  for (let n = 0; n < count; n++) {
    out.push({
      id: 'c' + n, title: n === open ? 'Parser order' : 'Chat ' + (n + 1),
      repo: '', when: 'Today 09:0' + (n % 10), active: n === open,
      preview: n === open ? 'Nothing written yet' : 'The last line of that chat, stated short'
    });
  }
  return out;
}

// ------------------------------------------------------- the corporate display
//
// Corporate mode keeps each assistant message inside one progress card as an ordered sub-block.
// Tool outcomes are recorded between those messages; the last assistant message is the final
// answer sub-block.
function corpBase(extra) {
  return Object.assign({
    head: '', lines: [], tool: '', id: '', done: false, failed: false,
    adds: 0, cuts: 0, command: '', file: '', output: '', sub: [],
    line: false, group: false
  }, extra || {});
}

// One call of one tool, return or not.
function corpCall(tool, id, head, extra) {
  return corpBase(Object.assign({ tool: tool, id: id, head: head, done: true }, extra || {}));
}

// A run of calls of one tool made one after another. Its header is reported in the plural, with
// the number of the calls in it, and the calls are under it.
function corpGroup(tool, head, run) {
  return corpBase({ head: head, tool: tool, group: true, done: true, sub: run });
}

// Tool outcome blocks from the first answer of the fixture below. Assistant narration is
// supplied by the messages themselves, just as it is in the live conversation.
const CORP_WORK = [
  corpCall('run_terminal', 'call-1', 'Ran cargo test --release markdown', {
    command: 'cargo test --release markdown',
    output: 'running 6 tests\ntest markdown::tests::list_after_paragraph ... ok\n' +
      'all 6 tests passed'
  }),
  corpCall('edit_file', 'call-2', 'Edited a file notes/plan.md', {
    file: 'notes/plan.md', adds: 2, cuts: 1,
    output: 'Edited notes/plan.md (2 replacements)'
  }),
  corpGroup('create_file', 'Created files (3)', [
    corpCall('create_file', 'call-3', 'Created a file notes/a.md', { file: 'notes/a.md' }),
    corpCall('create_file', 'call-4', 'Created a file notes/b.md', { file: 'notes/b.md' }),
    corpCall('create_file', 'call-5', 'Created a file notes/c.md', {
      file: 'notes/c.md', done: false, failed: true,
      output: 'Error: an error came between the file and the disk.'
    })
  ])
];

// A context of two answers of the model, in the forms the corporate display is made of. The
// first answer has assistant progress updates and tool outcomes in the card, with its final
// response outside. The
// second answer is a short question with nothing but a thought in it, and its header reports
// how long the model thought for.
const CORP = [
  said(1, 'You', 'Fix the order the blocks come out in, run the tests, and tell me what changed.'),
  said(2, 'Assistant', 'I will read the parser first, then run the tests of it.', {
    reasoning: 'The error is in the order the blocks come out in, so read the parser before touching it.',
    reasoning_ms: 4200, work: [], work_head: ''
  }),
  said(3, 'Tool', 'cargo test --release markdown', {
    tool_name: 'run_terminal',
    tool: 'running 6 tests\ntest markdown::tests::list_after_paragraph ... ok\nall 6 tests passed'
  }),
  said(4, 'Tool', 'notes/plan.md', {
    tool_name: 'edit_file',
    tool: 'Edited notes/plan.md (2 replacements)'
  }),
  said(5, 'Assistant',
    'Two files changed, and the six tests of the parser pass.\n\n' +
    '- A list that follows a paragraph keeps its place.\n' +
    '- The bar beside a heading takes its height from the words beside it.\n\n' +
    'One file of the three I meant to make was already remaining there, so I left it.', {
    reasoning: 'The order is mended and the tests remain. Say what changed, and say of the file that was there.',
    reasoning_ms: 2600,
    work: CORP_WORK,
    work_head: 'Ran a command, Edited a file, Created files (3)',
  }),
  said(6, 'You', 'And the height of the bar?'),
  said(7, 'Assistant', 'The top bar takes its height from the text beside it, and never from the block under it.', {
    reasoning: 'A short question, and the answer of it is already known from the reading above.',
    reasoning_ms: 11800, work: [], work_head: 'Thought for 12 seconds'
  })
];

// The state of the engine, as the record of web::state.

// The lists of the side bar. Two repositories remain with their chats, one with more chats than
// a group shows at first; one repository was pinned, and so were two of the chats that belong
// to no repository; and three chats were put away. The text of the chats is the same few, for
// the top bar shows only names.
function lists(st) {
  var openId = String(st.chat_id || '');
  var home = !!st.home;
  var titles = ['The ledger of the spring', 'A note on the margins', 'Words for the preface',
    'The cost of the roof', 'A letter not sent', 'Names of the streets', 'The well and the pump',
    'Three days of rain', 'The key of the archive', 'A map of the orchard', 'What the fair cost',
    'The clock in the hall', 'A basket of letters'];
  var line = function (id, repo, n, pinned, archived) {
    return { id: id, title: titles[n % titles.length], repo: repo, when: 'yesterday',
      pinned: !!pinned, archived: !!archived, active: id === openId, messages: 4 + n,
      preview: 'A line of the chat.' };
  };
  var many = function (repo, from, count) {
    var out = [];
    for (var i = 0; i < count; i++) { out.push(line(repo + '-' + (from + i), repo, from + i, false, false)); }
    return out;
  };
  var out = {
    repositories: [
      { id: 'r-one', name: 'The Ledger', pinned: false, chats: many('r-one', 0, 4) },
      { id: 'r-two', name: 'The Orchard', pinned: false, chats: many('r-two', 4, 9) },
      { id: 'r-pin', name: 'The Archive', pinned: true, chats: many('r-pin', 13, 2) }
    ],
    loose: [
      line('l-one', '', 2, true, false),
      line('l-two', '', 5, true, false),
      line('l-three', '', 8, false, false),
      line('l-four', '', 11, false, false),
      line('l-five', '', 14, false, false),
      line('l-six', '', 17, false, false),
      line('l-seven', '', 20, false, false),
      line('l-eight', '', 23, false, false),
      line('l-nine', '', 26, false, false),
      line('l-ten', '', 29, false, false),
      line('l-eleven', '', 32, false, false),
      line('l-twelve', '', 35, false, false),
      line('l-thirteen', '', 38, false, false),
      line('l-fourteen', '', 41, false, false),
      line('l-fifteen', '', 44, false, false),
      line('l-sixteen', '', 47, false, false)
    ],
    archived: [
      line('a-one', '', 1, false, true),
      line('a-two', 'r-one', 6, false, true),
      line('a-three', 'r-two', 9, false, true)
    ]
  };
  // A fixture with a chat open that is not one of these must have a row of the side bar flagged
  // as the chat in use, or the marker is never seen.
  if (openId && !home) {
    var any = out.loose.some(function (c) { return c.active; }) ||
      out.repositories.some(function (r) { return r.chats.some(function (c) { return c.active; }); });
    if (!any) { out.loose[2].active = true; }
  }
  return out;
}

// The theme of a fixture: the pale ground, the dark ground, or the ladder of grey and white
// alone. The word of the theme travels with the state, as it does from the program, and the
// pale theme needs no marker of its own.
function scene(name, dark, theme) {
  const start = {
    rev: 1, unchanged: false, structure_rev: 1,
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
    // True while the program is writing a handoff summary. The text of one is never shown, so
    // the page draws no live answer while it remains.
    summarising: false,
    // The calls of a tool the model is writing right now. A block of a command being written is
    // made of these.
    live_tools: [],
    // How far the server is with the text it was sent. The meter above the chat composer is on
    // these three.
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
        + 'styling). The context window of the model is nearly full, so the '
        + 'whole summary must end within a few hundred words.'
    },
    thinking: {
      mode: 'off', available: true,
      modes: ['off', 'low', 'medium', 'high', 'xhigh'],
      // The model of these fixtures can keep the thinking of the earlier turns, and the choice
      // is on, as it does for a new chat.
      preserve: true, preserve_available: true
    },
    mcp: { python: 'python3', on: ['mcpdemo0001'], servers: [
      { id: 'mcpdemo0001', name: 'Bookshop', kind: 'stdio', icon: 7, shown: true, on: true, tools: ['look_up', 'reserve'], fault: '' },
      { id: 'mcpdemo0002', name: 'Clock', kind: 'internal', icon: 2, shown: true, on: false, tools: ['now'], fault: '' },
      { id: 'mcpdemo0003', name: 'Archive', kind: 'http', icon: 9, shown: false, on: false, tools: [], fault: 'The server did not answer' }
    ] },
    chatting: { collapse_thinking: true, grow_thinking: true,
                tools_in_thinking: true, collapse_tools: true,
                // The field of random digits the cursor uncovers in the background of the chat.
                // It remains on unless the image is asked for with AN_NOCURSORDIGITS=1.
                cursor_digits: process.env.AN_NOCURSORDIGITS !== '1',
                highlight: true, chapters: true },
    context: { used: '0', total: '8,192', ratio: 0.0 },
    // The lines of the drop-down of the badge of the tools, as a chat has them from the
    // beginning: every line switched on but the two that cost a long run. AN_TASKS=1 puts the
    // line of the tasks suite on as well.
    tool_switches: {
      commands: true, files: true, picture: true, hand_out: true, display: true,
      ask: true, suggest: true, steps: true, infographics: false,
      summarise: true, tasks: process.env.AN_TASKS === '1'
    },
    generating: false, busy: false, code: false, search: false, sandbox: false,
    sandbox_available: true,
    sandbox_note: 'bubblewrap works. A command sees only the working directory of '
      + 'the chat, and it has no network.',
    status: 'Ready', dark: !!dark, theme: theme || (dark ? 'dark' : 'light'),
    theme_pixel: process.env.AN_PIXEL === '1',
    data_dir: '/home/user/.local/share/autumn-natter',
    contexts_dir: '/home/user/.local/share/autumn-natter/contexts'
  };
  if (name === 'chat' || name === 'rich' || name === 'parts' || name === 'streaming'
    || name === 'streaming-code' || name === 'previews') {
    start.home = false;
    start.chat = { id: 'c1', title: name === 'rich' ? 'Parser order' : 'Memory of the conversation' };
    start.chats = chats(name === 'previews' ? 9 : 3, 1);
    start.carousel = { index: 1, count: start.chats.length, blank: false };
    start.messages = name === 'rich' ? FULL : name === 'parts' ? PARTS : TALK;
    // AN_CUT=1 turns the run of the file tool into a call the user stopped manually. The block
    // stays in the chat, its pill reports 'cancelled', and under the second label remains the
    // word of the stop and not a result.
    if (process.env.AN_CUT === '1') {
      start.messages = start.messages.map(function (m) {
        if (m.tool_name !== 'edit_file') { return m; }
        return Object.assign({}, m, {
          cancelled: true,
          failed: true,
          tool: 'The call was cancelled: you stopped the answer while the call was '
            + 'being written, so the writing was stopped and the tool never ran',
          tool_args: '{"path":"notes/plan.md","edits":[{"old_string":"the fi'
        });
      });
    }
    // AN_GIFTS=1 ends the conversation with a run of the download tool and the blocks of the
    // files it proposed: the marker of the type of file, the name with the ending under the
    // name, the button at the right of the block, and the button of all of the proposal under
    // several files. The blocks are in the middle of the chat, under all of the answer.
    if (process.env.AN_GIFTS === '1') {
      start.messages = start.messages.concat([
        said(21, 'Tool', 'a-page-of-the-work.html', {
          tool_name: 'offer_files',
          tool: '2 files are offered to the user in download blocks: '
            + 'a-page-of-the-work.html, counts/of-the-run.csv',
          downloads: [
            { name: 'a-page-of-the-work', extension: 'html', kind: 'page',
              path: 'a-page-of-the-work.html', size: 4213 },
            { name: 'the counts of the run', extension: 'csv', kind: 'table',
              path: 'counts/of-the-run.csv', size: 118 }
          ]
        }),
        said(22, 'Assistant',
          'The page and the table of the run are under this answer, each with a '
          + 'button of its own.\n\nThe page holds the work, and the table the '
          + 'counts of the runs it went through.')
      ]);
    }
    // AN_SUGGEST=1 ends the conversation with a run of the show tool and a run of the
    // suggestion tool. The button that opens the window over the files shown and the buttons of
    // the paths proposed for what comes next both are at the end of the answer, one under the
    // other, and with them the blocks of AN_GIFTS if that remains too. Three types in one place
    // is how the place is proved.
    if (process.env.AN_SUGGEST === '1') {
      start.messages = start.messages.concat([
        said(23, 'Tool', 'notes/plan.md', {
          tool_name: 'display_files',
          tool: '1 file is in the window over the chat: notes/plan.md',
          displayed: [{ path: 'notes/plan.md', size: 2140 }]
        }),
        said(24, 'Tool', 'the plan of the parser', {
          tool_name: 'offer_suggestions',
          tool: '3 next steps are offered for the end of your answer: Run the '
            + 'tests of the parser, Show me the diff, Read the log of the run',
          suggestions: [
            { label: 'Run the tests of the parser',
              prompt: 'Run the tests of the parser and tell me what failed.' },
            { label: 'Show me the diff' },
            { label: 'Read the log of the run',
              prompt: 'Read the log of the last run and say what it cost.' }
          ]
        }),
        said(25, 'Assistant',
          'The plan is in the window over the chat, and the suggestions the conversation may go '
          + 'on by stand under this answer.\n\nPress one of them, or write what '
          + 'you would rather have.')
      ]);
    }
    // AN_STEPS=1 ends the conversation with a run of the steps tool, so the block of the steps
    // of the work is in the card with the other types. Four types in one place is how the card
    // is proved.
    if (process.env.AN_STEPS === '1') {
      start.messages = start.messages.concat([
        said(26, 'Tool', 'the plan for the parser', {
          tool_name: 'show_steps',
          tool: 'The steps of the work are under your answer: Read the fault, '
            + 'Write the parser, Run the tests, Tell the user',
          steps: {
            title: 'The plan for the parser',
            steps: [
              { label: 'Read the fault', content: 'Read the fault in the run and '
                  + 'say which line of the parser gives it.' },
              { label: 'Write the parser', content: 'Write the parser of the plan '
                  + 'in `markdown.rs`, one function for each type of line.' },
              { label: 'Run the tests', content: 'Run the tests of the parser and '
                  + 'keep the ones that fail in the answer.' },
              { label: 'Tell the user', content: 'Say what changed, what the tests '
                  + 'cost, and what is still open.' }
            ]
          }
        }),
        said(27, 'Assistant',
          'The steps of the work are under this answer. Press one of them and '
          + 'the block shows that step.')
      ]);
    }
    // AN_TASKS=1 switches on the line of the tasks suite, and ends the conversation with a run
    // of each of its six tools, so the header of every type of block of the suite can be
    // checked together. The line of the drop-down remains off in a chat from the beginning, as
    // it does in the program.
    if (process.env.AN_TASKS === '1') {
      start.messages = start.messages.concat([
        said(30, 'Tool', 'Lay out the reader', {
          tool_name: 'create_task',
          tool: 'Task made.\nindex: 3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60\n' 
            + 'status: open\nname: Lay out the reader\nsummary: Read the grammar '
            + 'and write the table of rules.\n\n1 task or tasks of this work '
            + 'directory stand open.'
        }),
        said(31, 'Tool', 'the list of tasks', {
          tool_name: 'view_tasks',
          tool: '2 tasks stand for this working directory: 1 open, 1 completed, 0 cancelled.\n\n'
            + '3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60  [open] Lay out the reader\n'
            + '    Read the grammar and write the table of rules.\n'
            + 'a27d5e10-90c4-4f3b-8d2a-71bc0e5f9d13  [completed] Read the fault\n'
            + '    Say which line of the user gives the error.\n\n'
            + 'Each index above is the number of the task. The contents of a task '
            + 'are not listed here: call view_task with its index for those.'
        }),
        said(32, 'Tool', '3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60', {
          tool_name: 'view_task',
          tool: 'index: 3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60\nstatus: open\n'
            + 'name: Lay out the reader\nsummary: Read the grammar and write the '
            + 'table of rules.\ncontent:\nTake the grammar of the plan, then write '
            + 'the table of rules it is on. Run the six tests of the user '
            + 'when the table is written, and keep the error in any test that fails.'
        }),
        said(33, 'Tool', '3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60', {
          tool_name: 'complete_task',
          tool: 'Task 3f9a1c22-6b4e-4d0f-9a71-2c5e8b41df60 is completed: Lay out '
            + 'the user.\n\n0 task or tasks of this working directory are left open.'
        }),
        said(34, 'Tool', 'a27d5e10-90c4-4f3b-8d2a-71bc0e5f9d13', {
          tool_name: 'cancel_task',
          tool: 'Task a27d5e10-90c4-4f3b-8d2a-71bc0e5f9d13 is cancelled: Read the '
            + 'fault.'
        }),
        said(35, 'Tool', 'No task is with the index bbbb', {
          tool_name: 'delete_task', failed: true,
          tool: 'No task is with the index bbbb. Use view_tasks to see the '
            + 'indexes.'
        }),
        said(36, 'Assistant',
          'Both tasks of this working directory are dealt with, and the list was read '
          + 'before this answer.\n\nThe user is decompressed and the six tests pass. '
          + 'The task about the error was cancelled, as you asked.')
      ]);
    }
    // The chapters the model flagged, tied to the message each begins after.
    start.chapters = name === 'rich'
      ? [{ id: 'ch1', name: 'The plan for the parser', after: 1 },
         { id: 'ch2', name: 'Running the tests', after: 3 },
         { id: 'ch3', name: 'The bar beside a list', after: 4 }]
      : [];
    // The handoff summaries the model wrote when the conversation came near the end of its
    // space. They are among the text as the header of the summarisation, and on the line at the
    // right of the chat.
    if (name === 'rich') {
      addSummaries(start, 2);
    }
    // AN_SUMMARIES=N puts that many summarisations in the conversation, so the line can be
    // checked when a chat has been summarised many times. AN_NOTIMELINE=1 turns the switch of
    // the Compaction section off, and AN_BROKEN=1 leaves the newest summarisation with no text,
    // which is what a server that was removed in the middle of a summary leaves behind.
    if (process.env.AN_SUMMARIES !== undefined) {
      addSummaries(start, Number(process.env.AN_SUMMARIES) || 0);
    }
    if (process.env.AN_NOTIMELINE === '1') { start.compaction.timeline = false; }
    // AN_NOCOMPACTION=1 puts the Compaction section on `No summary`. The line of the handoff
    // summary then leaves the menu of the badge of the tools, for the tool has nothing to do in
    // a chat that is never summarised.
    if (process.env.AN_NOCOMPACTION === '1') { start.compaction.type = 'off'; }
    if (process.env.AN_BROKEN === '1' && start.handoffs.length) {
      var lastCut = start.handoffs[start.handoffs.length - 1];
      lastCut.state = 'owed';
      lastCut.snippet = '';
      lastCut.why = 'the server was removed while the summary was written. Start it again';
      for (var k = 0; k < start.messages.length; k++) {
        if (start.messages[k].id === lastCut.id) {
          start.messages[k].body = '';
          start.messages[k].tool = lastCut.why;
        }
      }
    }
    // AN_SUMMARISING=1 puts the chat in the state the engine is in while a handoff summary is
    // being written: an answer is on the way, the marker of the summarisation is at the end of
    // the conversation with no text in it, and the text of the summary is not shown. The window
    // has no live answer to draw.
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
        tool: 'The handoff summary is being written ...', mono: false, failed: false,
        when: '11:02', when_full: '2026-10-21, 11:02',
        context_before: 6964, context_after: 0, attachments: []
      }]);
      start.handoffs = (start.handoffs || []).concat([{
        id: 399, number: writing, state: 'writing', when: '2026-10-21, 11:02',
        before: 6964, after: 0, snippet: '', why: ''
      }]);
    }
    // AN_CHAPTERS=N puts that many markers on the line, spread over the conversation, so the
    // line can be checked when a chat has more chapters than space. Zero means no chapter at
    // all. AN_NOCHAPTERS=1 leaves the markers in the chat but turns the switch of the Chatting
    // section off.
    if (process.env.AN_CHAPTERS !== undefined) {
      var many = Number(process.env.AN_CHAPTERS) || 0;
      var ids = start.messages.map(function (m) { return m.id; });
      start.chapters = [];
      for (var n = 0; many > 0 && n < many; n++) {
        start.chapters.push({
          id: 'ch' + n, name: 'Chapter ' + (n + 1) + ' of the long conversation',
          after: ids.length ? ids[Math.floor(n * ids.length / many)] : 0
        });
      }
    }
    if (process.env.AN_NOCHAPTERS === '1') { start.chatting.chapters = false; }
    // AN_USERMSGS=N puts that many messages of the user in the conversation, so the line of the
    // dashes can be checked when a chat holds more of them than the screen is tall for. They go
    // on the end of the conversation, and the answers between them are the short type, for the
    // line cares only about the questions.
    if (process.env.AN_USERMSGS !== undefined) {
      var howMany = Number(process.env.AN_USERMSGS) || 0;
      var next = 500;
      for (var u = 0; u < howMany; u++) {
        start.messages.push(said(next++, 'You',
          'Message ' + (u + 1) + ' of the many the user wrote in this chat, '
          + 'with a little more in it than a word.'));
        start.messages.push(said(next++, 'Assistant',
          'Done. That was answer ' + (u + 1) + '.'));
      }
    }
    // AN_RUNMARKS=1 puts a command at work with time to run out, so the markers of the header
    // of its block - the clock, the marker that adds five minutes, and the marker that stops
    // the call - are in the image.
    if (process.env.AN_RUNMARKS === '1') {
      start.generating = true;
      start.messages = start.messages.concat([{
        id: 471, role: 'Tool', body: 'cargo test --release',
        reasoning: '', reasoning_ms: 0, tool: 'running ...', mono: true, failed: false,
        tool_name: 'run_command', tool_args: 'cargo test --release', when: '11:04',
        when_full: '2026-10-21, 11:04', context_before: 3204, context_after: 0,
        attachments: []
      }]);
      start.running = { id: 471, timeout: 600, left: 287 };
    }
    // AN_LONGTOOL=4000 puts a run of the terminal tool in the chat whose result is that many
    // lines long, which is what a model that read a large file with one command brings back.
    // Nothing of the result is cut, so the block has to hold it: the check is that the page
    // scrolls the result inside the block instead of growing the chat past the window.
    if (process.env.AN_LONGTOOL) {
      var longLines = Number(process.env.AN_LONGTOOL) || 4000;
      var madeLines = '';
      for (var l = 1; l <= longLines; l++) {
        madeLines += 'line ' + l + ' of the file the command read\n';
      }
      start.messages = start.messages.concat([{
        id: 472, role: 'Tool', body: 'cat notes/big.md',
        reasoning: '', reasoning_ms: 0, tool: madeLines, mono: true, failed: false,
        tool_name: 'run_terminal', tool_args: '{"command":"cat notes/big.md"}', when: '11:06',
        when_full: '2026-10-21, 11:06', context_before: 5120, context_after: 0,
        attachments: []
      }]);
    }
    // AN_QUEUE=1 puts messages in the waiting list of the chat, as the program keeps them - the
    // next to go out first - with an answer still being written over them. AN_QUEUEHELD=1
    // markers the list as stopped, which is how it looks when a message of it could not be
    // sent.
    if (process.env.AN_QUEUE === '1') {
      start.generating = true;
      start.queue = {
        chat: 'c1',
        held: process.env.AN_QUEUEHELD === '1',
        items: [
          { id: 1, text: 'The oldest message, which is sent first.', files: [] },
          { id: 2, text: 'A second message, with a file hung on it.', files: ['notes-for-the-chat.md'] },
          { id: 3, text: 'The newest of them, which is at the top of the list.', files: [] }
        ]
      };
    }
    // AN_GOAL=1 puts the persistent goal bar over the message queue, as it is while
    // the model is pursuing a goal and has messages waiting to go.
    if (process.env.AN_GOAL === '1') {
      start.chat.goal = {
        request: 'Finish the parser refactor, run the relevant tests, and report the verified result.',
        started: Math.floor(Date.now() / 1000) - 367,
        elapsed: 367,
        paused: false
      };
    }
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
    // A block of code that is still being written, so the colours of it are painted again while
    // it grows.
    start.generating = true;
    start.live = 'The user of the fence is small:\n\n'
      + '```python\ndef read(line):\n    # a fence opens the block\n'
      + '    return line.startswith("```")  # and closes it';
    // The fence is left open on purpose: the block is still being written.
    start.reasoning = 'Show a fence arriving, so the repaint of a growing block is seen.';
    start.status = 'Answering';
    start.context = { used: '5,980', total: '8,192', ratio: 0.73 };
  }
  if (name === 'processing') {
    // The text of the answer has not begun, so the meter above the chat composer is the only
    // item that shows the program is at work.
    start.home = false;
    start.chat = { id: 'c1', title: 'Memory of the conversation' };
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.messages = [
      said(1, 'You', 'What is the difference between mmap and mlock?'),
      said(2, 'Assistant', 'mmap lets the system page the model in and out. mlock '
        + 'keeps the whole file in the memory of the computer.\n\nUse mlock when the '
        + 'model must not be moved.'),
      said(3, 'You', 'And which one costs less?'),
    ];
    start.generating = true;
    start.status = 'thinking';
    start.progress = { percent: 42, active: true, what: 'text' };
    start.context = { used: '3,204', total: '8,192', ratio: 0.39 };
  }
  if (name === 'writing') {
    // The model is writing a command. The block is in the chat while the text of it come, and
    // not only when the command runs.
    start.home = false;
    start.chat = { id: 'c1', title: 'Memory of the conversation' };
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.messages = [
      said(1, 'You', 'What is in the working directory of the chat? Run a command and tell me.'),
    ];
    start.generating = true;
    start.status = 'The model writes a command';
    // The text of the answer is nothing by default: the model spends it on the call.
    // AN_SAYS=<n> puts that a lot of text of an answer before the call, so the space between
    // them and the block of the command can be checked.
    start.live = '';
    if (process.env.AN_SAYS !== undefined) {
      var told = Math.max(0, Number(process.env.AN_SAYS) || 0);
      start.live = WRITING_SAYS.slice(0, Math.min(WRITING_SAYS.length, told));
    }
    start.reasoning = 'Look in the folder first, then say what is in it.';
    // The header of the thinking panel reports how long the thoughts have taken while they are
    // coming, and the count of the calls that are among them, so an image of a live answer
    // wants a length of its own to say, and one that is over wants the marker that shows it is
    // over.
    if (process.env.AN_THOUGHTMS !== undefined) {
      start.reasoning_ms = Number(process.env.AN_THOUGHTMS) || 0;
    }
    if (process.env.AN_THOUGHTDONE === '1') { start.reasoning_done = true; }
    var tool = process.env.AN_TOOL || 'run_terminal';
    var named = WRITING_NAMES[tool] !== undefined;
    // A file tool names its file in the header of the block, and makes its text in the body, so
    // the two remain apart here as they do in the engine.
    var fileTool = tool === 'edit_file' || tool === 'create_file';
    // The badge of the web search is on for a search, and the badge of the tools is on for the
    // tools that can run on the computer.
    start.search = tool === 'web_search';
    // The badge of the tools is on for every tool that can run on the computer, which is all of
    // them but the look about the web.
    start.code = tool !== 'web_search';
    var name = named ? WRITING_NAMES[tool] : '';
    // The text the call is making: the text of a file tool, the name of the item another tool
    // checks, or the command of a run.
    var full = fileTool ? WRITING_WORDS[tool] : (named ? name : WRITING);
    // A name of a file is short, so all of it remains unless the number of characters was
    // requested.
    var have = process.env.AN_WRITE === undefined ? (named ? full.length : 12)
      : Math.max(0, Number(process.env.AN_WRITE) || 0);
    start.live_tools = [{
      name: tool,
      command: full.slice(0, have),
      target: fileTool ? name : '',
      // The reason comes after the command, as the model writes it. A tool that works on a file
      // never requested a reason.
      purpose: !named && have > WRITING.length
        ? WRITING_WHY.slice(0, have - WRITING.length) : ''
    }];
    if (named) {
      // A user of the image should not wonder why the chat composer requests a command over the
      // block of a file tool, nor why a look about the web is under a word about files.
      var asked = {
        web_search: 'Look up how large the largest lake in Africa is, and tell me what you found.',
        view_image: 'Look at the image of the cat in the chat, and tell me what is in it.',
        summarise_chat: 'The first part of the work is finished. Put the conversation into a summary, and go on with the second part in the space that is created.'
      };
      start.messages = [said(1, 'You', asked[tool] ||
        'Put the plan in a file of the chat, and tell me what you changed.')];
      start.status = (WRITING_DOING[tool] || WRITING_DOING.create_file) + name;
    }
    start.context = { used: '1,180', total: '8,192', ratio: 0.14 };
  }
  if (name === 'streaming') {
    start.generating = true;
    start.live = 'The top bar takes its height from the text beside it, and the list '
      + 'keeps its place after the paragraph.';
    start.reasoning = 'Look at the order of the blocks first.';
    // The thinking is over while the answer is still coming, so the header of the panel reports
    // how long the model thought for.
    start.reasoning_ms = 12400;
    start.reasoning_done = true;
    start.status = 'Answering';
    start.context = { used: '5,980', total: '8,192', ratio: 0.73 };
  }
  if (name === 'markdown') {
    // A conversation made of the blocks that the markers of the markdown make: a table, whose
    // columns remain as the colons under its header stated they would, a quote with a list and
    // a block of code remaining inside it, the markers across the page that divide two parts of
    // an answer, and links. A link gives its text and keeps its address in itself; an address
    // that is not one to follow is read as text. The last message is a table of one column,
    // which is still a table.
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
        + 'The rule the two blocks remain by:\n\n'
        + '> The bar takes its height from the words beside it, and never from '
        + 'the block that is under it.\n'
        + '>\n'
        + '> - a heading, and\n'
        + '> - a list\n'
        + '>\n'
        + '> ```rust\n> fn height(words: &Words) -> f32 {\n>     lines(words) * 1.55\n> }\n> ```\n\n'
        + 'A line of text of its own, after all of that.',
        // The thoughts of an answer go through the same blocks, so a table and a quote are in
        // the panel of the thinking as well.
        { reasoning: 'The table wants the two heights that were measured, not the '
          + 'ones that were planned.\n\n| block | planned | measured |\n| --- | --- | --- |\n'
          + '| a heading | 15 | 16 |\n\n> The bar takes its height from the words '
          + 'beside it.\n\nSo the bar is not touched.',
          reasoning_ms: 64000 }),
      said(3, 'You', 'And a table of one column?'),
      said(4, 'Assistant',
        '| the whole of it |\n| :- |\n| one cell, standing in the middle |\n\n'
        + 'Nothing else. A table of one column is a table still.'),
      said(5, 'You', 'And the markers across the page, with a link?'),
      said(6, 'Assistant',
        'The rule divides one part of an answer from the next. It is a hairline of '
        + 'the neutral, for the accent is kept for the markers of the program.\n\n'
        + '---\n\n'
        + 'A link gives its words and keeps its address in itself: [the llama.cpp '
        + 'page](https://github.com/ggml-org/llama.cpp), and an address written '
        + 'whole <https://autumn.example/notes>.\n\n'
        + '- an item with [a link of its own](https://autumn.example/one)\n'
        + '- an item with an address that is not one to follow: [run it]'
        + '(javascript:alert(1))\n\n'
        + '# A heading with [a link](https://autumn.example/head) in it',
        { reasoning: 'The rule wants a hairline of the neutral, and the text of a '
          + 'link the bright accent of the theme.\n\n---\n\nSee [the palette]'
          + '(https://autumn.example/palette), '
          + 'and read <https://autumn.example/two> after it.',
          reasoning_ms: 3100 }),
      said(7, 'You', 'And a table of seven columns, in a chat this wide?'),
      said(8, 'Assistant',
        'The text of a cell break to fit, so the chat keeps the width it had.\n\n'
        + '| file | what it holds | lines | before | after | took | left |\n'
        + '| :--- | :--- | ---: | ---: | ---: | ---: | ---: |\n'
        + '| markdown.rs | the cutting of the blocks into kinds | 390 | 14 | 16 | 3 min | none |\n'
        + '| page.html | the page of the remote mode | 3207 | 6 | 8 | 1 min | none |'),
      // The four colours a model may put on its own text. The markers are in the text and are
      // painted: the two greens and the two reds of the page, in a line of text and inside a
      // heading. The switch of the Chatting settings is AN_NOCOLOUR=1 below, which reads the
      // same text with no colour.
      said(9, 'You', 'And colour on a few of the text?'),
      said(10, 'Assistant',
        'The model names the colour and the page paints the text between its two '
        + 'marks: <green>the build passed</green>, <light_green>it may pass</light_green>, '
        + '<red>the build failed</red> and <light_red>it may fail</light_red>.\n\n'
        + '## <red>A heading with a fault in it</red>\n\n'
        + 'A colour that nothing closes is left as it was written: <green>and here the '
        + 'model stopped before the marker that closes it.',
        { reasoning: 'The markers are read inside one line, and the text between them keeps '
          + 'their place. <light_red>The markers go out of the conversation that goes back to the model.</light_red>',
          reasoning_ms: 2100 }),
    ];
    start.context = { used: '2,040', total: '8,192', ratio: 0.25 };
  }
  // The corporate display. The fixture `corp` is a context of two answers of the model, both
  // come to their end. The first card holds thoughts, progress updates, and tool outcomes;
  // its final response is outside. The other card is nothing but a thought, and its header
  // reports how long the model thought for. The fixture `corplive` is in the middle of the first
  // answer: the card is being written, its header reports the current action, and live text is
  // held inside until generation ends. AN_CORP=1 puts the display over any other fixture.
  if (name === 'corp' || name === 'corplive' || name === 'corptools') {
    start.home = false;
    start.chat = { id: 'c1', title: 'Order of the blocks' };
    start.chatting.corporate = true;
    start.chats = chats(3, 1);
    start.carousel = { index: 1, count: 3, blank: false };
    start.context = { used: '3,204', total: '8,192', ratio: 0.39 };
    start.code = true;
    start.search = true;
    if (name === 'corp') {
      start.messages = CORP;
      start.status = 'Ready';
    } else if (name === 'corptools') {
      start.messages = [said(80, 'You', 'Read and update these files.')];
      start.generating = true;
      start.live = '';
      start.reasoning = '';
      start.live_work = [
        { head: 'Reading main.rs', tool: 'run_terminal', id: 'read-message',
          call_id: 'tool-read', command: 'cat src/main.rs', file: '',
          reading: true, done: false, sub: [], lines: [] },
        { head: 'Editing file src/parser.rs', tool: 'edit_file', id: 'stream:tool-edit',
          call_id: 'tool-edit', file: 'src/parser.rs', done: false },
        { head: 'Creating file notes/summary.md', tool: 'create_file', id: 'stream:tool-create',
          call_id: 'tool-create', file: 'notes/summary.md', done: false }
      ];
      start.tool_previews = start.live_work.splice(1);
      start.messages.push(said('read-message', 'Tool', 'cat src/main.rs', { activity: start.live_work[0] }));
      start.live_work_head = 'Reading main.rs';
      start.live_tools = [
        { call_id: 'tool-read', index: 0, name: 'run_terminal',
          command: 'cat src/main.rs', purpose: '', target: '', done: true },
        { call_id: 'tool-edit', index: 1, name: 'edit_file',
          command: '', purpose: '', target: 'src/parser.rs', done: false },
        { call_id: 'tool-create', index: 2, name: 'create_file',
          command: '', purpose: '', target: 'notes/summary.md', done: false }
      ];
      start.status = 'Writing a tool call';
    } else {
      start.messages = CORP.slice(0, 4);
      start.generating = true;
      start.live = 'Two files changed, and the six tests of the parser pass.\n\n'
        + '- A list that follows a paragraph keeps its place.\n';
      start.reasoning = 'The order is mended and the tests remain. Say what changed, '
        + 'and say of the file that was there.';
      start.reasoning_ms = 2600;
      start.reasoning_done = true;
      start.live_work = CORP_WORK;
      start.live_work_head = 'Creating files (3)';
      start.status = 'Answering';
      if (process.env.AN_CORPCLOSED === '1') {
        start.live_work_head = 'Ran a command, Edited a file, Created files (3)';
      }
    }
  }
  if (process.env.AN_CORP === '1') {
    start.chatting.corporate = true;
  }
  return start;
}

const CONFIG = {
  llama_dir: '/home/user/src/llama.cpp/build/bin',
  models_dir: '/home/user/models',
  repositories_dir: '',
  remote_port: 8723,
  upnp_enabled: false,
  theme: 'light',
  dark_mode: false,
  chat_collapse_thinking: true,
  chat_grow_thinking: true,
  chat_tools_in_thinking: true,
  chat_collapse_tools: true,
  chat_date_new_chat: true,
  chat_highlight: true,
  chat_chapters: true,
  // A sent message is carried from the box into the chat.
  chat_send_anim: true,
  // The two rooms for steps remain unlimited, as they do for a new set of settings.
  chat_max_tool_steps: -1,
  chat_max_answer_steps: -1,
  // A paste of a lot of text becomes a file at six thousand characters.
  pasted_text_char_limit: 6000,
  // Compaction: the handoff summary, at a share of the space, with the line of the
  // summarisations remaining at the right of the chat.
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
    + 'come to an end within a few hundred words. Say what was done, what is in '
    + 'hand right now, and what must be done next. Name the files that matter with '
    + 'one short line each.',
  // The text before the conversation. The shot reads the prompt of the user as if the user
  // wrote it, and markers both pre-written prompts, so the table of them is drawn with its
  // markers on.
  chat_system_prompt: 'Answer in short sentences. Name the file and the line.',
  chat_system_prompts_on: ['engineer', 'adhd'],
  chat_timeline: true
};

// The pre-written prompts of the Program tab. The text here are short remain-ins for the text
// that is in the program: a shot only has to show that the table is drawn with its markers, and
// that the window of reading opens with the text of the prompt that was clicked on.
var PROMPT_LINES = [
  { id: 'engineer', name: 'Software Engineer Writing Style',
    summary: 'Plain professional English for developers: standard terms, short '
           + 'sentences, no filler, facts only.',
    words: 'You are a technical assistant for software developers. Write in clear, '
         + 'normal professional English.\n\nLANGUAGE\n'
         + '- Use standard technical terms.\n'
         + '- Keep sentences short, under about 25 words.' },
  { id: 'adhd', name: 'ADHD Rules',
    summary: 'Answers the user can act on at once: the answer first, short lists, '
           + 'one next step, no recap.',
    words: 'Write every response so the user can act on it without scrolling or '
         + 're-reading. Put the answer first.\n\nRULES\n'
         + '1. Start with the next action or the direct answer.\n'
         + '2. If the task has more than one step, use a numbered list.' }
];

// The answer of the program to a request for the prompts. It is written inside the bridge that
// is put into the page, because the page is the one that asks.

const MODEL = {
  id: 'm1', name: 'Qwen3.5-0.8B-UD-Q6_K', kind: 'GGUF',
  settings: {
    ctx_size: 8192, kv_index: 1, load_index: 1, spec_index: 0, draft_model: '',
    draft_tokens: 4, ngram_match: 0, ngram_max: 0, ngram_min: 0, parallel: 1, batch_size: 512, ubatch_size: 128, vision: false,
    split_mode: 'layer',
    temperature: 0.7, top_p: 0.95, top_k: 20, min_p: 0, repeat_penalty: 1,
    presence_penalty: 0, max_tokens: -1, seed: -1,
    ctx_checkpoints: 32, cache_ram: 8192, cache_prompt: true,
    image_min_tokens: 0, image_max_tokens: 0, no_context_shift: false,
    mmproj_cpu: false,
    reasoning_budget: -1, reasoning_message: '', chat_template: '', extra_args: '',
    send_default_args: true,
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
  // The choices of `--spec-type` the stylesheet shows, as `llama::spec_names` gives them. The
  // last is the pair: ngram-mod and MTP at once.
  specs: ['none', 'draft-simple', 'draft-eagle3', 'draft-mtp', 'draft-dflash',
    'draft-dspark', 'ngram-simple', 'ngram-map-k', 'ngram-map-k4v', 'ngram-mod',
    'ngram-cache', 'ngram-mod + MTP']
};

// The logs of llama.cpp, for the Logs tab of the settings panel. The current log is the one
// this run is writing, so it holds the text of a start that went wrong: the line the program
// puts at the start of every log, a warning, and an error in the model file. The older log
// means a start of an hour ago, and it is longer than what is read, so the text about the start
// of the log can be seen as well.
const LOG_TEXT = [
  'autumn-natter 0.1.0 began this log on 2026-10-23, 21:05.',
  'the file that was run: /home/user/src/llama.cpp/build/bin/llama-server',
  'the arguments: -m /home/user/models/Qwen3.5-0.8B-UD-Q6_K_XL.gguf --host 127.0.0.1',
  '  --port 41987 --no-webui --props --slots -v --log-timestamps --log-colors off -c 8192',
  '',
  '0.00.162 I log_info: version: b9100-3f0c1d',
  '0.00.162 I log_info: loaded build 2026-10-19T09:14:22Z, x86_64-pc-linux-gnu, cc 13.2',
  '0.00.163 I log_info: using LLAMA_CUBLAS=1',
  '0.00.402 I ggml_backend_cuda_buffer_type_alloc_buffer: allocating 1024.00 MiB of device memory',
  '0.01.118 W load: the model has no chat template, so --jinja was dropped and no tool is offered',
  '0.01.204 D llama_new_context_with_model: n_ctx      = 8192',
  '0.01.205 D llama_new_context_with_model: mem_size   = 100663296 bytes',
  '0.01.902 I load: model is loaded',
  '0.02.010 I srv  srv_start: model is loaded in 1.90 seconds',
  '0.02.011 I srv    operator(): listening on 127.0.0.1 port 41987',
  '0.03.441 I slot get_avail_slot: assigned slot  0 to task, n_tokens = 612',
  '0.03.441 D slot launch_slot_: processing task, id =  0, n_tokens = 612',
  '0.04.052 I slot print_timing:    0 |  prompt eval time =   611.00 ms / 612 tokens',
  '0.04.052 I slot print_timing:    0 |  eval time       =  1204.00 ms / 148 tokens',
  '0.11.320 E srv  operator(): error: the model file could not be read: no such file',
  '0.11.320 E srv  operator(): the request was refused and the chat was left as it stood',
  '0.11.321 W slot ~slot: the slot was given back while an answer was still being written',
  '0.11.402 I slot get_avail_slot: assigned slot  0 to task, n_tokens = 640',
  '0.12.010 I slot print_timing:    0 |  prompt eval time =   588.00 ms / 640 tokens',
  '0.12.640 I slot print_timing:    0 |  eval time       =  1222.00 ms / 162 tokens'
].join('\n');

const LOGS = {
  kind: 'server',
  dir: '/home/user/.config/autumn_natter/logs',
  version: '0.1.0',
  kept: 10,
  logs: [
    { id: 'server-1762146344.log', when: '2026-10-23, 21:05', size: '18 KiB',
      bytes: 18942, current: true },
    { id: 'server-1762140120.log', when: '2026-10-23, 19:22', size: '1.2 MiB',
      bytes: 1258291, current: false },
    { id: 'server-1762131000.log', when: '2026-10-23, 16:50', size: '302 KiB',
      bytes: 309248, current: false }
  ]
};

// The log the program writes of its own work, for the other face of the Logs tab. The lines are
// of the form the program writes them in: the moment to the millisecond, one word for the part
// it came from, and the item that happened.
const PROG_TEXT = [
  '2026-11-02 03:01:41.902 start the log of this run is begun. Autumn Natter 0.1.0',
  '2026-11-02 03:02:18.047 meter the meter moved from 412 to 1,284 while an answer was coming in',
  '2026-11-02 03:02:19.115 space the size of the context is weighed on 6,964, which is weighed on the conversation as it is now, the report of a call that was stopped left out of it; the context window of the model is 8,192, the limit set for the summarisation is at ,554; a handoff summary is wanted',
  '2026-11-02 03:02:19.118 summary a handoff summary is begun. The context meter is at ,964, and the text that go out with the request measure 6,902',
  '2026-11-02 03:02:24.604 summary an ask for the summary came back: the server spoke 6,902 words in and 412 out; the writing of it is at an end, it came to its end with stop',
  '2026-11-02 03:02:24.711 summary the handoff summary #1 is kept: 6,964 of the conversation was for 412 in the summary, and the context meter goes back to 1,041',
  '2026-11-02 03:04:02.330 cut the writing is stopped at 6,588 of 8,192, for the limit set for the summarisation stands at 6,554; the model was writing the words of a tool call',
  '2026-11-02 03:04:02.331 room no summary is asked for: one stands owed from before and was never written, so the user is asked to start it again',
  '2026-11-02 03:04:02.340 cut the answer ends at the cut. The rule that ended it: the summary was never written. The next ask would carry 2,412, the request that was cut carried 6,588, the conversation without the without the report of the call measures 1,980, the limit is at ,554 of 8,192'
].join('\n');

const LOGS_PROGRAM = {
  dir: '/home/user/.config/autumn_natter/logs',
  kind: 'program',
  version: '0.1.0',
  kept: 10,
  logs: [
    { id: 'program-1762146101.log', when: '2026-11-02, 03:01', size: '6 KiB',
      bytes: 6184, current: true },
    { id: 'program-1762140120.log', when: '2026-10-23, 19:22', size: '44 KiB',
      bytes: 45056, current: false }
  ]
};

// One log of the record above. The older one means a start that wrote more than is read, so the
// stylesheet reports that the start of the log is not shown. The two answers remain as data,
// for the bridge is handed out as text and can carry no function of this file.
const LOG_ONE = {
  now: { id: 'server-1762146344.log', kind: 'server', text: LOG_TEXT, bytes: 18942, cut: false },
  old: { id: 'server-1762140120.log', kind: 'server', text: LOG_TEXT, bytes: 1258291, cut: true },
  program: { id: 'program-1762146101.log', kind: 'program', text: PROG_TEXT, bytes: 6184, cut: false }
};

async function main() {
  let name = process.argv[2] || 'home';
  const out = process.argv[3] || (name + '.png');
  const width = Number(process.argv[4] || 1180);
  const height = Number(process.argv[5] || 760);
  let dark = false;
  let theme = process.env.AN_THEME || '';
  if (name.endsWith('-dark')) { dark = true; theme = 'dark'; name = name.slice(0, -5); }
  else if (name.endsWith('-silver')) { theme = 'silver'; name = name.slice(0, -7); }
  else if (name.endsWith('-corporate')) { theme = 'corporate'; name = name.slice(0, -10); }
  else if (name.endsWith('-neon')) { theme = 'neon'; name = name.slice(0, -5); }
  else if (name.endsWith('-glass')) { theme = 'glass'; name = name.slice(0, -6); }
  else if (name.endsWith('-pale')) { theme = 'light'; name = name.slice(0, -5); }
  if (theme && ['light', 'dark', 'silver', 'corporate', 'neon', 'glass'].indexOf(theme) < 0) { theme = 'light'; }
  const isSheet = name === 'sheet-program' || name === 'sheet-model' || name === 'sheet-logs' || name === 'sheet-mcp';
  const state = scene(isSheet ? 'chat' : name, dark, theme);
  CONFIG.theme = theme || 'light';
  CONFIG.dark_mode = (theme || 'light') === 'dark';

  // AN_FOLD=0 turns the two fold switches of the Chatting section off, so an image can be taken
  // of a panel that is written as it arrives. AN_HL=0 turns the colours of a code block off.
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
  // AN_NOCOLOUR=1 turns the colours the model requests off: the text of the four colours then
  // is in the colour of the text about them.
  if (process.env.AN_NOCOLOUR === '1') {
    state.chatting.colour_text = false;
    CONFIG.chat_colour_text = false;
  }
  // AN_PRESERVE=0 takes the choice to keep the thinking of the earlier turns away from the
  // model, so the switch of the menu can be checked off, and AN_NOPRESERVE=1 leaves the model
  // without the choice at all.
  if (process.env.AN_PRESERVE === '0') { state.thinking.preserve = false; }
  if (process.env.AN_NOPRESERVE === '1') {
    state.thinking.preserve_available = false;
    delete state.thinking.preserve;
  }
  // AN_PROGRESS=<0..100> puts the meter above the chat composer at that number, and AN_WHAT
  // reports what the program is working on: text, image or images.
  if (process.env.AN_PROGRESS !== undefined) {
    state.progress = {
      percent: Math.max(0, Math.min(100, Number(process.env.AN_PROGRESS) || 0)),
      active: true,
      what: state.progress.what
    };
  }
  if (process.env.AN_WHAT) { state.progress.what = process.env.AN_WHAT; }

  const puppeteer = require('puppeteer');
  let shell = process.env.AN_CHROME || '';
  try {
    if (!shell) { shell = execSync('find "' + (process.env.PUPPETEER_CACHE_DIR
      || process.env.HOME + '/.cache/puppeteer') + '" '
      + '-name chrome-headless-shell -type f | head -1').toString().trim(); }
  } catch (e) { shell = ''; }

  const browser = await puppeteer.launch({
    headless: process.env.AN_CHROME ? true : (shell ? 'shell' : true),
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu',
      '--font-render-hinting=none', '--force-color-profile=srgb']
  });
  // A fixture with a chat open has to say which chat it is, as the engine does, so the row of
  // that chat can be flagged in the side bar.
  if (!state.home) { state.chat_id = 'l-three'; }

  const view = await browser.newPage();
  await view.setViewport({ width: width, height: height, deviceScaleFactor: 2 });
  view.on('pageerror', function (e) { console.log('PAGEERROR', String(e)); });
  view.on('console', function (m) { if (m.type() === 'error') { console.log('CONSOLE', m.text()); } });

  await view.evaluateOnNewDocument(function (fetchMode, held, config, model, logs, logsProgram, one, pic, pic2, listsOf, mcpHeld, promptTable) {
    // The answer to a request for the pre-written prompts: the lines of the table when no id is
    // given, the text of one prompt when an id comes with the request.
    function promptsAnswer(id) {
      if (!id) {
        return { lines: promptTable.map(function (one) {
          return { id: one.id, name: one.name, summary: one.summary }; }) };
      }
      for (var i = 0; i < promptTable.length; i += 1) {
        if (promptTable[i].id === id) {
          return { id: id, name: promptTable[i].name, words: promptTable[i].words };
        }
      }
      return { refused: 'No prompt of that name' };
    }
    if (fetchMode) {
      // The page of the remote mode asks a small HTTP server. The records are the same, so the
      // same fixtures can be drawn.
      window.fetch = function (url, options) {
        // The switch of the theme. The word of the new theme is taken out of the body of the
        // call, for the state of the shot holds the theme as the program holds it.
        if (String(url).indexOf('/api/theme') === 0) {
          try {
            var asked = JSON.parse((options && options.body) || '{}');
            if (asked.theme) { held.theme = asked.theme; config.theme = asked.theme; }
          } catch (e) { /* words that are no words change nothing */ }
          return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
        }
        if (String(url).indexOf('/api/state') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(held))); } });
        }
        // The settings panel. The page of the remote mode reads the same three answers the
        // window of the program reads through its bridge.
        if (String(url).indexOf('/api/config') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(config))); } });
        }
        // The pre-written prompts of the Program tab: the lines of the table without an id, the
        // text of one prompt with one.
        if (String(url).indexOf('/api/prompts') === 0) {
          var at = String(url).indexOf('id=');
          var asked = at < 0 ? '' : decodeURIComponent(String(url).slice(at + 3));
          return Promise.resolve({ json: function () {
            return Promise.resolve(promptsAnswer(asked)); } });
        }
        if (String(url).indexOf('/api/repositories') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(held.repositories.map(function (r) {
              return { id: r.id, name: r.name }; })); } });
        }
        if (String(url).indexOf('/api/lists') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(listsOf))); } });
        }
        if (String(url).indexOf('/api/model_settings') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(model))); } });
        }
        if (String(url).indexOf('/api/preview_command') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve('llama-server -m model.gguf --jinja -c 8192 '
              + '-ngl 999 --fit on --fit-target 400'); } });
        }
        if (String(url).indexOf('/api/mcp') === 0) {
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(mcpHeld))); } });
        }
        if (String(url).indexOf('/api/logs') === 0) {
          // The type of the log is in the address the page asked with.
          var madeList = String(url).indexOf('kind=program') < 0 ? logs : logsProgram;
          return Promise.resolve({ json: function () {
            return Promise.resolve(JSON.parse(JSON.stringify(madeList))); } });
        }
        if (String(url).indexOf('/api/log') === 0) {
          // The older log of the record was longer than what is read.
          return Promise.resolve({ json: function () {
            if (String(url).indexOf('kind=program') >= 0) { return one.program; }
            return String(url).indexOf('1762140120') < 0 ? one.now : one.old; } });
        }
        if (String(url).indexOf('/api/attachment') === 0) {
          // One attachment record, as the small server of the remote mode pointers it to the
          // block that requests its file.
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
          if (command === 'action' && String((args && args.path) || '') === 'lists') {
            return Promise.resolve(JSON.parse(JSON.stringify(listsOf)));
          }
          // The switch of the theme, which the page of the window makes through the envelope of
          // the calls of the stylesheet.
          if (command === 'action' && String((args && args.path) || '') === 'theme') {
            var made = (args && args.body && args.body.theme) || 'light';
            held.theme = made; config.theme = made;
            return Promise.resolve({ ok: true });
          }
          if (command === 'get_model_settings') {
            return Promise.resolve(JSON.parse(JSON.stringify(model)));
          }
          if (command === 'preview_command') {
            return Promise.resolve('llama-server -m model.gguf --jinja -c 8192 '
              + '-ngl 999 --fit on --fit-target 400');
          }
          if (command === 'get_mcp') {
            return Promise.resolve(JSON.parse(JSON.stringify(mcpHeld)));
          }
          if (command === 'get_prompts') {
            return Promise.resolve(promptsAnswer(String((args && args.id) || '')));
          }
          if (command === 'get_logs') {
            // The type of the log is in the request.
            var listed = (args && args.kind) === 'program' ? logsProgram : logs;
            return Promise.resolve(JSON.parse(JSON.stringify(listed)));
          }
          if (command === 'get_log') {
            if (args && args.kind === 'program') { return Promise.resolve(one.program); }
            return Promise.resolve(String((args && args.id) || '').indexOf('1762140120') < 0
              ? one.now : one.old);
          }
          if (command === 'get_attachment') {
            // One attachment record, as the window pointers it to the block that requests its
            // file.
            return Promise.resolve({ name: 'memory-map.png', mime: 'image/png',
              data: String((args && args.id) || '').indexOf('tall') < 0 ? pic : pic2,
              text: '' });
          }
          return Promise.resolve({ ok: true });
        }
      }
    };
  }, useFetch, state, CONFIG, MODEL, LOGS, LOGS_PROGRAM, LOG_ONE, ATT_PIC, ATT_TALL, lists(state), MCP_HELD, PROMPT_LINES);

  await view.goto(process.env.AN_PAGE_URL || ('file://' + path.resolve(page)), {
    waitUntil: 'networkidle0'
  });
  // The page face asks for the program mark at "/autumn-natter.svg", which the served page
  // answers. Under file:// nothing answers an address from the root of the disk, so the mark is
  // pointed at the file of the asset in this repository.
  if (process.env.AN_PAGE) {
    await view.evaluate(function () {
      var marks = document.querySelectorAll('img.mark[src^="/"]');
      for (var i = 0; i < marks.length; i++) {
        marks[i].src = '../assets/icons/autumn-natter.svg';
      }
    });
    await new Promise(function (r) { setTimeout(r, 150); });
  }
  await new Promise(function (r) { setTimeout(r, 700); });
  // AN_PASTE=1 hangs an image and a text file on the chat composer the way Ctrl+V would, so the
  // blocks over the text of the box can be checked.
  if (process.env.AN_PASTE === '1') {
    await view.evaluate(function (picUrl) {
      var bin = atob(picUrl.split(',')[1]);
      var bytes = new Uint8Array(bin.length);
      for (var n = 0; n < bin.length; n++) { bytes[n] = bin.charCodeAt(n); }
      var transfer = new DataTransfer();
      transfer.items.add(new File([bytes], 'dropped-photo.png', { type: 'image/png' }));
      transfer.items.add(new File(['the pasted text of a note'], 'note.txt',
        { type: 'text/plain' }));
      var draft = document.getElementById('draft');
      draft.dispatchEvent(new ClipboardEvent('paste', {
        clipboardData: transfer, bubbles: true, cancelable: true
      }));
    }, ATT_PIC);
    await new Promise(function (r) { setTimeout(r, 400); });
  }

  // AN_DROP=1 lets an image and a text file go over the chat composer, the way a drag from the
  // file manager of the computer would. The window took these from the page before, so this is
  // the appearance of the path that was shut.
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
    // The stylesheet is taller than the window, so bring a field of it into the image: the last
    // field of the Chatting section, or the field named by AN_FIELD, which is how the
    // Compaction section is checked.
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
  // The Logs tab: the list, the text of the run, and the log itself. The stylesheet is put back
  // at the top of itself, for the log remains near the header of it.
  if (name === 'sheet-mcp') {
    await view.evaluate(function () {
      var b = document.getElementById('settingsbtn');
      if (b) { b.click(); }
      var t = document.getElementById('tabMcp');
      if (t) { t.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 700); });
    // A new server of the type the program runs, so the field of the code remains filled with
    // the example the program sent with it.
    if (!process.env.AN_NOKIND) {
      await view.evaluate(function () {
        var k = document.getElementById('mcKind');
        if (k) { k.value = 'internal'; k.dispatchEvent(new Event('change', { bubbles: true })); }
        var box = document.getElementById('mcCode');
        if (box) { box.scrollTop = 0; }
      });
      await new Promise(function (r) { setTimeout(r, 250); });
    }
  }
  if (name === 'mcp-menu') {
    await view.evaluate(function () {
      var b = document.getElementById('mcp');
      if (b) { b.click(); }
    });
    await new Promise(function (r) { setTimeout(r, 400); });
  }
  if (name === 'sheet-logs') {
    await view.evaluate(function () {
      var b = document.getElementById('settingsbtn');
      if (b) { b.click(); }
      var t = document.getElementById('tabLogs');
      if (t) { t.click(); }
      var body = document.querySelector('.sheet-body');
      if (body) { body.scrollTop = 0; }
    });
    await new Promise(function (r) { setTimeout(r, 900); });
  }
  // AN_LOGSEARCH=error writes text in the field over the log, so the appearance of a search can
  // be seen: the lines that hold them remain by themselves, the text is flagged, and the two
  // steps go from one to the next.
  if (process.env.AN_LOGSEARCH && name === 'sheet-logs') {
    await view.evaluate(function (words) {
      var field = document.getElementById('logSearch');
      field.value = words;
      field.dispatchEvent(new Event('input', { bubbles: true }));
    }, process.env.AN_LOGSEARCH);
    await new Promise(function (r) { setTimeout(r, 400); });
  }
  // AN_RATE means the pace of the log, for the button of Refresh is live on manual alone and
  // greyed on the two paces that read by themselves.
  if (process.env.AN_RATE && name === 'sheet-logs') {
    await view.evaluate(function (pace) {
      var sel = document.getElementById('logRate');
      sel.value = pace;
      sel.dispatchEvent(new Event('change', { bubbles: true }));
    }, process.env.AN_RATE);
    await new Promise(function (r) { setTimeout(r, 400); });
  }
  // AN_LOGKIND means the face of the Logs tab: `program` reads the log the program writes of
  // its own work, and `server` the output of llama.cpp, which is the face the tab had alone. It
  // is how the switch at the header of the tab and the text of the other log is checked.
  if (process.env.AN_LOGKIND && name === 'sheet-logs') {
    await view.evaluate(function (kind) {
      var b = document.getElementById(kind === 'program' ? 'logFaceProgram' : 'logFaceServer');
      if (b) { b.click(); }
    }, process.env.AN_LOGKIND);
    await new Promise(function (r) { setTimeout(r, 500); });
  }
  if (process.env.AN_SCROLL === 'top') {
    await view.evaluate(function () {
      var box = document.getElementById('thread');
      if (box) { box.scrollTop = 0; }
    });
    await new Promise(function (r) { setTimeout(r, 250); });
  }
  if (process.env.AN_SCROLL === 'sheet') {
    // Look at the bottom of the settings panel, where the last sections remain.
    await view.evaluate(function () {
      var box = document.querySelector('.sheet-body');
      if (box) { box.scrollTop = box.scrollHeight; }
    });
    await new Promise(function (r) { setTimeout(r, 250); });
  }
  // AN_SIDE=1 opens the side bar, which the program keeps closed until the user asks for it, so
  // its parts and the corner where it meets the top bar can be seen.
  if (process.env.AN_SIDE === '1') {
    await view.evaluate(function () {
      document.body.classList.add('sideopen');
    });
    await new Promise(function (r) { setTimeout(r, 400); });
  }
  // AN_PIXEL=1 draws the pixel-art form of the theme, which is a word of the state the program
  // sends, as the settings panel sends it for the user.
  // AN_HOVER=selector puts the mouse over one item, so an appearance that only shows when the
  // user hovers over it can be seen. It comes before AN_CLICK, so a click may land on an item
  // that is in a card under the pointer. AN_PICK=selector draws an image of each item that
  // matches, of its own, instead of the page.
  if (process.env.AN_HOVER) {
    const over = await view.$(process.env.AN_HOVER);
    if (over) {
      await over.hover();
      await new Promise(function (r) { setTimeout(r, 400); });
    }
  }
  // AN_CLICK=selector clicks one item, so a menu that opens on a click can be seen open. The
  // chat glides when a chapter is picked, so the wait is long enough for the glide to come to
  // rest.
  if (process.env.AN_CLICK) {
    const wants = String(process.env.AN_CLICK).split(';').filter(Boolean);
    for (const want of wants) {
      const hit = await view.$(want.trim());
      if (!hit) { continue; }
      await hit.click();
      await new Promise(function (r) { setTimeout(r, 700); });
    }
  }
  // AN_HOVER2=selector hovers a second item, after the click was created, so a menu that opens
  // a wing of its own can be seen with the panel out.
  if (process.env.AN_HOVER2) {
    const over2 = await view.$(process.env.AN_HOVER2);
    if (over2) {
      await over2.hover();
      await new Promise(function (r) { setTimeout(r, 400); });
    }
  }
  // AN_CURSOR=x,y puts the cursor at that share of the place the chat is in and lets it rest
  // there, so the aura of digits the cursor uncovers is at full and is seen in the image. The two
  // numbers are shares of the width and of the height, so the same request works at any size.
  if (process.env.AN_CURSOR) {
    const part = process.env.AN_CURSOR.split(',');
    const place = await view.evaluate(function () {
      const stage = document.getElementById('stage');
      if (!stage) { return null; }
      const box = stage.getBoundingClientRect();
      return { left: box.left, top: box.top, width: box.width, height: box.height };
    });
    if (place) {
      const wide = place.left + place.width * Number(part[0] || 0.4);
      const over = place.top + place.height * Number(part[1] || 0.45);
      await view.mouse.move(wide - 30, over - 20);
      await view.mouse.move(wide, over);
      // A second of the reveal, and a breath of the aura after it.
      await new Promise(function (r) { setTimeout(r, 2200); });
    }
  }
  // AN_JS=text runs the text in the page and prints what they answer, so a question about the
  // page can be asked without leaving the image.
  if (process.env.AN_JS) {
    const said = await view.evaluate(function (words) {
      try { return String(eval(words)); } catch (e) { return 'error: ' + String(e); }
    }, process.env.AN_JS);
    console.log('JS', said);
  }
  // AN_MEASURE=selector prints where the items that match remain, in the measure of the page,
  // so a layout can be checked without looking at pixels. It is read after a click or a hover
  // was requested, so the image a click leaves behind can be measured too.
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
