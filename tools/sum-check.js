
// A look at the sheet of a whole handoff summary, which shares the name `.shead`
// with the head of a group in the side bar. The words of the sheet stand at the
// left, as the sheet of settings does, and the head of the bar is not felt here.
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');
(async () => {
  const browser = await puppeteer.launch({ args: ['--no-sandbox'] });
  const page = await browser.newPage();
  await page.setViewport({ width: 1180, height: 760 });
  await page.goto('file://' + path.resolve(__dirname, '..', 'frontend', 'index.html'));
  await new Promise((r) => setTimeout(r, 400));
  const out = await page.evaluate(() => {
    const view = document.querySelector('.sumview');
    if (!view) { return { error: 'no sheet of a summary in the page' }; }
    view.classList.remove('hidden');
    const head = view.querySelector('.shead');
    const title = view.querySelector('.stitle');
    const meta = view.querySelector('.smeta');
    if (title) { title.textContent = 'The handoff summary'; }
    if (meta) { meta.innerHTML = 'Made at 09:41 · <b>6,964</b> tokens made short to <b>412</b>'; }
    const g = (n) => { if (!n) { return null; } const b = n.getBoundingClientRect(); const c = getComputedStyle(n);
      return { left: Math.round(b.left), top: Math.round(b.top), w: Math.round(b.width), h: Math.round(b.height),
        pad: c.padding, cursor: c.cursor, align: c.alignItems, dir: c.flexDirection, bg: c.backgroundColor }; };
    return { head: g(head), title: g(title), meta: g(meta),
      titleAtLeftOfHead: title && head ? Math.round(title.getBoundingClientRect().left - head.getBoundingClientRect().left) : null };
  });
  console.log(JSON.stringify(out, null, 1));
  await browser.close();
})();
