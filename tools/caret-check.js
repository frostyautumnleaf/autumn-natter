
// Check that the mark of an answer that is coming in never ends up in the
// middle of the words. The page is loaded as it is, with no bridge and no
// engine: only the writer of the live answer is driven, a few characters at a
// time, and after every look the words behind the mark are looked at. The words
// of the answer hold a heading, a list, a rule and links, so the mark is looked
// at behind every kind of block.
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
      if (!caret.parentNode) { faults.push('the mark stands in nothing at ' + shown); continue; }
      // Nothing of the words may stand behind the mark.
      var after = caret.nextSibling;
      while (after) {
        var text = after.textContent || '';
        if (text.replace(/\s+$/g, '') !== '') {
          faults.push('words stand behind the mark at ' + shown + ': ' + JSON.stringify(text.slice(0, 30)));
          break;
        }
        after = after.nextSibling;
      }
      // The words before the mark are the words of the answer so far, and all
      // of them: the blocks of the part written, joined.
      // The words of the part written, as the reader sees them: the blocks
      // joined, with the marks of the writing taken out and the words of a link
      // standing for the link.
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
        faults.push('the words of the page differ at ' + shown + ': ' + JSON.stringify(got.slice(-40)) + ' not ' + JSON.stringify(want.slice(-40)));
      }
    }
    return faults;
  });
  await browser.close();
  if (bad.length) { console.log('FAULTS\n' + bad.slice(0, 8).join('\n')); process.exit(1); }
  console.log('the mark stayed behind the words at every look');
})();
