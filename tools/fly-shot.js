// One look at a copy on its way: the box, the copy, and the message it is going
// to, in one picture.
//   node fly-shot.js out.png
'use strict';
const path = require('path');
const puppeteer = require('puppeteer');
const REMOTE = process.env.AN_PAGE ? path.resolve(process.env.AN_PAGE)
  : path.join(__dirname, '..', 'frontend', 'index.html');
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
(async () => {
  const browser = await puppeteer.launch({ headless: 'shell',
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu'] });
  const page = await browser.newPage();
  await page.setViewport({ width: 1180, height: 820 });
  await page.goto('file://' + REMOTE);
  await wait(400);
  await page.evaluate(() => {
    cur = { chatting: { send_anim: true }, chat: { id: 'c1', title: 'A chat', home: false },
      messages: [
        { id: 'a1', role: 'You', body: 'What stands at the beginning?', when: '09:00',
          when_full: 'today at 09:00', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 },
        { id: 'a2', role: 'Assistant', body: 'A line, and then another.', when: '09:01',
          when_full: 'today at 09:01', tool: '', mono: false, failed: false,
          reasoning: '', reasoning_ms: 0 } ] };
    readChatting(cur);
    drawThread(cur);
    el.draft.value = 'And what stands at the end of it all, in a few words?';
    grow();
    send();
  });
  await wait(260);
  await page.evaluate(() => {
    cur.messages.push({ id: 'a3', role: 'You',
      body: 'And what stands at the end of it all, in a few words?', when: '09:02',
      when_full: 'today at 09:02', tool: '', mono: false, failed: false,
      reasoning: '', reasoning_ms: 0 });
    drawThread(cur);
  });
  await wait(260);
  await page.screenshot({ path: process.argv[2] || 'fly-shot.png' });
  await browser.close();
  console.log('wrote ' + (process.argv[2] || 'fly-shot.png'));
})().catch((e) => { console.error(e); process.exit(1); });
