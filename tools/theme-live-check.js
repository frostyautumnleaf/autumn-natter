// A theme chosen on a program that is really running holds until the program agrees to it.
//
// The page of the remote mode is polled, and the state it reads decides the colours of the page.
// A theme change is written to the program and put on the page at once, so the change has to
// survive two things: the state that was on its way when the theme was clicked, which carries the
// theme the user left, and the time the program takes to write its settings file. This check does
// with a real program what tools/theme-ask-check.js does with a state written by hand - and it is
// the slower of the two, because a disk is in the way.
//
// Every one of the five themes is put on by clicking the marker of the top bar, and the class of
// the body is watched at every frame after each click: the theme that was left must never be seen
// again once the theme of the click is on, the theme of the click must be the theme the page is
// in at the end, and the program must have written it down. The pixel-art form is put on and off
// the same way, and the form must never be taken back by a state that does not know of it.
//
//   AN_URL=http://<address>:<port> node theme-live-check.js
//   CHECK=tools/theme-live-check.js tools/live-run.sh   # starts a program of its own
'use strict';
const puppeteer = require('puppeteer');

const URL = process.env.AN_URL;
if (!URL) {
  console.log('SKIP theme-live-check: AN_URL is not set, so no program is asked for its state.'
    + ' Start the program with --remote and give its address: AN_URL=http://<address>:<port>'
    + ' node tools/theme-live-check.js. CHECK=tools/theme-live-check.js tools/live-run.sh starts'
    + ' one of its own.');
  process.exit(0);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const THEMES = ['light', 'dark', 'silver', 'corporate', 'neon'];

// Watch what the page is drawn in, once a frame: a theme that flickers back for a few hundred
// milliseconds is over between two polls and must not be missed.
const WATCH = function () {
  window.__seen = [];
  const body = document.body;
  const look = function () {
    const names = ['dark', 'silver', 'corporate', 'neon'];
    let theme = 'light';
    for (const one of names) { if (body.classList.contains(one)) { theme = one; } }
    return { theme: theme, pixel: body.classList.contains('pixel') };
  };
  window.__watchFrames = true;
  (function each() {
    if (!window.__watchFrames) { return; }
    window.__seen.push(look());
    requestAnimationFrame(each);
  })();
};

const STOP = function () {
  window.__watchFrames = false;
  return { seen: window.__seen, word: window.themeWord, art: window.pixelArt };
};

// What the program holds, read straight out of a state ask.
const SERVER = async function () {
  const r = await fetch('/api/state');
  const s = await r.json();
  return { theme: s.theme, pixel: !!s.theme_pixel };
};

const nextWord = (word) => THEMES[(THEMES.indexOf(word) + 1) % THEMES.length];

(async function main() {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  const page = await browser.newPage();
  const faults = [];
  page.on('pageerror', function (e) { faults.push('page: ' + e.message); });

  await page.setViewport({ width: 1280, height: 860 });
  await page.goto(URL, { waitUntil: 'domcontentloaded' });
  await sleep(1200);

  // Each of the five themes, put on by the marker of the top bar.
  const cycles = [];
  for (let turn = 0; turn < 5; turn++) {
    const before = await page.evaluate(function () { return window.themeWord; });
    const wanted = nextWord(before);
    await page.evaluate(WATCH);
    await page.click('#themebtn');
    await sleep(2500);
    const after = await page.evaluate(STOP);
    const on = after.seen.findIndex(function (one) { return one.theme === wanted; });
    const late = on >= 0
      ? after.seen.slice(on).filter(function (one) { return one.theme === before; }).length
      : after.seen.filter(function (one) { return one.theme === before; }).length;
    if (late > 0) {
      faults.push('the theme left (' + before + ') came back ' + late + ' time(s) after '
        + wanted + ' was put on');
    }
    if (after.word !== wanted) {
      faults.push('the page ended in ' + after.word + ' and not in the theme clicked (' + wanted + ')');
    }
    const server = await page.evaluate(SERVER);
    if (server.theme !== wanted) {
      faults.push('the program holds ' + server.theme + ' and not ' + wanted);
    }
    cycles.push({ from: before, to: wanted, frames: after.seen.length, late: late });
  }

  // The pixel-art form of the theme: on, then off. A state that has not heard of the change must
  // not take it back, and the program must hold it.
  const forms = [];
  for (const ask of [true, false]) {
    await page.evaluate(function (on) { window.chooseThemeArt(on); }, ask);
    await page.evaluate(WATCH);
    await sleep(2000);
    const after = await page.evaluate(STOP);
    const lost = after.seen.filter(function (one) { return one.pixel !== ask; }).length;
    if (lost > 0) {
      faults.push('the pixel-art form asked for (' + ask + ') was taken off ' + lost
        + ' time(s) while the state was read again');
    }
    if (after.art !== ask) {
      faults.push('the page ended with the form ' + after.art + ' and not ' + ask);
    }
    const server = await page.evaluate(SERVER);
    if (server.pixel !== ask) {
      faults.push('the program holds the form ' + server.pixel + ' and not ' + ask);
    }
    forms.push({ asked: ask, frames: after.seen.length, lost: lost });
  }

  await browser.close();
  if (faults.length) {
    faults.forEach(function (line) { console.log('ERROR: ' + line); });
    process.exit(1);
  }
  console.log('the themes put on: ' + cycles.map(function (one) {
    return one.from + '->' + one.to;
  }).join(', '));
  console.log('the forms put on: ' + forms.map(function (one) {
    return String(one.asked);
  }).join(', '));
  console.log('a theme chosen on a running program holds until the program agrees to it');
})();
