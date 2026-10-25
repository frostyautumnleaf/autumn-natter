
// A look at the side bar and the first screen, without the desktop or the model.
//
// The page is loaded as it stands, the lists of the bar are handed to it by
// hand, and the numbers of the layout are written out: whether the chat that
// is open has its name in bold, and how much room stands above and below the
// group of the first screen.
//
//   node side-check.js                       # both questions, pale skin
//   node side-check.js dark                  # the dark skin
//   node side-check.js 1440 900              # another window
//   node side-check.js shot side.png         # a picture of the bar, scrolled to
//     the chat that is open, so the bold of its name is seen
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const dark = process.argv.includes('dark');
// The measure of the window, taken from whichever two numbers are given.
const nums = process.argv.slice(2).filter((a) => /^\d+$/.test(a)).map(Number);
const W = nums[0] || 1180;
const H = nums[1] || 760;

function chats(n, openAt, repo) {
  const out = [];
  for (let i = 0; i < n; i++) {
    out.push({
      id: (repo || 'l') + '-' + i,
      title: 'Chat ' + (i + 1) + ' about the layout of the window',
      repo: repo || '', when: 'Today 09:0' + (i % 10),
      pinned: false, archived: false, active: i === openAt,
      messages: 4, preview: 'a line of the talk'
    });
  }
  return out;
}

const OPEN_AT = 3;
const LISTS = {
  loose: chats(18, 3, ''),
  repo1: chats(3, -1, 'r1'),
  repo2: chats(7, -1, 'r2')
};

(async () => {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  const page = await browser.newPage();
  await page.setViewport({ width: W, height: H });
  page.on('pageerror', (e) => console.log('PAGEERROR', String(e)));
  await page.goto('file://' + path.resolve(__dirname, '..', 'frontend', 'index.html'));
  await new Promise((r) => setTimeout(r, 500));

  const out = await page.evaluate(({ dark, openAt, L, D }) => {
    if (dark) { document.body.classList.add('dark'); }
    const loose = L.loose;
    lists = {
      repositories: [
        { id: 'r1', name: 'autumn-natter', pinned: true, chats: L.repo1 },
        { id: 'r2', name: 'parser-notes', pinned: false, chats: L.repo2 }
      ],
      loose: loose,
      archived: []
    };
    listsGot = 1;
    side.open = true;
    lastView = { chat_id: loose[openAt] ? loose[openAt].id : '', structure_rev: 1 };
    drawSide(lastView);

    const on = document.querySelector('#sideBody .srow.on');
    const off = document.querySelectorAll('#sideBody .srow:not(.on) .stitle')[0];
    const cs = on ? getComputedStyle(on.querySelector('.stitle')) : null;
    const rows = document.querySelectorAll('#looseBody .srow');
    const box = document.getElementById('sideBody').getBoundingClientRect();
    const last = rows.length ? rows[rows.length - 1].getBoundingClientRect() : null;

    // The first screen: the group of the mark, the words, and the chat box.
    document.body.classList.add('home');
    document.getElementById('greet').textContent = 'Good morning. What shall we work on?';
    document.getElementById('homenote').innerHTML =
      '<b>Qwen3.5-0.8B</b> · GGUF · 8,192 tokens of context · loose chats';
    const area = document.getElementById('area').getBoundingClientRect();
    const hello = document.querySelector('.hello').getBoundingClientRect();
    const dock = document.getElementById('dock').getBoundingClientRect();
    const mark = document.querySelector('.hello .mark').getBoundingClientRect();
    const line = document.querySelector('.state-line').getBoundingClientRect();
    return {
      win: D,
      sideRows: document.querySelectorAll('#sideBody .srow').length,
      looseShown: rows.length, sideWindow: side.window, fit: side.fit,
      hasOn: !!on,
      onCount: document.querySelectorAll('#sideBody .srow.on').length,
      onTitle: on ? on.querySelector('.stitle').textContent : null,
      weightOn: cs && cs.fontWeight,
      weightOff: off && getComputedStyle(off).fontWeight,
      family: cs && cs.fontFamily.split(',')[0],
      lastRowBottom: last && Math.round(last.bottom),
      boxBottom: Math.round(box.bottom),
      areaH: Math.round(area.height),
      gapBoxAbove: Math.round(hello.top - area.top),
      gapBoxBelow: Math.round(area.bottom - dock.bottom),
      gapInkAbove: Math.round(mark.top - area.top),
      gapInkBelow: Math.round(area.bottom - line.bottom),
      dockTop: Math.round(dock.top), dockH: Math.round(dock.height)
    };
  }, { dark, openAt: OPEN_AT, L: LISTS, D: W + 'x' + H });

  // The lists a moment behind the window: the chat that stood open before is
  // still marked open in them, and the bar must set apart only the chat the
  // window is looking at now.
  const stale = await page.evaluate(({ L }) => {
    const loose = L.loose.map((c, i) => Object.assign({}, c, { active: i === 1 }));
    lists = { repositories: [], loose: loose, archived: [] };
    listsGot = 2;
    lastView = { chat_id: loose[4].id };
    sideStamp = '';
    drawSide(lastView);
    const marked = Array.from(document.querySelectorAll('#sideBody .srow.on'))
      .map((r) => r.querySelector('.stitle').textContent);
    const w = document.querySelector('#sideBody .srow.on .stitle');
    return { onCount: marked.length, marked: marked,
      weight: w && getComputedStyle(w).fontWeight,
      other: (() => { const o = document.querySelector('#sideBody .srow:not(.on) .stitle');
        return o && getComputedStyle(o).fontWeight; })() };
  }, { L: LISTS });

  // The three dots of a row, asked for by the pointer: the day gives way to the
  // mark, the row keeps the height and the width it had, and the mark stands on
  // the same line as the day.
  if (process.argv.includes('hover')) {
    const rows = await page.$$('#looseBody .srow');
    const before = await page.evaluate(() => {
      const row = document.querySelector('#looseBody .srow');
      const g = (n) => { const b = n.getBoundingClientRect(); return { t: Math.round(b.top), r: Math.round(b.right), w: Math.round(b.width), h: Math.round(b.height) }; };
      return { row: g(row), day: g(row.querySelector('.sday')), dots: g(row.querySelector('.sdots')),
        dotsOpacity: getComputedStyle(row.querySelector('.sdots')).opacity,
        dotsWidth: getComputedStyle(row.querySelector('.sdots')).width };
    });
    await rows[1].hover();
    await new Promise((r) => setTimeout(r, 60));
    const during = await page.evaluate(() => {
      const row = document.querySelectorAll('#looseBody .srow')[1];
      const g = (n) => { const b = n.getBoundingClientRect(); return { t: Math.round(b.top), r: Math.round(b.right), w: Math.round(b.width), h: Math.round(b.height) }; };
      return { row: g(row), day: g(row.querySelector('.sday')), dots: g(row.querySelector('.sdots')),
        dotsOpacity: getComputedStyle(row.querySelector('.sdots')).opacity,
        dotsWidth: getComputedStyle(row.querySelector('.sdots')).width,
        inline: Math.abs((row.querySelector('.sdots').getBoundingClientRect().top +
          row.querySelector('.sdots').getBoundingClientRect().height / 2) -
          (row.querySelector('.sday').getBoundingClientRect().top +
           row.querySelector('.sday').getBoundingClientRect().height / 2)) < 3,
        transition: getComputedStyle(row.querySelector('.sdots')).transitionProperty };
    });
    // The move itself: the width of the mark is looked at while the pointer is
    // on the row, so a move that takes the time it was given can be told from a
    // snap from nought to full in one step.
    const steps = [];
    await rows[2].hover();
    for (let i = 0; i < 5; i++) {
      steps.push(await page.evaluate(() => {
        const d = document.querySelectorAll('#looseBody .srow')[2].querySelector('.sdots');
        return Math.round(d.getBoundingClientRect().width * 10) / 10;
      }));
      await new Promise((r) => setTimeout(r, 40));
    }
    const file = 'f7-hover.png';
    await page.screenshot({ path: path.resolve(__dirname, file), clip: { x: 0, y: 40, width: 300, height: 260 } });
    console.log(JSON.stringify({ before, during, steps, wrote: file }, null, 1));
    await browser.close();
    return;
  }

  const shot = process.argv.indexOf('shot');
  if (shot >= 0) {
    const file = process.argv[shot + 1] || 'side-check.png';
    await page.evaluate(() => {
      document.body.classList.remove('home');
      const on = document.querySelector('#sideBody .srow.on');
      const box = document.getElementById('sideBody');
      if (on) { box.scrollTop = on.offsetTop - box.offsetTop - 40; }
    });
    await new Promise((r) => setTimeout(r, 120));
    await page.screenshot({ path: path.resolve(__dirname, file) });
    console.log('wrote ' + file);
  }

  console.log(JSON.stringify(out, null, 1));
  console.log('stale lists:', JSON.stringify(stale, null, 1));
  await browser.close();
})();
