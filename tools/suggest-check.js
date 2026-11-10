// Check the buttons an answer ends with: the roads the suggestion tool offered,
// and the button that opens the window over the files that were shown.
//
// The page is loaded as it stands, with no engine under it. The state of the chat
// is written by hand, so the runs of the two tools are the ones the check puts
// there. Both bridges are stood in for, so one check runs on the window page and
// on the page of the remote mode, and the words a press sends are read off the
// call the page makes rather than off an engine.
//
//   node suggest-check.js
//   AN_PAGE=../web/page.html AN_FETCH=1 node suggest-check.js
//
// Eight things are looked at:
//   * the newest offer of a chat is the one that stands, and an older offer is
//     gone;
//   * a road is a button that shows its label and sends its prompt, and a press
//     sends those words over the road a message of the user goes over;
//   * the buttons are put by as soon as one is taken;
//   * nothing is offered while the model is still writing, and the offer of an
//     answer comes back when the writing is over;
//   * one answer that showed a file, offered a file and offered roads: the three
//     kinds stand in the one card, in that order, under the head of the card;
//   * the button of the show is named 'Show files' and stands above the other
//     buttons of the end, so no two of them are ever given the same place;
//   * the roads spread over the width of the chat and stand in one line while
//     their words fit in it;
//   * a line of roads that will not fit goes under, centred, and nothing runs
//     out of the chat;
//   * in a chat as narrow as the window can be made, a road whose words are
//     longer than the chat is cut by it and never runs out of it.
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE) : PAGE;

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });
  const faults = [];
  page.on('pageerror', (e) => faults.push(String(e)));
  page.on('console', (m) => {
    if (m.type() === 'error' && !/Fetch API cannot load|ERR_|Failed to fetch|favicon/.test(m.text())) {
      faults.push(m.text());
    }
  });
  await page.goto('file://' + REMOTE);
  await wait(400);

  const trouble = [];
  const note = (what) => { trouble.push(what); console.log('  FAULT: ' + what); };

  // The page, the state of the chat, and the calls the page makes. The words of
  // a message are short, for the check is about the buttons and not about the
  // words; only the labels of the roads are looked at by length.
  await page.evaluate(() => {
    window.__sent = [];
    var answer = function (name, body) {
      window.__sent.push({ name: name, body: body || {} });
      if (name === 'get_state') { return { rev: 1, unchanged: true }; }
      if (name === 'state') { return { rev: 1, unchanged: true }; }
      return { ok: true };
    };
    // The bridge of the window, and the small server of the remote mode. The
    // names of the calls match, so the same list of calls comes out of either.
    invoke = function (name, args) {
      var clean = String(name || '');
      if (clean === 'action' && args && args.path) {
        return Promise.resolve(answer(args.path, args.body || {}));
      }
      return Promise.resolve(answer(clean.replace(/^get_/, ''), args || {}));
    };
    window.fetch = function (where, init) {
      var clean = String(where).split('?')[0].replace(/^\/api\//, '');
      var body = {};
      try { body = JSON.parse((init && init.body) || '{}'); } catch (e) { body = {}; }
      var made = answer(clean, body);
      return Promise.resolve({
        ok: true,
        json: function () { return Promise.resolve(made); },
        text: function () { return Promise.resolve(JSON.stringify(made)); }
      });
    };
  });

  // The two answers of a chat that was offered roads twice: the first answer
  // showed a file and offered two roads, the second answer offered one.
  const said = (id, role, body, extra) => Object.assign({
    id: id, role: role, body: body, when: '09:0' + (id % 10),
    when_full: 'today at 09:0' + (id % 10), tool: '', mono: false, failed: false,
    reasoning: '', reasoning_ms: 0, attachments: []
  }, extra || {} );
  const tool = (id, name, words, extra) => said(id, 'Tool', words, Object.assign({
    tool_name: name, tool: words
  }, extra || {}));

  const FIRST = [
    said(1, 'You', 'Show me the plan.'),
    tool(2, 'display_files', '1 file is in the window over the chat: notes/plan.md', {
      displayed: [{ path: 'notes/plan.md', size: 2140 }]
    }),
    tool(3, 'offer_suggestions', '2 next steps are offered', {
      suggestions: [
        { label: 'Run the tests', prompt: 'Run the tests and tell me what failed.' },
        { label: 'Read the log' }
      ]
    }),
    said(4, 'Assistant', 'The plan is in the window over the chat.')
  ];
  const SECOND = [
    said(5, 'You', 'And now?'),
    tool(6, 'offer_suggestions', '1 next step is offered', {
      suggestions: [{ label: 'Show me the diff' }]
    }),
    said(7, 'Assistant', 'Say the word.')
  ];

  // 1: the newest offer stands and the older one is gone.
  const newest = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var rows = document.querySelectorAll('.suggests');
    var labels = [];
    for (var n = 0; n < rows.length; n++) {
      var buttons = rows[n].querySelectorAll('.suggest');
      for (var k = 0; k < buttons.length; k++) { labels.push(buttons[k].textContent.trim()); }
    }
    var lastTurn = document.querySelectorAll('#col .turn');
    var holder = rows.length ? rows[rows.length - 1].parentNode : null;
    return {
      rows: rows.length,
      labels: labels,
      inLastTurn: !!holder && holder.parentNode === lastTurn[lastTurn.length - 1],
      shownRows: document.querySelectorAll('.shown-again').length
    };
  }, FIRST.concat(SECOND));
  if (newest.rows !== 1) { note('the chat holds ' + newest.rows + ' rows of roads, not one'); }
  if (newest.labels.join('|') !== 'Show me the diff') {
    note('the roads that stand are "' + newest.labels.join('|') + '"');
  }
  if (!newest.inLastTurn) { note('the roads do not stand under the newest answer'); }
  if (newest.shownRows !== 1) {
    note('the chat holds ' + newest.shownRows + ' buttons of a show, not one');
  }
  console.log('newest:', JSON.stringify(newest));

  // 2: a press sends the prompt of the road and not its label, and the buttons
  // are put by. The answer that carries them is the newest of the chat, so the
  // roads of that answer stand.
  const pressed = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    window.__sent.length = 0;
    var buttons = document.querySelectorAll('.suggest');
    if (buttons.length !== 2) { return { missing: 'the answer offers ' + buttons.length + ' roads, not two' }; }
    var held = [];
    for (var n = 0; n < buttons.length; n++) { held.push(buttons[n].textContent.trim()); }
    buttons[0].click();
    var sent = [];
    for (var k = 0; k < window.__sent.length; k++) {
      if (window.__sent[k].name === 'send') { sent.push(window.__sent[k].body.text); }
    }
    var after = [];
    for (var j = 0; j < buttons.length; j++) { after.push(buttons[j].disabled); }
    return { held: held, sent: sent, allPutBy: after.indexOf(false) === -1 };
  }, FIRST);
  if (pressed.held && pressed.held.join('|') !== 'Run the tests|Read the log') {
    note('the roads that stand are "' + pressed.held.join('|') + '"');
  }
  if (!pressed.sent || pressed.sent.join('|') !== 'Run the tests and tell me what failed.') {
    note('the press sent ' + JSON.stringify(pressed.sent));
  }
  if (pressed.allPutBy === false) { note('a road was left to be taken twice'); }
  console.log('pressed:', JSON.stringify(pressed));

  // 3: the offer of an answer that is not the newest is gone. The user spoke
  // again and the last answer offered nothing, so the roads of the answer before
  // it lead to a point the talk has left.
  const older = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    return { rows: document.querySelectorAll('.suggests').length,
      shown: document.querySelectorAll('.shown-again').length };
  }, FIRST.concat([
    said(5, 'You', 'And now?'),
    said(6, 'Assistant', 'Nothing to offer here.')
  ]));
  if (older.rows !== 0) { note('the roads of an older answer still stand: ' + older.rows); }
  console.log('older:', JSON.stringify(older));

  // 4: nothing stands while the model is writing, and the offer comes back when
  // the writing is over.
  const writing = await page.evaluate((all) => {
    var shown = function () { return document.querySelectorAll('.suggests').length; };
    cur = { chatting: { send_anim: false }, generating: true, home: false, live: 'Still at it',
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var whileWriting = shown();
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    drawThread(cur);
    return { whileWriting: whileWriting, afterWriting: shown() };
  }, FIRST.slice(0, 4).concat([
    said(5, 'You', 'And now?'),
    tool(6, 'offer_suggestions', '1 next step is offered', {
      suggestions: [{ label: 'Show me the diff' }]
    })
  ]));
  if (writing.whileWriting !== 0) {
    note('roads stood while the model was writing: ' + writing.whileWriting);
  }
  if (writing.afterWriting !== 1) {
    note('the offer did not come back when the writing was over: ' + writing.afterWriting);
  }
  console.log('writing:', JSON.stringify(writing));

  // 5: one answer that showed a file, offered a file and offered roads. The
  // button of the show is named 'Show files' and stands above the rest, and no
  // two of the three are given the same place.
  const three = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var end = document.querySelector('.answer-end');
    if (!end) { return { missing: 'the answer ends with nothing at all' }; }
    // The card carries one head over the parts, and the parts are the kinds.
    var kinds = [];
    var boxes = [];
    var headWords = '';
    for (var n = 0; n < end.children.length; n++) {
      if (end.children[n].classList.contains('ae-head')) {
        headWords = end.children[n].textContent.trim();
        continue;
      }
      kinds.push(end.children[n].className);
      var b = end.children[n].getBoundingClientRect();
      boxes.push({ top: Math.round(b.top), bottom: Math.round(b.bottom),
        left: Math.round(b.left), right: Math.round(b.right) });
    }
    var words = end.querySelector('.shown-again button');
    var overlapped = [];
    for (var a = 0; a < boxes.length; a++) {
      for (var c = a + 1; c < boxes.length; c++) {
        if (boxes[a].top < boxes[c].bottom && boxes[c].top < boxes[a].bottom) {
          overlapped.push(kinds[a] + ' and ' + kinds[c]);
        }
      }
    }
    return {
      kinds: kinds,
      words: words ? words.textContent.trim() : '',
      headFirst: end.firstElementChild
        && end.firstElementChild.classList.contains('ae-head'),
      headWords: headWords,
      shownFirst: kinds.length ? kinds[0] === 'shown-again' : false,
      overlapped: overlapped,
      boxes: boxes
    };
  }, [
    said(1, 'You', 'Show me the plan and give me the file.'),
    tool(2, 'display_files', '2 files are in the window over the chat', {
      displayed: [{ path: 'notes/plan.md', size: 2140 }, { path: 'notes/log.md', size: 900 }]
    }),
    tool(3, 'offer_files', '1 file is offered to the user', {
      downloads: [{ name: 'the plan', extension: 'md', kind: 'text',
        path: 'notes/plan.md', size: 2140 }]
    }),
    tool(4, 'offer_suggestions', '2 next steps are offered', {
      suggestions: [{ label: 'Run the tests' }, { label: 'Read the log' }]
    }),
    said(5, 'Assistant', 'The plan is in the window over the chat, and the file stands below.')
  ]);
  if (three.kinds && three.kinds.join('|') !== 'shown-again|gifts|suggests') {
    note('the answer ends with ' + JSON.stringify(three.kinds));
  }
  if (three.headFirst === false) { note('the card of the end has no head over its parts'); }
  if (three.headWords !== 'From this answer') {
    note('the head of the card says "' + three.headWords + '"');
  }
  if (three.words !== 'Show files') { note('the button of a show says "' + three.words + '"'); }
  if (three.overlapped && three.overlapped.length) {
    note('two blocks of the end stand over one another: ' + three.overlapped.join(', '));
  }
  if (three.shownFirst === false) { note('the button of a show does not stand at the top'); }
  console.log('three:', JSON.stringify(three));

  // 6: the roads spread over the width of the chat. Two short roads stand in one
  // line, and the line fills the width it was given.
  const spread = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var row = document.querySelector('.suggests');
    var buttons = row.querySelectorAll('.suggest');
    // The roads fill the room of the card, which is the row without its own
    // padding. The padding is taken away here, for the check is about the roads
    // and not about the room the card gives them.
    var outer = row.getBoundingClientRect();
    var pad = getComputedStyle(row);
    var rowBox = { left: outer.left + parseFloat(pad.paddingLeft || 0),
      right: outer.right - parseFloat(pad.paddingRight || 0),
      width: outer.width - parseFloat(pad.paddingLeft || 0)
        - parseFloat(pad.paddingRight || 0) };
    var tops = [];
    var sum = 0;
    var out = [];
    for (var n = 0; n < buttons.length; n++) {
      var b = buttons[n].getBoundingClientRect();
      tops.push(Math.round(b.top));
      sum += b.width;
      if (b.left < rowBox.left - 1 || b.right > rowBox.right + 1) { out.push(n); }
    }
    var one = true;
    for (var t = 1; t < tops.length; t++) {
      if (tops[t] !== tops[0]) { one = false; }
    }
    return {
      count: buttons.length,
      oneLine: one,
      filled: Math.abs((rowBox.width - sum) / rowBox.width),
      gap: Math.round(rowBox.width - sum),
      outside: out,
      width: Math.round(rowBox.width)
    };
  }, [
    said(1, 'You', 'What next?'),
    tool(2, 'offer_suggestions', '2 next steps are offered', {
      suggestions: [{ label: 'Run the tests' }, { label: 'Read the log' }]
    }),
    said(3, 'Assistant', 'Two roads below.')
  ]);
  if (spread.oneLine === false) { note('two short roads did not stand in one line'); }
  if (spread.filled > 0.03) {
    note('the roads filled ' + Math.round((1 - spread.filled) * 100) + ' per cent of the chat');
  }
  if (spread.outside && spread.outside.length) { note('a road ran out of the chat'); }
  console.log('spread:', JSON.stringify(spread));

  // 7: a line of roads whose words will not stand side by side goes under, and
  // each line is centred. Nothing runs out of the chat, and no road is narrower
  // than the words on it: a road cut by the width of the chat is the one road
  // that may fill it.
  const wrapped = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var row = document.querySelector('.suggests');
    var buttons = row.querySelectorAll('.suggest');
    var outer = row.getBoundingClientRect();
    var pad = getComputedStyle(row);
    var rowBox = { left: outer.left + parseFloat(pad.paddingLeft || 0),
      right: outer.right - parseFloat(pad.paddingRight || 0),
      width: outer.width - parseFloat(pad.paddingLeft || 0)
        - parseFloat(pad.paddingRight || 0) };
    var lines = {};
    var outside = [];
    var cut = [];
    for (var n = 0; n < buttons.length; n++) {
      var b = buttons[n].getBoundingClientRect();
      var key = Math.round(b.top);
      lines[key] = lines[key] || [];
      lines[key].push({ left: Math.round(b.left), right: Math.round(b.right),
        width: Math.round(b.width) });
      if (b.left < rowBox.left - 1 || b.right > rowBox.right + 1) { outside.push(n); }
      var word = buttons[n].querySelector('.said');
      if (word.scrollWidth > word.clientWidth + 1) { cut.push(n); }
    }
    var keys = Object.keys(lines);
    var offCentre = [];
    for (var k = 0; k < keys.length; k++) {
      var line = lines[keys[k]];
      var lo = line[0].left, hi = line[0].right;
      for (var j = 1; j < line.length; j++) {
        lo = Math.min(lo, line[j].left); hi = Math.max(hi, line[j].right);
      }
      offCentre.push(Math.round(Math.abs((lo - rowBox.left) - (rowBox.right - hi))));
    }
    return { lines: keys.length, outside: outside, cut: cut, offCentre: offCentre };
  }, [
    said(1, 'You', 'What next?'),
    tool(2, 'offer_suggestions', '3 next steps are offered', {
      suggestions: [
        { label: 'Write the parser of the plan and run its tests' },
        { label: 'Read the log of every run of the last hour' },
        { label: 'Put the whole of the work into one page' }
      ]
    }),
    said(3, 'Assistant', 'Three long roads below.')
  ]);
  if (wrapped.lines < 2) { note('three long roads did not go under one another'); }
  if (wrapped.outside && wrapped.outside.length) { note('a road ran out of the chat'); }
  if (wrapped.cut && wrapped.cut.length) { note('a road was cut though it had room'); }
  var worst = wrapped.offCentre ? Math.max.apply(null, wrapped.offCentre) : 0;
  if (worst > 2) { note('a line of roads is not centred, by ' + worst + ' pixels'); }
  console.log('wrapped:', JSON.stringify(wrapped));

  // 8: a chat that is narrow. The words of a road are longer than the chat is
  // wide, so the road is cut by the chat and not let run out of it: a button the
  // user cannot read whole is a button the user cannot press.
  await page.setViewport({ width: 420, height: 800 });
  await wait(200);
  const narrow = await page.evaluate((all) => {
    cur = { chatting: { send_anim: false }, generating: false, home: false,
      chat: { id: 'chat-one', title: 'A chat', home: false }, messages: all };
    readChatting(cur);
    drawThread(cur);
    var row = document.querySelector('.suggests');
    var outer = row.getBoundingClientRect();
    var pad = getComputedStyle(row);
    var rowBox = { left: outer.left + parseFloat(pad.paddingLeft || 0),
      right: outer.right - parseFloat(pad.paddingRight || 0),
      width: outer.width - parseFloat(pad.paddingLeft || 0)
        - parseFloat(pad.paddingRight || 0) };
    var buttons = row.querySelectorAll('.suggest');
    var outside = [];
    for (var n = 0; n < buttons.length; n++) {
      var b = buttons[n].getBoundingClientRect();
      if (b.left < rowBox.left - 1 || b.right > rowBox.right + 1) { outside.push(n); }
    }
    var shown = document.querySelector('.shown-again button');
    var shownBox = shown.getBoundingClientRect();
    return {
      count: buttons.length,
      rowWidth: Math.round(rowBox.width),
      outside: outside,
      cut: row.querySelectorAll('.said').length ? (function () {
        var cutAny = 0;
        var words = row.querySelectorAll('.said');
        for (var k = 0; k < words.length; k++) {
          if (words[k].scrollWidth > words[k].clientWidth + 1) { cutAny += 1; }
        }
        return cutAny;
      })() : 0,
      shownInside: shownBox.left >= rowBox.left - 1 && shownBox.right <= rowBox.right + 1
    };
  }, [
    said(1, 'You', 'What next?'),
    tool(2, 'display_files', '1 file is in the window over the chat: notes/plan.md', {
      displayed: [{ path: 'notes/a-file-of-the-chat-with-a-long-name.md', size: 2140 }]
    }),
    tool(3, 'offer_suggestions', '3 next steps are offered', {
      suggestions: [
        { label: 'Write the parser of the plan and run its whole suite of tests' },
        { label: 'Read the log of every run of the last hour and of the day before' },
        { label: 'Put the whole of the work into one page of words' }
      ]
    }),
    said(4, 'Assistant', 'Three long roads in a narrow chat.')
  ]);
  if (narrow.outside && narrow.outside.length) {
    note('a road ran out of a narrow chat: ' + narrow.outside.join(','));
  }
  if (narrow.shownInside === false) { note('the button of a show ran out of a narrow chat'); }
  if (!narrow.cut) {
    note('no road was cut in a narrow chat, so the words were let run out of it');
  }
  console.log('narrow:', JSON.stringify(narrow));
  await page.setViewport({ width: 1280, height: 900 });

  await browser.close();
  console.log(trouble.length ? 'FAIL ' + trouble.length + ' faults' : 'ok   the roads at the end of an answer');
  process.exit(trouble.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(1); });
