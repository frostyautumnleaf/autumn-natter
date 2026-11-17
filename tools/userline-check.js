// Check the line of the user's own messages: the dashes hard against the right
// edge of the chat, the card of a dash, the going to a message, and the setting
// that switches the line off. The page is the one the remote mode serves, and the
// chat is made by asking the engine, so the messages are real ones.
//
//   AN_URL=http://127.0.0.1:8798 node userline-check.js
//
// The live check needs a running program. It asks the engine for the messages,
// so the chat it looks at is a real one, which is what the check is for: the
// same drawing is looked at in both faces, with the drawing driven by hand, by
// userline-face-check.js. A running program with a model chosen is not part of
// the harness of the page, so with no AN_URL the check says so and leaves - a
// check that can only reach a program of the developer's own is not a fault of
// the page:
//
//   autumn-natter --remote --port 8798     # it prints the address it serves on
//   AN_URL=http://127.0.0.1:8798 node tools/userline-check.js
//
// Seven things are looked at:
//   * the line stands when a chat has messages of the user, and its nodes are
//     dashes (a stroke across the line, not a bead on it);
//   * one dash for one message of the user, and none for the sayings of the
//     model, the notes of the program or the runs of the tools;
//   * the card of a dash says which message it is, when it was sent, and shows a
//     little of what it says;
//   * a message with things hung on it says how many, pictures told from files;
//   * clicking a dash brings that message to the top of the view, and the chat
//     stops following the answer;
//   * the setting switched off takes the line away, and the chat takes the room
//     of it back;
//   * the line stands nearer the chat than the line of the summarisations when a
//     chat has both, and neither stands over the words.
const path = require('path');
const puppeteer = require('puppeteer');

const URL = process.env.AN_URL || '';
const shell = process.env.AN_SHELL_BIN || '';

async function ask(url, body) {
  const r = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body || {}),
  });
  return r.json();
}

const PNG = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAYAAACp8Z5+AAAAFklEQUGlIgPYrVmD'
  + 'g4cPIgDwDwJwAQVQEAQABgMAH64AfwMAAAAASUVORK5CYII=';

async function makeChat() {
  await ask(URL + '/api/new_chat', {});
  const bits = [
    { text: 'First question about `the thing` with a long tail of words to read', files: [] },
    { text: 'Second question', files: [{ name: 'notes.md', mime: 'text/markdown', text: 'a file of words' }] },
    { text: 'Third question with a picture hung on it', files: [{ name: 'cat.png', mime: 'image/png', data: PNG }] },
    { text: 'Fourth question', files: [] },
  ];
  for (const one of bits) {
    await ask(URL + '/api/send', { text: one.text, attachments: one.files });
    // Wait for the answer to come to its end before asking again.
    for (let n = 0; n < 120; n++) {
      const s = await (await fetch(URL + '/api/state')).json();
      if (!s.generating) { break; }
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  return (await (await fetch(URL + '/api/state')).json());
}

// The words said when no program is offered to the check. They are printed as
// the answer of the check, so a run of the harness shows the skip instead of a
// fault, and says what would take the skip away.
const SKIPPED = 'SKIP userline-check: AN_URL is not set, so no program is offered'
  + ' to it. This check asks a running program for its messages. Start the'
  + ' program with --remote and give its address: AN_URL=http://<address>:<port> '
  + 'node tools/userline-check.js. The program must have a model chosen, for the'
  + ' messages of the chat come from the engine.';

// Wait for the program to answer. A program that is asked for a model starts the
// server of the model first, so the first looks may be refused.
async function ready() {
  for (let n = 0; n < 40; n++) {
    try {
      const r = await fetch(URL + '/api/state');
      if (r.ok) { return await r.json(); }
    } catch (e) { /* the program has not begun to listen yet */ }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error('nothing answers at ' + URL + '. Start the program with'
    + ' --remote and give the address it prints as AN_URL');
}

async function main() {
  // The page of a face is not what this check reads: it reads the page a running
  // program serves. The pass of the harness over the browser face is therefore
  // left alone, so the live program is not asked twice for the same chat.
  if (process.env.AN_PAGE) {
    console.log('SKIP userline-check: this check reads the page a running program'
      + ' serves, not a page of one face. It runs on the pass with no AN_PAGE.');
    return 0;
  }
  if (!URL) { console.log(SKIPPED); return 0; }
  const open = await ready();
  const before = await makeChat();
  const mine = (before.messages || []).filter((m) => m.role === 'You');
  if (mine.length < 4) {
    const named = ((before.model || open.model) || {}).name || 'no model';
    throw new Error('the chat did not get four messages of the user; that program'
      + ' answers with the model "' + named + '", and this check needs a program'
      + ' with a model chosen so that the answers really come');
  }

  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  const faults = [];
  page.on('pageerror', (e) => faults.push(String(e)));
  page.on('requestfailed', (r) => { faults.push('request failed ' + r.url()); });
  page.on('response', (r) => { if (r.status() >= 400) { faults.push('HTTP ' + r.status() + ' ' + r.url()); } });
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().indexOf('favicon') < 0) { faults.push(m.text()); }
  });
  const trouble = [];
  const note = (what) => { trouble.push(what); console.log('  FAULT: ' + what); };

  await page.goto(URL + '/', { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('.turn', { timeout: 15000 });

  // 1 and 2: the line stands, and it is made of dashes, one for each message.
  const line = await page.evaluate(() => {
    const box = document.getElementById('userline');
    if (!box) { return { missing: 'no element of the user line' }; }
    const stage = document.getElementById('stage');
    const thread = document.getElementById('thread');
    const dashes = Array.from(box.querySelectorAll('.unode'));
    const shown = !box.classList.contains('hidden')
      && getComputedStyle(box).display !== 'none';
    const one = dashes[0] ? getComputedStyle(dashes[0]) : null;
    return {
      shown: shown,
      withclass: stage.classList.contains('withuserline'),
      count: dashes.length,
      dash: one ? { w: parseFloat(one.width), h: parseFloat(one.height), r: one.borderRadius } : null,
      pad: parseFloat(getComputedStyle(thread).paddingRight),
      right: parseFloat(getComputedStyle(box).right),
    };
  });
  if (line.missing) { note(line.missing); }
  if (!line.shown) { note('the line is not shown though the chat has messages of the user'); }
  if (!line.withclass) { note('the stage does not know the line stands'); }
  if (line.count !== mine.length) {
    note('the line holds ' + line.count + ' dashes for ' + mine.length + ' messages');
  }
  if (!line.dash || line.dash.h > 5 || line.dash.w < 8) {
    note('the node is not a dash: ' + JSON.stringify(line.dash));
  }
  if (!(line.pad > 20)) { note('the chat made no room for the line: padding ' + line.pad); }
  console.log('line:', JSON.stringify(line));

  // 3 and 4: the card of a dash. The third message carries a picture, and the
  // second a file, so their cards say so.
  for (const n of [0, 1, 2]) {
    const card = await page.evaluate((which) => {
      const dash = document.querySelectorAll('#userline .unode')[which];
      if (!dash) { return { missing: true }; }
      dash.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      dash.classList.add('hovered');
      const box = dash.querySelector('.card');
      const style = box ? getComputedStyle(box) : null;
      return {
        visible: !!style && style.visibility !== 'hidden' && parseFloat(style.opacity) > 0.5,
        text: box ? box.textContent.replace(/\s+/g, ' ').trim() : '',
      };
    }, n);
    if (card.missing) { note('dash ' + n + ' has no card'); continue; }
    // The card is shown by :hover, which a headless page cannot be put into, so
    // the words are read whether the card is open or not; the openness is
    // checked once, with the real mouse.
    if (card.text.indexOf('Your message ' + (n + 1)) !== 0) {
      note('dash ' + n + ' does not say which message it is: ' + card.text.slice(0, 60));
    }
    if (!/Sent /.test(card.text)) { note('dash ' + n + ' says nothing of when: ' + card.text); }
    if (n === 0 && card.text.indexOf('First question') < 0) {
      note('dash 0 shows no preview: ' + card.text.slice(0, 80));
    }
    if (n === 1 && card.text.indexOf('1 file') < 0) {
      note('dash 1 does not count the file: ' + card.text.slice(0, 90));
    }
    if (n === 2 && card.text.indexOf('1 picture') < 0) {
      note('dash 2 does not count the picture: ' + card.text.slice(0, 90));
    }
    console.log('card ' + n + ':', JSON.stringify(card.text).slice(0, 120));
  }

  // The card is really opened by the mouse, and stands clear of the chat's edge.
  let hover = null;
  for (let tryn = 0; tryn < 5 && !(hover && hover.open); tryn++) {
    await page.mouse.move(4, 4);
    await new Promise((r) => setTimeout(r, 120));
    const opened = await page.evaluate(async () => {
      const dash = document.querySelectorAll('#userline .unode')[0];
      const r = dash.getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    });
    await page.mouse.move(opened.x, opened.y, { steps: 4 });
    await new Promise((r) => setTimeout(r, 300));
    hover = await page.evaluate((pt) => {
      const dash = document.querySelectorAll('#userline .unode')[0];
      const box = dash.querySelector('.card');
      const style = getComputedStyle(box);
      const here = box.getBoundingClientRect();
      const stage = document.getElementById('stage').getBoundingClientRect();
      const col = document.getElementById('col').getBoundingClientRect();
      return {
        open: style.visibility !== 'hidden' && parseFloat(style.opacity) > 0.5,
        inside: here.left >= stage.left - 1 && here.right <= stage.right + 1,
        overWords: here.left < col.right && here.right > col.left,
        at: [Math.round(pt.x), Math.round(pt.y)],
        col: Math.round(col.right), card: [Math.round(here.left), Math.round(here.right)],
      };
    }, opened);
  }
  void 0;
  if (!hover.open) { note('the card does not open under the mouse'); }
  if (!hover.inside) { note('the card hangs out of the stage: ' + JSON.stringify(hover.card)); }
  console.log('hover:', JSON.stringify(hover));

  // 5: clicking a dash takes the chat to that message. The chat is scrolled to
  // the end first, so the going is seen.
  const moved = await page.evaluate(() => {
    const thread = document.getElementById('thread');
    thread.scrollTop = thread.scrollHeight;
    const dash = document.querySelectorAll('#userline .unode')[0];
    dash.click();
    return { top: thread.scrollTop };
  });
  await new Promise((r) => setTimeout(r, 700));
  const after = await page.evaluate(() => {
    const thread = document.getElementById('thread');
    const first = document.querySelector('#col .turn');
    const view = thread.getBoundingClientRect();
    const here = first ? first.getBoundingClientRect() : null;
    return {
      top: thread.scrollTop,
      near: here ? Math.round(here.top - view.top) : -1,
      stick: (typeof stick === 'boolean') ? stick : null,
    };
  });
  if (moved.top <= 1) { note('the chat was not at its end before the click'); }
  if (after.top > 40) { note('the click did not bring the message up: scrollTop ' + after.top); }
  if (after.near < 0) { note('the first message is not in view after the click'); }
  if (after.stick !== false) { note('the chat still follows the answer after the click'); }
  console.log('scroll:', JSON.stringify(moved), JSON.stringify(after));

  // 7: the line of the summarisations, when a chat has one too. It is drawn by
  // the same page from the state; here the state of a chat that was never
  // summarised is stood in for, so the two lines are looked at together.
  const both = await page.evaluate(() => {
    const stage = document.getElementById('stage');
    const box = document.getElementById('timeline');
    stage.classList.add('withtimeline');
    box.classList.remove('hidden');
    for (let n = 0; n < 2; n++) {
      const bead = document.createElement('button');
      bead.className = 'tnode';
      box.appendChild(bead);
    }
    const col = document.getElementById('col').getBoundingClientRect();
    const thread = document.getElementById('thread');
    const pad = parseFloat(getComputedStyle(thread).paddingRight);
    if (window.setChatWidth) { window.setChatWidth(); }
    const dashes = document.querySelector('#userline .unode').getBoundingClientRect();
    const beads = document.querySelector('#timeline .tnode').getBoundingClientRect();
    const wide = document.getElementById('col').getBoundingClientRect();
    return {
      pad: pad,
      dash: Math.round(dashes.left + dashes.width / 2),
      bead: Math.round(beads.left + beads.width / 2),
      colRight: Math.round(wide.right),
      colWidth: Math.round(wide.width),
    };
  });
  // Nearer the chat means a smaller x, for the chat stands at the left of both
  // lines. The dashes also stand a little way from the words rather than a long
  // way off, which is what makes the line read as a border of the chat.
  if (!(both.dash < both.bead)) {
    note('the dashes are not nearer the chat than the summarisations: ' + JSON.stringify(both));
  }
  if (both.dash - both.colRight > 60) {
    note('the dashes stand a long way from the words: ' + JSON.stringify(both));
  }
  if (both.bead <= both.colRight) { note('a line stands over the words: ' + JSON.stringify(both)); }
  if (both.dash <= both.colRight) { note('the dashes stand over the words: ' + JSON.stringify(both)); }
  console.log('both lines:', JSON.stringify(both));

  await page.evaluate(() => {
    const stage = document.getElementById('stage');
    stage.classList.remove('withtimeline');
    const box = document.getElementById('timeline');
    box.classList.add('hidden');
    box.innerHTML = '';
  });

  // 6: the setting. The sheet is driven as a user drives it: opened, the mark
  // moved, and the sheet saved. Switched off, the line goes and the chat takes
  // the room of it back.
  const moved2 = await page.evaluate(async () => {
    openSettings('program');
    const box = document.getElementById('fUserline');
    if (!box) { return { missing: true }; }
    // The sheet is filled from the engine, and the filling is a fetch: waiting
    // for the mark to stand is waiting for the sheet to know the settings.
    let waited = 0;
    while (!box.checked && waited < 6000) {
      await new Promise((r) => setTimeout(r, 100));
      waited += 100;
    }
    const was = box.checked;
    box.checked = false;
    box.dispatchEvent(new Event('change', { bubbles: true }));
    await saveSheet();
    return { was: was, waited: waited };
  });
  if (moved2.missing) {
    note('the settings hold no mark for the line');
  } else if (!moved2.was) {
    note('the mark for the line is not on to begin with');
  }
  let waited2 = 0;
  let seen = null;
  while (waited2 < 8000) {
    seen = await page.evaluate(() => {
      const box = document.getElementById('userline');
      const thread = document.getElementById('thread');
      return {
        setting: (typeof chatting !== 'undefined') ? chatting.userline : null,
        hidden: box.classList.contains('hidden'),
        shown: getComputedStyle(box).display !== 'none',
        withclass: document.getElementById('stage').classList.contains('withuserline'),
        pad: parseFloat(getComputedStyle(thread).paddingRight),
      };
    });
    if (seen.setting === false) { break; }
    await new Promise((r) => setTimeout(r, 200));
    waited2 += 200;
  }
  if (seen.pad > 10) {
    // The chat keeps its room only until the thread is laid out again, so the
    // width is asked for again before it is judged.
    await page.evaluate(() => { setChatWidth(); });
    seen.padAgain = await page.evaluate(() => parseFloat(
      getComputedStyle(document.getElementById('thread')).paddingRight));
  }
  const gone = await page.evaluate(async () => {
    const box = document.getElementById('userline');
    const thread = document.getElementById('thread');
    return {
      hidden: box.classList.contains('hidden'),
      shown: getComputedStyle(box).display !== 'none',
      withclass: document.getElementById('stage').classList.contains('withuserline'),
      pad: parseFloat(getComputedStyle(thread).paddingRight),
    };
  });
  if (seen.setting !== false) { note('the setting did not reach the page: ' + JSON.stringify(seen)); }
  if (seen.shown && !seen.hidden) { note('the line stands with the setting off'); }
  if (seen.withclass) { note('the stage still makes room for a line that is off'); }
  if (seen.padAgain !== undefined && seen.padAgain > 10) {
    note('the chat kept the room of a line that is off: ' + seen.padAgain);
  }
  console.log('setting off:', JSON.stringify(moved2), JSON.stringify(seen));

  // The mark is put back, so the check leaves the settings as it found them. The
  // sheet is opened again first: a save of a sheet that is closed writes nothing,
  // and the field must be moved after the sheet has read the settings of the
  // engine, or the filling puts the mark back where the engine has it.
  let back = { setting: false, shown: false };
  for (let n = 0; n < 3 && !(back.setting === true && back.shown); n++) {
    await page.evaluate(async () => {
      openSettings('program');
      const box = document.getElementById('fUserline');
      if (!box) { return; }
      // Wait for the filling of the sheet, which is a fetch: while it stands the
      // words of the fields are not yet the words of the engine.
      let waited = 0;
      while (box.checked && waited < 3000) {
        await new Promise((r) => setTimeout(r, 100));
        waited += 100;
      }
      if (!box.checked) {
        box.checked = true;
        box.dispatchEvent(new Event('change', { bubbles: true }));
        await saveSheet();
      }
    });
    await new Promise((r) => setTimeout(r, 400));
    back = await page.evaluate(() => {
      const box = document.getElementById('userline');
      return {
        setting: (typeof chatting !== 'undefined') ? chatting.userline : null,
        shown: !box.classList.contains('hidden') && getComputedStyle(box).display !== 'none',
      };
    });
  }
  if (!back.shown) { note('the line did not come back with the setting on'); }
  console.log('setting back:', JSON.stringify(back));

  if (faults.length) { note('the page faulted: ' + faults.slice(0, 3).join(' | ')); }
  await browser.close();
  console.log(trouble.length ? '\n' + trouble.length + ' faults' : '\nnothing amiss');
  return trouble.length;
}

main().then((n) => process.exit(n ? 1 : 0), (e) => { console.error(e); process.exit(2); });
