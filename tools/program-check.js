// The programmed message of the chat composer, checked in both pages of the program with no
// engine behind it.
//
// Nine items are checked, in each page:
// * the list that appears over the box names both commands and reports what the keys of the
//    list do;
// * Tab picks the command that remains out, and the word /program changes the form of the box:
//    the two blocks over the text, the fence about the box, and the pointer put in the field of
//    the text to search for;
// * the block over the field reports, word for word, what the field is for;
// * a message whose text to search for were left unwritten is not posted, and the block and the
//    field are flagged instead;
// * Enter with the text written posts queue_plan with the message and the text beside it, puts
//    the box back as it was, and takes the text to search for out of its field again;
// * Enter in the field of the text to search for puts the pointer in the message box, and sends
//    nothing, because a programmed message goes when an answer holds its text;
// * the waiting list keeps the programmed messages in a section of their own: the row reports
//    what it waits for, is flagged apart, and is not taken hold of, while an ordinary row keeps
//    its marker to take hold of; and the count of the header counts both types;
// * taking a programmed row out of the list posts its number, as any other row does.
//
// node program-check.js AN_PAGE=web/page.html AN_FETCH=1 node program-check.js
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

// The text the block over the field has to say, word for word, as the task asked for it.
const SAYING = 'Enter the text to match in the final response';

function stateOf(theme) {
  return {
    rev: 1, unchanged: false, theme: theme || 'light',
    model: { id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' },
    models: [{ id: 'm1', name: 'Qwen3.5-0.8B-Q6_K_XL', kind: 'GGUF' }],
    repository: { id: '', name: 'No repository' },
    repositories: [],
    chat: { id: 'c1', title: 'A chat with a rule written in it', planning: false, planned: false },
    chats: [{ id: 'c1', title: 'A chat with a rule written in it', repo: '', when: 'Today 09:04',
      active: true, preview: 'Write the rule.' }],
    // The commands of the chat composer, as the engine gives them: the word of the command and
    // what it is for.
    commands: [
      { name: '/plan', words: 'Plan the work before any of it is done.' },
      { name: '/program', words: 'Write a message that is sent when an answer holds the text you name.' },
    ],
    home: false, greeting: 'Still here, still ready.', overview: false,
    messages: [
      { id: '1', role: 'You', body: 'Write the rule.', reasoning: '', reasoning_ms: 0,
        attachments: [], tool: '', mono: false, failed: false, when: '09:04' },
      { id: '2', role: 'Assistant', body: 'The work is ALL DONE now.',
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
    // The waiting list. The programmed messages are in a list of their own, and each carries
    // the text it waits for.
    queue: null,
    generating: false, busy: false, code: true, sandbox: false,
    sandbox_available: true, sandbox_note: 'bubblewrap works.',
    status: 'ready', dark: false, data_dir: '/tmp/autumn-natter',
  };
}

// What is put into the page before it runs: the two bridges, and a book of the actions the page
// requests, so the check can read what was sent instead of watching for an answer that no
// engine would send.
function bridges(given) {
  window.__state = given;
  window.__posts = [];
  var note = function (name, body) { window.__posts.push({ name: name, body: body }); };
  window.fetch = function (url, opts) {
    var where = String(url);
    if (where.indexOf('/api/state') === 0) {
      return Promise.resolve({ json: function () { return Promise.resolve(window.__state); } });
    }
    if (where.indexOf('/api/') === 0) {
      var body = {};
      try { body = JSON.parse((opts && opts.body) || '{}'); } catch (e) { body = {}; }
      note(where.slice(5), body);
    }
    return Promise.resolve({ json: function () { return Promise.resolve({ ok: true }); } });
  };
  window.__TAURI__ = {
    core: {
      invoke: function (command, args) {
        if (command === 'get_state') { return Promise.resolve(window.__state); }
        if (command === 'get_config') { return Promise.resolve({}); }
        if (command === 'action') { note((args && args.path) || '', (args && args.body) || {}); }
        return Promise.resolve({ ok: true });
      }
    },
    event: { listen: function () { return Promise.resolve(function () {}); } }
  };
}

// Write text into the chat composer as a pointer writes it: the value, the place of the caret
// after them, and the word that shows the value changed.
function write(page, words) {
  return page.evaluate(function (text) {
    var box = document.getElementById('draft');
    box.focus();
    box.value = text;
    box.selectionStart = box.selectionEnd = box.value.length;
    box.dispatchEvent(new Event('input', { bubbles: true }));
  }, words);
}

// One stroke of a key, on the item that has the pointer.
function key(page, name, where) {
  return page.evaluate(function (keyName, on) {
    var node = on ? document.getElementById(on) : document.activeElement;
    node.dispatchEvent(new KeyboardEvent('keydown', {
      key: keyName, code: keyName, bubbles: true, cancelable: true,
    }));
    return { on: on || (document.activeElement && document.activeElement.id), programming: programming() };
  }, name, where || null);
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
    await page.setViewport({ width: 1280, height: 900 });
    const faults = [];
    page.on('pageerror', (e) => faults.push(String(e)));
    await page.evaluateOnNewDocument(bridges, stateOf('light'));
    await page.goto('file://' + one.file, { waitUntil: 'domcontentloaded' });
    await sleep(500);
    const notes = [];
    const note = (what) => { notes.push(what); console.log('  ERROR: ' + what); };

    // The list that appears over the box, and the word about the keys.
    await write(page, '/');
    await sleep(120);
    const popped = await page.evaluate(() => {
      const list = document.getElementById('commands');
      const rows = list.querySelectorAll('li[data-cmd]');
      const names = [];
      for (let at = 0; at < rows.length; at++) { names.push(rows[at].dataset.cmd); }
      const hint = list.querySelector('li.cmdhint');
      return {
        open: !list.classList.contains('hidden'),
        names: names,
        hint: hint ? hint.textContent : '',
        hintIsRow: hint ? hint.hasAttribute('data-cmd') : true,
      };
    });
    if (!popped.open) { note('the list of commands did not come up'); }
    if (popped.names.indexOf('/plan') < 0 || popped.names.indexOf('/program') < 0) {
      note('the list of commands does not name both of them: ' + popped.names.join(', '));
    }
    if (popped.hint.indexOf('Tab') < 0) {
      note('the list does not say that Tab chooses: ' + JSON.stringify(popped.hint));
    }
    if (popped.hintIsRow) { note('the word about the keys is pickable as a command is'); }

    // Tab picks the command that remains out, and the box changes form.
    await write(page, '/program');
    await sleep(120);
    await key(page, 'Tab');
    await sleep(200);
    const shaped = await page.evaluate(() => {
      const box = document.getElementById('box');
      const prog = document.getElementById('prog');
      const field = document.getElementById('progmatch');
      const draft = document.getElementById('draft');
      return {
        pending: pendingCommand,
        programming: programming(),
        chip: !document.getElementById('cmdchip').classList.contains('hidden'),
        hidden: prog.classList.contains('hidden'),
        saying: prog.querySelector('.prog-say') ? prog.querySelector('.prog-say').textContent : '',
        fenced: box.classList.contains('programming'),
        draftMarked: draft.classList.contains('programming'),
        focused: document.activeElement === field,
        placeholder: draft.placeholder,
        list: document.getElementById('commands').classList.contains('hidden'),
      };
    });
    if (shaped.pending !== '/program') {
      note('Tab did not pick the command of the list: ' + shaped.pending);
    }
    if (!shaped.programming || shaped.hidden) { note('the box of a programmed message is not was up'); }
    if (shaped.saying !== SAYING) {
      note('the block over the field does not say what it is for, word for word: ' + JSON.stringify(shaped.saying));
    }
    if (!shaped.fenced || !shaped.draftMarked) { note('the fence of the programmed box is not worn'); }
    if (!shaped.focused) { note('the pointer was not put in the field of the text to search for'); }
    if (shaped.placeholder.indexOf('answer') < 0) {
      note('the box does not say what the message is for: ' + shaped.placeholder);
    }
    if (!shaped.list) { note('the list of commands is left remaining after the pick'); }

    // A message whose text to search for were never written is not posted.
    await write(page, 'say it again');
    await sleep(120);
    await key(page, 'Enter', 'draft');
    await sleep(250);
    const refused = await page.evaluate(() => ({
      // The page requests the lists of the side bar on its own account, so only the requests
      // that would send a message say anything here.
      posts: window.__posts.filter(function (p) {
        return p.name === 'queue_plan' || p.name === 'queue_add' || p.name === 'send';
      }).length,
      names: window.__posts.map((p) => p.name),
      miss: document.getElementById('prog').classList.contains('miss')
        || document.getElementById('progmatch').classList.contains('miss'),
      held: document.getElementById('draft').value,
      still: programming(),
    }));
    if (refused.posts !== 0) {
      note('a programmed message with no text to search for was posted: ' + refused.names.join(', '));
    }
    if (!refused.miss) { note('the field was left unmarked when the text to search for were missing'); }
    if (refused.held !== 'say it again' || !refused.still) {
      note('the message was deleted instead of being left for the pointer to finish');
    }

    // Enter in the field of the text to search for asks for the next part of the message, which
    // is the message itself. Nothing is sent from the field: a programmed message goes when an
    // answer holds the words, and not when a marker is pressed.
    await page.evaluate(() => {
      const field = document.getElementById('progmatch');
      field.value = 'All done';
      field.dispatchEvent(new Event('input', { bubbles: true }));
      field.focus();
    });
    await sleep(120);
    await key(page, 'Enter', 'progmatch');
    await sleep(200);
    const stepped = await page.evaluate(() => ({
      on: document.activeElement && document.activeElement.id,
      programming: programming(),
      posts: window.__posts.filter(function (p) {
        return p.name === 'queue_plan' || p.name === 'queue_add' || p.name === 'send';
      }).length,
    }));
    if (stepped.on !== 'draft') {
      note('the field of the text to search for does not ask for the message on Enter: ' + stepped.on);
    }
    if (!stepped.programming) { note('the message stopped being a programmed one'); }
    if (stepped.posts !== 0) { note('a programmed message went out of the field of its own accord'); }
    // The pointer is now in the box, so the same stroke sends the message into the list.
    await key(page, 'Enter', 'draft');
    await sleep(300);
    const posted = await page.evaluate(() => {
      let asked = null;
      for (let at = 0; at < window.__posts.length; at++) {
        if (window.__posts[at].name === 'queue_plan') { asked = window.__posts[at].body; }
      }
      return {
        asked: asked,
        names: window.__posts.map((p) => p.name),
        empty: document.getElementById('draft').value === '',
        field: document.getElementById('progmatch').value,
        programming: programming(),
        fenced: document.getElementById('box').classList.contains('programming'),
        hidden: document.getElementById('prog').classList.contains('hidden'),
        miss: document.getElementById('prog').classList.contains('miss'),
      };
    });
    if (!posted.asked) {
      note('no queue_plan ask went out: ' + posted.names.join(', '));
    } else if (posted.asked.text !== 'say it again' || posted.asked.match !== 'All done') {
      note('the request carries the wrong text: ' + JSON.stringify(posted.asked));
    }
    if (!posted.empty || posted.programming || posted.fenced || !posted.hidden) {
      note('the box was not put back as it was: ' + JSON.stringify(posted));
    }
    if (posted.miss) { note('the marker of the missing text was left on the block'); }
    if (posted.field !== '') {
      note('the text to search for was left in the field after its message went into the list: '
        + JSON.stringify(posted.field));
    }
    await page.evaluate(() => { window.__posts.length = 0; });

    // The list over the box: two types of message in two sections.
    await page.evaluate(() => {
      window.__state.queue = {
        chat: 'c1',
        items: [{ id: 3, text: 'the plain one', files: [], matches: '' }],
        planned: [{ id: 4, text: 'the programmed one', files: [], matches: 'All done' }],
        held: false,
      };
    });
    await sleep(900);
    const listed = await page.evaluate(() => {
      if (typeof queueOpen !== 'undefined' && !queueOpen) { document.getElementById('queuehead').click(); }
      const rows = document.querySelectorAll('#queuelist .qrow');
      const seen = [];
      for (let at = 0; at < rows.length; at++) {
        const row = rows[at];
        const grip = row.querySelector('.qgrip');
        const wait = row.querySelector('.qwait');
        seen.push({
          planned: row.classList.contains('planned'),
          words: row.querySelector('.qtext').textContent,
          waits: wait ? wait.textContent : '',
          grip: !!grip,
          draggable: row.draggable,
        });
      }
      const secs = [];
      const marks = document.querySelectorAll('#queuelist .qsec');
      for (let at = 0; at < marks.length; at++) { secs.push(marks[at].textContent); }
      return {
        shown: !document.getElementById('queue').classList.contains('hidden'),
        count: document.getElementById('queuecount').textContent,
        sections: secs,
        rows: seen,
      };
    });
    if (!listed.shown) { note('the list of messages waiting is not shown for one message of each type'); }
    if (listed.count !== '2') {
      note('the header does not count both types: ' + listed.count);
    }
    if (listed.sections.length !== 2) {
      note('the two types are not named apart: ' + listed.sections.join(' / '));
    }
    const plannedRow = listed.rows.filter((row) => row.planned)[0];
    const plainRow = listed.rows.filter((row) => !row.planned)[0];
    if (!plannedRow || !plainRow) {
      note('one of the two types of row is not drawn: ' + JSON.stringify(listed.rows));
    } else {
      if (plannedRow.words !== 'the programmed one' || plannedRow.waits !== 'All done') {
        note('the programmed row does not say what it waits for: ' + JSON.stringify(plannedRow));
      }
      if (plannedRow.grip || plannedRow.draggable) {
        note('a programmed row may be taken hold of: ' + JSON.stringify(plannedRow));
      }
      if (!plainRow.grip || !plainRow.draggable || plainRow.words !== 'the plain one') {
        note('an ordinary row lost its marker to take hold of: ' + JSON.stringify(plainRow));
      }
      if (listed.rows[0] !== plannedRow) {
        note('the programmed messages are not over the others in the list');
      }
    }

    // Taking a programmed row out requests its number, as any other row does.
    await page.evaluate(() => {
      const row = document.querySelector('#queuelist .qrow.planned');
      const marks = row.querySelectorAll('.qbtn');
      marks[marks.length - 1].click();
    });
    await sleep(250);
    const removed = await page.evaluate(() => window.__posts.slice());
    const askedOut = removed.filter((one) => one.name === 'queue_remove');
    if (askedOut.length !== 1 || askedOut[0].body.id !== 4) {
      note('taking the programmed row out did not request its number: ' + JSON.stringify(removed));
    }

    // Only one type in the list is what the list always was: no word over it.
    await page.evaluate(() => {
      window.__state.queue = {
        chat: 'c1',
        items: [{ id: 5, text: 'only a plain one', files: [], matches: '' }],
        planned: [],
        held: false,
      };
    });
    await sleep(900);
    const single = await page.evaluate(() => ({
      count: document.getElementById('queuecount').textContent,
      sections: document.querySelectorAll('#queuelist .qsec').length,
      rows: document.querySelectorAll('#queuelist .qrow').length,
    }));
    if (single.count !== '1' || single.rows !== 1) {
      note('a list of one plain message is not drawn as it was: ' + JSON.stringify(single));
    }
    if (single.sections !== 0) {
      note('a list of one type is named in sections: ' + single.sections.join(' / '));
    }

    if (faults.length) { note('page faults: ' + faults.join(' | ')); }
    await page.close();
    if (!notes.length) { console.log('  ok ' + one.name); } else { trouble += notes.length; }
  }
  await browser.close();
  if (trouble) { throw new Error(trouble + ' fault(s) across ' + PAGES.length + ' faces'); }
  console.log('a programmed message is written in the box, waits in the list, and goes when an answer reports its text');
}

main().catch((e) => { console.error(e); process.exit(1); });
