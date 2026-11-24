// The skin a first run opens in. The skin is decided in the settings of the program and not in
// the page - the page is told which one to paint - so this is the one item of the page that no
// hand-written state can be a first run for: it asks a program that was started with an empty
// place for its data what it says its theme is, paints what it is told, and then has a theme
// chosen by hand to show that a choice of the user is what rules after that.
//
// Run it beside a program of your own, in a place of the data no run was in before:
//
//   mkdir -p /tmp/an-fresh && autumn-natter --remote --port 8799 --data-dir /tmp/an-fresh
//   AN_URL=http://127.0.0.1:8799 node tools/default-theme-live.js
//
// and read the two images it writes beside it: `default-run-dark.png`, the page as a first run
// paints it, and `default-chosen-light.png`, the same page after the pale theme was chosen.
//
// With no AN_URL the check records it and leaves: the item is of a program that is running, and
// the standing rules of the skin - the word of a theme, the switch of the two that was before
// the third, and the skin of a first run - are checked in Rust, by
// `config::tests::the_word_of_a_skin_is_kept_or_read_as_the_pale_one` and
// `engine::tests::the_skin_of_the_pages_keeps_the_old_switch_in_step`, which need no program and
// run with every `cargo test`.
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');

const URL = (process.env.AN_URL || '').replace(/\/$/, '');
const shell = process.env.AN_SHELL_BIN || '';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// What the program says of the skin it paints in, and of the switch of the two that was before
// the third. They are asked of the state the page is painted from, so a page that disagrees with
// its program is caught here rather than looked for in the picture.
async function told(what) {
  const where = `${URL}/api/${what === 'state' ? 'state' : what}`;
  const answer = await fetch(where);
  if (!answer.ok) { throw new Error(`the program answered ${answer.status} at ${where}`); }
  return answer.json();
}

async function main() {
  if (!URL) {
    console.log('SKIP the skin of a first run is only to be seen in a program that is running:'
      + ' give its address with AN_URL=http://127.0.0.1:8799');
    process.exit(0);
  }
  let trouble = 0;
  const note = (what) => { trouble++; console.log('  ERROR: ' + what); };

  const first = await told('state');
  console.log('  a first run says its theme is "' + first.theme + '"');
  if (first.theme !== 'dark') { note('a first run opened in "' + first.theme + '", not in dark'); }

  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 860 });
  const faults = [];
  page.on('pageerror', (e) => faults.push(String(e)));
  await page.goto(URL + '/', { waitUntil: 'networkidle2' });
  await sleep(1200);
  if (faults.length) { note('page faults: ' + faults.join(' | ')); }

  // The page as a first run paints it: the dark ground, and the word of the theme on the marker
  // of the themes.
  const painted = await page.evaluate(function () {
    return {
      dark: document.body.classList.contains('dark'),
      silver: document.body.classList.contains('silver'),
      corporate: document.body.classList.contains('corporate'),
      neon: document.body.classList.contains('neon'),
      ground: getComputedStyle(document.body).backgroundColor,
      said: (document.querySelector('#themebtn') || {}).getAttribute
        ? (document.querySelector('#themebtn').getAttribute('data-tip')
          || document.querySelector('#themebtn').title || '') : '',
    };
  });
  console.log('  the page paints the dark skin: ' + painted.dark + ', ground ' + painted.ground
    + ', the marker of the themes says "' + painted.said + '"');
  if (!painted.dark) { note('the page of a first run is not painted in the dark skin'); }
  if (!/dark/i.test(painted.said)) { note('the marker of the themes does not say dark: "'
    + painted.said + '"'); }
  await page.screenshot({ path: path.join(__dirname, 'default-run-dark.png') });

  // A theme the user chooses is kept, and rules from then on. The pale theme is chosen because it
  // is the skin the program is cut for and no longer starts in: were the default still only
  // painted, this is where it would be found.
  await fetch(URL + '/api/theme', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ theme: 'light', pixel: false }),
  });
  await sleep(1200);
  const chosen = await page.evaluate(function () {
    return {
      dark: document.body.classList.contains('dark'),
      ground: getComputedStyle(document.body).backgroundColor,
    };
  });
  const after = await told('state');
  console.log('  after the pale theme was chosen: dark skin ' + chosen.dark + ', ground '
    + chosen.ground + '; the program says "' + after.theme + '"');
  if (chosen.dark) { note('the page is left in the dark skin after the pale theme was chosen'); }
  if (after.theme !== 'light') { note('the program did not keep the theme that was chosen: "'
    + after.theme + '"'); }
  await page.screenshot({ path: path.join(__dirname, 'default-chosen-light.png') });

  await browser.close();
  console.log(trouble ? 'ERRORS: ' + trouble
    : 'ok: a first run opens dark, and a theme chosen by hand is kept over it');
  process.exit(trouble ? 1 : 0);
}

main().catch(function (e) { console.error('ERROR: ' + (e && e.stack || e)); process.exit(1); });
