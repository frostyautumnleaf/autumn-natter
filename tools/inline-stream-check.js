// Check that finishing inline Markdown does not fade existing streamed text again.
// Run with AN_PAGE_URL set to the native or remote page served over HTTP.
'use strict';

const puppeteer = require('puppeteer');

(async () => {
  const browser = await puppeteer.launch({
    headless: true,
    executablePath: process.env.AN_CHROME || '/usr/bin/chromium',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  try {
    const page = await browser.newPage();
    await page.goto(process.env.AN_PAGE_URL, { waitUntil: 'domcontentloaded' });
    await page.evaluate(() => {
      reduce = false;
      const cases = [
        'An earlier paragraph.\n\nNow use `hello` and continue.',
        'An earlier paragraph.\n\nNow use ``a `literal` value`` and continue.',
        'An earlier paragraph.\n\nSee [the page](https://example.com) for details.',
        'An earlier paragraph.\n\nNow write **bold text** and continue.'
      ];
      for (const source of cases) {
        const node = document.createElement('div');
        const state = { shape: '' };
        document.body.appendChild(node);
        let previousShape = '';
        for (let at = 1; at <= source.length; at++) {
          const text = source.slice(0, at);
          const nextShape = shapeOf(blocks(text));
          writeBlocks(node, state, text);
          if (previousShape && nextShape !== previousShape && node.querySelector('.ink, a.fade')) {
            throw new Error('Previously rendered text faded again at ' + JSON.stringify(text));
          }
          previousShape = nextShape;
        }
        const result = node.textContent;
        if (!result.startsWith('An earlier paragraph.') || !result.endsWith('continue.')
            && !result.endsWith('details.')) {
          throw new Error('Streamed text is missing: ' + result);
        }
        if (source.includes('`') && !node.querySelector('code.inline-code')) {
          throw new Error('Inline code styling was lost: ' + source);
        }
        node.remove();
      }
    });
    console.log('Streamed inline formatting keeps previously rendered text visible');
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exit(1); });
