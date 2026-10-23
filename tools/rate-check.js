// Check the counter of the speed that stands beside the words of the state. Two
// kinds of work are at hand while an answer comes in: the words that were sent
// are read over, and then the model writes. The number beside the state is the
// number of the reading while the gauge of the reading stands, and the number of
// the writing after it; and it goes away when nothing is being done, or when the
// server has said nothing of speed. The page is loaded as it stands, with no
// bridge and no engine.
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE) : PAGE;

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + REMOTE);
  const out = await page.evaluate(async () => {
    var faults = [];
    function said(state) {
      drawRate(state);
      return { shown: !el.rate.classList.contains('hidden'), words: el.rate.textContent };
    }
    // The reading of the words that were sent, while the gauge stands. The
    // numbers are those a run of the model really gave: 817 tokens read in
    // 30.967 seconds, and 16 tokens written in 2.772 seconds.
    var read = said({ generating: true, progress: { active: true, reading: 26.4, writing: 0 } });
    if (!read.shown) { faults.push('the speed of the reading was hidden away'); }
    if (read.words.indexOf('26') < 0 || read.words.indexOf('reading') < 0) {
      faults.push('the speed of the reading reads: ' + read.words);
    }
    // The writing of the answer, after the gauge went.
    var wrote = said({ generating: true, progress: { active: false, reading: 26.4, writing: 5.77 } });
    if (!wrote.shown) { faults.push('the speed of the writing was hidden away'); }
    if (wrote.words.indexOf('5.8') < 0 || wrote.words.indexOf('writing') < 0) {
      faults.push('the speed of the writing reads: ' + wrote.words);
    }
    // Nothing being done, and nothing to say.
    var still = said({ generating: false, progress: { active: false, reading: 26.4, writing: 5.77 } });
    if (still.shown) { faults.push('the speed stood though nothing is being done: ' + still.words); }
    var mute = said({ generating: true, progress: { active: true, reading: 0, writing: 0 } });
    if (mute.shown) { faults.push('a speed of nothing was shown: ' + mute.words); }
    var none = said({ generating: true });
    if (none.shown) { faults.push('a speed was shown though the state says nothing of one'); }
    return faults;
  });
  await browser.close();
  if (out.length) {
    console.error(out.join('\n'));
    process.exit(1);
  }
  console.log('the speed beside the state: the reading while the gauge of the reading '
    + 'stands, the writing after it, and nothing at all when nothing is being done or '
    + 'the server says nothing of speed');
})().catch(function (e) { console.error(e); process.exit(1); });
