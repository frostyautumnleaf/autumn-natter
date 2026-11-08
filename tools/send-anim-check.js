// Check the journey of a sent message: the copy out of the box, the squeeze,
// the curve up the chat, and the message unfolding into its place.
//
// The page is loaded as it stands, with no engine under it. The state of the
// chat is written by hand, so the message that comes in after a send is the one
// the check puts there.
//
//   node send-anim-check.js                     the desktop face
//   AN_PAGE=web/page.html AN_FETCH=1 node send-anim-check.js    the browser face
//
// Six things are looked at, and a seventh besides:
//   * a send makes a copy of the words over the box, and the box is empty;
//   * the copy squeezes itself small while it waits for its message;
//   * the message that comes in stands in the chat but is not seen, so the
//     words are read once and not twice over;
//   * the copy is carried up the chat, and it comes to rest on the bubble of
//     that message;
//   * the copy goes, the message is let through, and nothing is left over;
//   * with the setting off no copy is made at all, and a message of another
//     chat is never taken for the one the copy is looking for;
//   * a message that is put by for later while the model is writing makes no
//     copy at all.
const fs = require('fs');
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
  await page.setViewport({ width: 1440, height: 900 });
  const faults = [];
  page.on('pageerror', (e) => faults.push(String(e)));
  page.on('console', (m) => {
    // The ask of a page that stands without an engine cannot go anywhere: the
    // page takes the failure in its stride, and the browser says so.
    if (m.type() === 'error' && !/Fetch API cannot load|ERR_|Failed to fetch|favicon/.test(m.text())) {
      faults.push(m.text());
    }
  });
  await page.goto('file://' + REMOTE);
  await wait(400);

  const trouble = [];
  const note = (what) => { trouble.push(what); console.log('  FAULT: ' + what); };

  // The chat before the send: one message of the user and one answer, so the
  // thread is not empty and the box has its usual room.
  await page.evaluate(() => {
    cur = { chatting: { send_anim: true }, chat: { id: 'chat-one', title: 'A chat',
      home: false }, messages: [
        { id: 'm1', role: 'You', body: 'What stands at the beginning?', when: '09:00',
          when_full: 'today at 09:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 },
        { id: 'm2', role: 'Assistant', body: 'A line, and then another.', when: '09:01',
          when_full: 'today at 09:01', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 }
      ] };
    readChatting(cur);
    drawThread(cur);
    el.draft.value = 'And what stands at the end?';
    grow();
  });
  await wait(150);

  // 1: the send. The copy is taken while the box still holds the words, so it
  // stands over the place they were written on, and the box is emptied.
  const made = await page.evaluate(() => {
    send();
    const card = document.querySelector('.flycard');
    if (!card) { return { missing: 'the send made no copy of the words' }; }
    const box = el.draft.getBoundingClientRect();
    const r = card.getBoundingClientRect();
    const style = getComputedStyle(card);
    return {
      emptied: el.draft.value === '',
      over: Math.abs(r.left - box.left) < 4 && Math.abs(r.top - box.top) < 4,
      words: card.textContent,
      still: style.pointerEvents,
      one: document.querySelectorAll('.flycard').length
    };
  });
  if (made.missing) { note(made.missing); await browser.close(); process.exit(1); }
  if (!made.emptied) { note('the box kept its words after the send'); }
  if (!made.over) { note('the copy does not stand over the box'); }
  if (made.words !== 'And what stands at the end?') { note('the copy holds other words'); }
  if (made.still !== 'none') { note('the copy catches the hand: pointer-events ' + made.still); }
  if (made.one !== 1) { note('there are ' + made.one + ' copies over the page'); }
  console.log('made:', JSON.stringify(made));

  // 2: the squeeze, while the copy waits for its message.
  await wait(240);
  const squeezed = await page.evaluate(() => {
    const card = document.querySelector('.flycard');
    if (!card) { return { missing: 'the copy went before its message came' }; }
    const m = new DOMMatrixReadOnly(getComputedStyle(card).transform);
    const r = card.getBoundingClientRect();
    return { sx: m.a, top: r.top, height: r.height, landing: document.querySelectorAll('.landing').length };
  });
  if (squeezed.missing) { note(squeezed.missing); }
  else if (!(squeezed.sx > 0.2 && squeezed.sx < 0.75)) {
    note('the copy did not squeeze itself small: scale ' + squeezed.sx);
  }
  if (squeezed.landing) { note('a message was held back before one came in'); }
  console.log('squeezed:', JSON.stringify(squeezed));

  // 3: the message comes in. It stands in the chat, and it is not seen.
  await page.evaluate(() => {
    cur.messages.push({ id: 'm3', role: 'You', body: 'And what stands at the end?',
      when: '09:02', when_full: 'today at 09:02', tool: '', mono: false, failed: false,
      reasoning: '', reasoning_ms: 0 });
    drawThread(cur);
  });
  const held = await page.evaluate(() => {
    const turn = document.querySelector('.turn[data-mid="m3"]');
    if (!turn) { return { missing: 'the message that came in is not in the chat' }; }
    const bubble = turn.querySelector('.bubble');
    return {
      landing: turn.classList.contains('landing'),
      seen: getComputedStyle(turn).visibility,
      room: bubble ? bubble.getBoundingClientRect().height > 10 : false,
      flying: !!document.querySelector('.flycard')
    };
  });
  if (held.missing) { note(held.missing); }
  else {
    if (!held.landing) { note('the message the copy is on its way to was not held back'); }
    if (held.seen !== 'hidden') { note('the message is seen while the copy is coming'); }
    if (!held.room) { note('the message that is held back took no room in the chat'); }
    if (!held.flying) { note('the copy went away before it reached its message'); }
  }
  console.log('held:', JSON.stringify(held));

  // 4: the copy is on its way up the chat. Halfway through the journey it
  // stands above the box and under its message.
  await wait(200);
  const midway = await page.evaluate((was) => {
    const card = document.querySelector('.flycard');
    const turn = document.querySelector('.turn[data-mid="m3"]');
    if (!card) { return { missing: 'the copy went before it reached its message' }; }
    const c = card.getBoundingClientRect();
    const b = turn.querySelector('.bubble').getBoundingClientRect();
    const box = el.draft.getBoundingClientRect();
    const m = new DOMMatrixReadOnly(getComputedStyle(card).transform);
    return { above: c.top < box.top - 4, rising: c.top > b.top - 40,
      growing: m.a > was + 0.02, sx: m.a,
      top: Math.round(c.top), bubbleTop: Math.round(b.top), boxTop: Math.round(box.top) };
  }, squeezed.sx || 0.4);
  if (midway.missing) { note(midway.missing); }
  else {
    if (!midway.above) { note('the copy did not leave the box: top ' + midway.top); }
    if (!midway.growing) { note('the copy did not grow as it came: scale ' + midway.sx); }
  }
  console.log('midway:', JSON.stringify(midway));

  // 5: the copy is gone, the message is let through, and nothing is left over.
  await wait(700);
  const landed = await page.evaluate(() => {
    const turn = document.querySelector('.turn[data-mid="m3"]');
    return {
      cards: document.querySelectorAll('.flycard').length,
      landing: turn ? turn.classList.contains('landing') : null,
      landed: turn ? turn.classList.contains('landed') : null,
      seen: turn ? getComputedStyle(turn).visibility : null,
      words: turn ? turn.querySelector('.bubble').textContent : '',
      flying: !!window.fly
    };
  });
  if (landed.cards) { note('the copy is still over the page after it landed'); }
  if (landed.landing) { note('the message is still held back after the copy landed'); }
  if (!landed.landed) { note('the message did not unfold into its place'); }
  if (landed.seen !== 'visible') { note('the message is not seen after it landed'); }
  if (landed.words !== 'And what stands at the end?') { note('the message lost its words'); }
  if (landed.flying) { note('the page still thinks a copy is on its way'); }
  console.log('landed:', JSON.stringify(landed));

  // 6a: with the setting off no copy is made, and the message stands at once.
  const off = await page.evaluate(() => {
    chatting.send_anim = false;
    startFly('Words with no journey');
    const card = document.querySelector('.flycard');
    return { card: !!card, fly: !!window.fly };
  });
  if (off.card) { note('a copy was made though the setting stands off'); }
  if (off.fly) { note('the page is still waiting for a journey that was not made'); }
  console.log('off:', JSON.stringify(off));

  // 6b: a message of another chat is never taken for the one the copy looks
  // for. The copy is made here, the chat is turned over, and the copy is given
  // up rather than flown into the words of a chat it was not made for.
  const other = await page.evaluate(() => {
    chatting.send_anim = true;
    el.draft.value = 'Words for this chat only';
    grow();
    startFly('Words for this chat only');
    cur = { chatting: { send_anim: true }, chat: { id: 'chat-two', title: 'Another chat',
      home: false }, messages: [
        { id: 'z1', role: 'You', body: 'Quite another thing', when: '10:00',
          when_full: 'today at 10:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 }
      ] };
    drawThread(cur);
    return { taken: document.querySelectorAll('.landing').length, cards: document.querySelectorAll('.flycard').length };
  });
  if (other.taken) { note('a message of another chat was held back for this copy'); }
  console.log('other:', JSON.stringify(other));

  await wait(700);
  const after = await page.evaluate(() => ({
    cards: document.querySelectorAll('.flycard').length,
    landing: document.querySelectorAll('.landing').length,
    flying: !!window.fly,
    seen: getComputedStyle(document.querySelector('.turn[data-mid="z1"]')).visibility
  }));
  if (after.cards) { note('the copy of the first chat was left over the page'); }
  if (after.landing) { note('a message is still held back after the copy was given up'); }
  if (after.seen !== 'visible') { note('the message of the other chat is not seen'); }
  console.log('after:', JSON.stringify(after));

  // 7: a message that is put by for later while the model is writing is not
  // carried anywhere: it stands in the list over the box.
  const queued = await page.evaluate(() => {
    cur = { chatting: { send_anim: true }, generating: true, chat: { id: 'chat-two',
      title: 'Another chat', home: false }, messages: [
        { id: 'z1', role: 'You', body: 'Quite another thing', when: '10:00',
          when_full: 'today at 10:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 } ] };
    queue.length = 0;
    el.draft.value = 'Words put by for later';
    grow();
    send();
    return { cards: document.querySelectorAll('.flycard').length,
      fly: !!window.fly, inList: queue.length, emptied: el.draft.value === '' };
  });
  if (queued.cards) { note('a message put by for later made a copy over the page'); }
  if (queued.fly) { note('a message put by for later set a copy on its way'); }
  if (queued.inList !== 1) { note('the message put by did not stand in the list'); }
  if (!queued.emptied) { note('the box kept its words though the message was put by'); }
  console.log('queued:', JSON.stringify(queued));

  await browser.close();
  console.log(trouble.length ? 'FAIL ' + trouble.length + ' faults' : 'ok   the journey of a sent message');
  process.exit(trouble.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(1); });
