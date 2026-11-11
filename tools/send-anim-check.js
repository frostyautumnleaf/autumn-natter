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
// Twelve things are looked at:
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
//     copy at all;
//   * the copy is as wide as its own words, and as wide as the bubble it lands
//     on, so the message does not shrink when the copy goes away;
//   * the copy goes up the chat and into the corner the messages of the user
//     stand in, and not away from it;
//   * a copy that cannot read the words it carries still takes the message that
//     came in;
//   * a page that is not seen gets no journey at all;
//   * a page that is given no steps still lets its message through when the
//     bound of the journey comes, so no message is held out of sight by a
//     journey that stopped.
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

  // 8 and 9: the copy is as wide as its own words, under the cap a bubble stands
  // under, and it goes up the chat and into the corner the messages of the user
  // stand in. A copy as wide as the chat read as a message that shrank when it
  // landed, and a copy that drifted the wrong way read as a message going the
  // wrong road.
  const way = await page.evaluate(() => {
    chatting.send_anim = true;
    cur = { chatting: { send_anim: true }, chat: { id: 'chat-three', title: 'Third chat',
      home: false }, messages: [
        { id: 'k1', role: 'You', body: 'How wide is the copy of these words?', when: '11:00',
          when_full: 'today at 11:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 },
        { id: 'k2', role: 'Assistant', body: 'As wide as its words, and no wider.',
          when: '11:01', when_full: 'today at 11:01', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 } ] };
    readChatting(cur);
    drawThread(cur);
    var bubble = document.querySelector('.turn[data-mid="k1"] .bubble');
    var b = bubble.getBoundingClientRect();
    el.draft.value = 'How wide is the copy of these words?';
    grow();
    send();
    var card = document.querySelector('.flycard');
    if (!card) { return { missing: 'the send made no copy' }; }
    var c = card.getBoundingClientRect();
    var box = el.draft.getBoundingClientRect();
    return {
      words: Math.round(c.width), bubble: Math.round(b.width),
      atRight: Math.round(b.right - c.right), boxRight: Math.round(box.right - c.right),
      room: Math.round(b.right), startRight: Math.round(c.right), startTop: Math.round(c.top)
    };
  });
  if (way.missing) { note(way.missing); }
  else {
    if (Math.abs(way.words - way.bubble) > 2) {
      note('the copy is ' + way.words + ' wide and the message it is going to is ' + way.bubble);
    }
  }
  console.log('size:', JSON.stringify(way));

  await wait(60);
  // The message comes in, the copy takes it, and the journey is on its way.
  await page.evaluate(() => {
    cur.messages.push({ id: 'k3', role: 'You', body: 'How wide is the copy of these words?',
      when: '11:02', when_full: 'today at 11:02', tool: '', mono: false, failed: false,
      reasoning: '', reasoning_ms: 0 });
    drawThread(cur);
  });
  await wait(240);
  const going = await page.evaluate((was) => {
    var card = document.querySelector('.flycard');
    var bubble = document.querySelector('.turn[data-mid="k3"] .bubble');
    if (!card || !bubble) { return { missing: 'the copy or its message went away' }; }
    var c = card.getBoundingClientRect();
    var b = bubble.getBoundingClientRect();
    var box = el.draft.getBoundingClientRect();
    var near = Math.abs(b.right - c.right);
    var far = Math.abs(was.room - was.startRight);
    var out = { nearer: near < far, above: c.top < box.top - 4, near: Math.round(near),
      far: Math.round(far), top: Math.round(c.top), startTop: was.startTop,
      held: document.querySelectorAll('.landing').length };
    dropFly();
    return out;
  }, way);
  if (going.missing) { note(going.missing); }
  else {
    if (!going.nearer) {
      note('the copy did not come to the corner its message stands in: ' + going.near
        + ' from it, was ' + going.far);
    }
    if (!going.above) { note('the copy did not leave the box: top ' + going.top); }
    if (!(going.top < going.startTop)) { note('the copy did not go up the chat'); }
  }
  console.log('way:', JSON.stringify(going));

  // 10: a copy that cannot read the words it carries still takes the message
  // that came in. The newest message that was not in the chat when the words went
  // out is the message they became.
  const blind = await page.evaluate(() => {
    var was = flyWordsMatch;
    window.__match = was;
    flyWordsMatch = function () { return false; };
    cur = { chatting: { send_anim: true }, chat: { id: 'chat-four', title: 'Fourth chat',
      home: false }, messages: [
        { id: 'n1', role: 'You', body: 'A question with marks of **weight** in it',
          when: '12:00', when_full: 'today at 12:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 } ] };
    readChatting(cur);
    drawThread(cur);
    startFly('A question with marks of **weight** in it');
    cur.messages.push({ id: 'n2', role: 'You', body: 'A question with marks of **weight** in it',
      when: '12:01', when_full: 'today at 12:01', tool: '', mono: false, failed: false,
      reasoning: '', reasoning_ms: 0 });
    drawThread(cur);
    return { flying: !!window.fly };
  });
  await wait(80);
  const taken = await page.evaluate(() => {
    var out = { held: document.querySelectorAll('.landing').length,
      mid: window.fly ? window.fly.mid : '' };
    dropFly();
    flyWordsMatch = window.__match;
    return out;
  });
  if (taken.held !== 1 || taken.mid !== 'n2') {
    note('a copy that could not read its words did not take the message that came in: '
      + JSON.stringify(taken));
  }
  console.log('blind:', JSON.stringify({ start: blind, taken: taken }));

  // 11: a page that is not seen gets no journey at all, for a page behind another
  // is given no steps and a copy there would hold its message out of sight.
  const hidden = await page.evaluate(() => {
    dropFly();
    var on = Object.getOwnPropertyDescriptor(Document.prototype, 'hidden');
    Object.defineProperty(document, 'hidden', { configurable: true, get: function () { return true; } });
    startFly('Words written while the window stood behind another');
    var made = { card: document.querySelectorAll('.flycard').length, fly: !!window.fly };
    Object.defineProperty(document, 'hidden', { configurable: true, get: on.get });
    return made;
  });
  if (hidden.card || hidden.fly) { note('a copy was made for a page that is not seen'); }
  console.log('hidden:', JSON.stringify(hidden));

  // 12: a page that is given no steps at all still lets its message through. The
  // bound is kept by a timer, and a copy that cannot be moved is given up when the
  // bound comes, so no message is held out of sight by a journey that stopped.
  const stalled = await page.evaluate(() => {
    dropFly();
    window.__raf = window.requestAnimationFrame;
    window.requestAnimationFrame = function () { return 0; };
    cur = { chatting: { send_anim: true }, chat: { id: 'chat-five', title: 'Fifth chat',
      home: false }, messages: [
        { id: 'p1', role: 'You', body: 'Words the page cannot carry', when: '13:00',
          when_full: 'today at 13:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 } ] };
    readChatting(cur);
    drawThread(cur);
    startFly('Words the page cannot carry');
    cur.messages.push({ id: 'p2', role: 'You', body: 'Words the page cannot carry',
      when: '13:01', when_full: 'today at 13:01', tool: '', mono: false, failed: false,
      reasoning: '', reasoning_ms: 0 });
    drawThread(cur);
    return { card: document.querySelectorAll('.flycard').length,
      held: document.querySelectorAll('.landing').length };
  });
  if (stalled.card !== 1 || stalled.held !== 1) {
    note('the stalled journey did not hold its message back as it should: '
      + JSON.stringify(stalled));
  }
  await wait(1000);
  const stillHeld = await page.evaluate(() => ({
    card: document.querySelectorAll('.flycard').length,
    held: document.querySelectorAll('.landing').length
  }));
  if (!stillHeld.card || !stillHeld.held) {
    note('the stalled journey gave up too soon: ' + JSON.stringify(stillHeld));
  }
  await wait(4400);
  const freed = await page.evaluate(() => {
    window.requestAnimationFrame = window.__raf;
    var turn = document.querySelector('.turn[data-mid="p2"]');
    return {
      card: document.querySelectorAll('.flycard').length,
      held: document.querySelectorAll('.landing').length,
      flying: !!window.fly,
      seen: turn ? getComputedStyle(turn).visibility : 'gone'
    };
  });
  if (freed.card) { note('a copy was left over the page after its bound came'); }
  if (freed.flying) { note('the page still waits for a journey whose bound came'); }
  if (freed.held) { note('a message is still held back after the bound came'); }
  if (freed.seen !== 'visible') { note('the message of a stalled journey is not seen: ' + freed.seen); }
  console.log('stalled:', JSON.stringify({ at: stalled, after1s: stillHeld, atBound: freed }));

  await browser.close();
  console.log(trouble.length ? 'FAIL ' + trouble.length + ' faults' : 'ok   the journey of a sent message');
  process.exit(trouble.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(1); });
