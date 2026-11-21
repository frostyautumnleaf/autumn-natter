
// Check that the marker of an answer that is arriving never ends up in the middle of the text.
// The page is loaded as it is, with no bridge and no engine: only the writer of the live answer
// is driven, a few characters at a time, and after every look the text behind the marker are
// checked. The text of the answer hold a heading, a list, a rule and links, so the marker is
// checked behind every type of block.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = path.join(__dirname, '..', 'frontend', 'index.html');

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + PAGE);
  const bad = await page.evaluate(async () => {
    // The writer needs a turn to write into, and the state it reads.
    var host = document.createElement('div');
    host.className = 'turn t-ai';
    document.body.appendChild(host);
    var made = thinkPanel();
    var turn = { };
    stopInk();
    var plain = make('div', 'plain');
    host.appendChild(made.panel);
    host.appendChild(plain);
    ink.turn = host; ink.panel = made.panel; ink.thought = made.words;
    ink.words = plain; ink.caret = make('span', 'caret');
    ink.head = made.head; ink.label = made.label;
    plain.appendChild(ink.caret);
    var story = 'The kettle clicked on. And Mara stood in her kitchen holding a warning from her own future, feeling the floor tilt very slightly under her.\n\n## Chapter Two\n\n- first point\n- second point, with a [link](https://example.com/two) in it\n\n---\n\nShe wrote it down at <https://example.com/note>, and read [the first chapter](https://example.com/one) again. The end. For years after, she came to the lighthouse on Sundays.';
    var faults = [];
    for (var shown = 0; shown <= story.length; shown += 7) {
      ink.said = story;
      ink.shown = shown;
      writeSaid();
      var caret = ink.caret;
      if (!caret) { faults.push('the mark is gone at ' + shown); continue; }
      if (!caret.parentNode) { faults.push('the marker is in nothing at ' + shown); continue; }
      // Nothing of the text may be behind the marker.
      var after = caret.nextSibling;
      while (after) {
        var text = after.textContent || '';
        if (text.replace(/\s+$/g, '') !== '') {
          faults.push('text is behind the marker at ' + shown + ': ' + JSON.stringify(text.slice(0, 30)));
          break;
        }
        after = after.nextSibling;
      }
      // The text before the marker is the text of the answer so far, and all of it: the blocks
      // of the part written, joined. The text of the part written, as the user sees it: the
      // blocks joined, with the markers of the writing taken out and the text of a link
      // remaining for the link.
      var made = blocks(story.slice(0, shown));
      var want = '';
      var said = function (list) {
        for (var b = 0; b < list.length; b++) {
          var block = list[b];
          if (block.kind === 'list') {
            for (var it = 0; it < block.items.length; it++) {
              want += inlineWords(inlineParts(block.items[it].text));
            }
          } else if (block.blocks) {
            said(block.blocks);
          } else {
            want += inlineWords(inlineParts(block.text || ''));
          }
        }
      };
      said(made);
      var got = plain.textContent || '';
      if (got !== want) {
        faults.push('the text of the page differ at ' + shown + ': ' + JSON.stringify(got.slice(-40)) + ' not ' + JSON.stringify(want.slice(-40)));
      }
    }
    return faults;
  });
  await browser.close();
  if (bad.length) { console.log('FAULTS\n' + bad.slice(0, 8).join('\n')); process.exit(1); }
  console.log('the marker stayed behind the text at every look');
})();
