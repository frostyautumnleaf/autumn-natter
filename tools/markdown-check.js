// Look at the blocks of the markdown, as the chat cuts them and as the page
// puts them down. The page is loaded as it stands, with no bridge and no
// engine: the block cutter, the maker of the nodes, and the writer of an answer
// that is coming in are driven by hand.
//
// Three things are looked at. A table and a quote are cut into blocks of their
// own, and into the right ones. The nodes they make say the same words, in the
// same order, with the head where the marks said it was, and no mark of the
// table or the quote left for the reader. And an answer that grows a few
// characters at a time keeps every word of a table and of a quote on the page,
// so the writer that puts the words of the last block on again never loses one.
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

const PAGE = process.env.AN_PAGE
  ? path.resolve(process.env.AN_PAGE)
  : path.join(__dirname, '..', 'frontend', 'index.html');

(async () => {
  const shell = process.env.AN_SHELL_BIN;
  const browser = await puppeteer.launch({
    headless: 'shell',
    executablePath: shell || undefined,
    args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-gpu']
  });
  const page = await browser.newPage();
  await page.goto('file://' + PAGE);
  const faults = await page.evaluate(() => {
    var found = [];
    function say(fault) { found.push(fault); }

    // One message of the markdown, what the blocks of it should be, and what
    // the page should say of it.
    var TABLE = 'The columns stand as the marks say:\n\n'
      + '| file | lines | left |\n'
      + '| :--- | ---: | :-: |\n'
      + '| web.rs | 12 | 3 |\n'
      + '| page.html | 304 | 41 |\n\n'
      + 'The last row is the longest.';

    var QUOTE = 'The model answers:\n\n'
      + '> A quote of another voice.\n'
      + '> It goes on over lines.\n>\n'
      + '> - with a list inside\n'
      + '> - and one more thing\n'
      + '>\n'
      + '> ```js\n> var a = 1;\n> ```\n\n'
      + 'And words of its own after it.';

    var kindsOf = function (made) {
      return made.map(function (one) {
        return one.kind + (one.level || '') + (one.blocks ? '(' + kindsOf(one.blocks) + ')' : '');
      }).join(' ');
    };

    // 1. The blocks the marks are cut into.
    var table = blocks(TABLE);
    if (kindsOf(table) !== 'text table text') {
      say('a table did not come between two blocks of words: ' + kindsOf(table));
    }
    var one = table[1] || {};
    if (one.head.join(',') !== 'file,lines,left') {
      say('the head of the table is ' + JSON.stringify(one.head));
    }
    if ((one.align || []).join(',') !== 'l,r,c') {
      say('the columns do not stand as the colons said: ' + JSON.stringify(one.align));
    }
    if ((one.rows || []).length !== 2 || ((one.rows || [])[1] || []).join(',') !== 'page.html,304,41') {
      say('the rows of the table are wrong: ' + JSON.stringify(one.rows));
    }

    var quote = blocks(QUOTE);
    if (kindsOf(quote) !== 'text quote(text list code) text') {
      say('a quote did not come between two blocks of words: ' + kindsOf(quote));
    }
    var inner = (quote[1] || {}).blocks || [];
    if (inner.length !== 3) {
      say('the quote holds ' + inner.length + ' blocks, not three');
    } else {
      if (inner[0].kind !== 'text' || inner[0].text.indexOf('\n') < 0) {
        say('the first words of the quote are not one block over two lines');
      }
      if (inner[1].kind !== 'list' || inner[1].items.length !== 2) {
        say('a list did not stand inside the quote');
      }
      if (inner[2].kind !== 'code' || inner[2].text.indexOf('var a = 1;') < 0) {
        say('a block of code did not stand inside the quote, or its marks came with it');
      }
    }

    // 2. The marks that must not be taken for a table.
    var RULE = 'Words above.\n\n---\n\nWords below.';
    if (kindsOf(blocks(RULE)).indexOf('table') >= 0) {
      say('three marks on their own were taken for a table: ' + kindsOf(blocks(RULE)));
    }
    var NOHEAD = 'a | b\nnot a delimiter\nc | d';
    if (kindsOf(blocks(NOHEAD)) !== 'text') {
      say('a line of walls with no line of dashes under it was taken for a table: '
        + kindsOf(blocks(NOHEAD)));
    }
    var NARROW = '| a | b |\n| - | - |\n| 1 |';
    var narrow = blocks(NARROW)[0] || {};
    if ((narrow.rows || []).join(',') !== '1,' || (narrow.width || 0) !== 2) {
      say('a row with a cell missing did not end up as wide as the table: '
        + JSON.stringify(narrow.rows) + ' of width ' + narrow.width);
    }
    var ESCAPED = '| a \\| b | c |\n| --- | --- |\n| x | y |';
    var esc = blocks(ESCAPED)[0] || {};
    if ((esc.head || []).join(',') !== 'a | b,c') {
      say('a pipe with a line before it was taken for a wall: ' + JSON.stringify(esc.head));
    }

    // 3. The nodes, and the words they say. The nodes stand in the page, for a
    // look at the style of a thing that is not on the page says nothing of the
    // style it would take there.
    var stage = document.createElement('div');
    stage.className = 'turn t-ai';
    var words = document.createElement('div');
    words.className = 'plain';
    stage.appendChild(words);
    document.body.appendChild(stage);
    function said(made) {
      var box = document.createElement('div');
      for (var n = 0; n < made.length; n++) { box.appendChild(blockNode(made[n])); }
      words.appendChild(box);
      return box;
    }
    var host = said(table);
    var cellText = function (box) {
      var rows = box.querySelectorAll('table.mdtable tr'), out = [];
      for (var r = 0; r < rows.length; r++) {
        var cells = rows[r].children, line = [];
        for (var c = 0; c < cells.length; c++) { line.push(cells[c].textContent); }
        out.push(line.join(','));
      }
      return out.join(' / ');
    };
    if (cellText(host) !== 'file,lines,left / web.rs,12,3 / page.html,304,41') {
      say('the table on the page says something else: ' + cellText(host));
    }
    var marks = host.querySelector('table.mdtable');
    if (!marks) { say('no table stands on the page'); }
    var heads = host.querySelectorAll('table.mdtable thead th');
    if (heads.length !== 3 || heads[0].tagName !== 'TH') {
      say('the head of the table is not made of head cells: ' + heads.length);
    }
    var aligned = host.querySelector('table.mdtable tbody tr td:nth-child(2)');
    if (!aligned || window.getComputedStyle(aligned).textAlign !== 'right') {
      say('the second column does not stand to the right as its colon said');
    }
    var centred = host.querySelector('table.mdtable tbody tr td:nth-child(3)');
    if (!centred || window.getComputedStyle(centred).textAlign !== 'center') {
      say('the third column does not stand in the middle as its colons said');
    }
    if (host.textContent.indexOf('|') >= 0) {
      say('a wall of the table reached the reader');
    }
    var quoted = said(blocks(QUOTE));
    if (quoted.querySelector('.quote .marks li').textContent !== 'with a list inside') {
      say('the list inside the quote lost its words');
    }
    if (/(^|\n)\s*>/.test(quoted.querySelector('.quote').textContent)) {
      say('the mark of the quote reached the reader');
    }
    if (quoted.querySelectorAll('.quote').length !== 1) {
      say('the quote was made more than once');
    }
    // The words of the quote are dim, and the bar of it is drawn: a quote that
    // reads exactly like the words about it is a quote nobody sees.
    var bar = window.getComputedStyle(quoted.querySelector('.quote')).borderLeftWidth;
    if (parseFloat(bar) < 2) {
      say('the quote stands without a rule of colour beside it');
    }
    var ground = window.getComputedStyle(host.querySelector('.tablewrap')).backgroundColor;
    if (ground === 'rgba(0, 0, 0, 0)' || ground === 'transparent') {
      say('the table has no ground of its own');
    }

    // A table of more columns than the chat is wide is moved sideways inside its
    // frame, and does not push the chat wider.
    var WIDE = '| one | two | three | four | five | six | seven | eight |\n'
      + '| --- | --- | --- | --- | --- | --- | --- | --- |\n'
      + '| a long word in every cell | a long word in every cell | a long word | '
      + 'a long word | a long word | a long word | a long word | a long word |';
    var chat = document.createElement('div');
    chat.className = 'plain';
    chat.style.width = '420px';
    stage.appendChild(chat);
    chat.appendChild(said(blocks(WIDE)));
    var frame = chat.querySelector('.tablewrap');
    if (!frame) { say('a wide table made no frame of its own'); }
    else {
      if (frame.getBoundingClientRect().width > 421) {
        say('a wide table pushed the chat wider than it is: '
          + Math.round(frame.getBoundingClientRect().width) + ' of 420');
      }
      // The words of a cell break to fit the chat, as the words of a block of
      // code do, so a table of many columns is read without moving sideways.
      if (frame.scrollWidth > frame.clientWidth) {
        say('a wide table stands wider than the frame it stands in');
      }
    }
    // A table may stand inside a quote, as anything else of the markdown may.
    var INQUOTE = '> | a | b |\n> | --- | --- |\n> | 1 | 2 |';
    if (kindsOf(blocks(INQUOTE)) !== 'quote(table)') {
      say('a table did not stand inside a quote: ' + kindsOf(blocks(INQUOTE)));
    }

    // 4. An answer that grows a few characters at a time. Whatever the marks
    // say, the page must say the same words.
    var samples = [TABLE, QUOTE,
      '| one | two |\n| --- | --- |\n| a',
      '> only a quote, and nothing after it',
      '> | a | b |\n> | --- | --- |\n> | 1 | 2 |',
      'Words.\n\n| a | b |\n| :--- | --- |\n| 1 | 2 |\n\n- a list after the table\n',
      '> ## A heading inside a quote\n>\n> Words after the heading.'];
    for (var s = 0; s < samples.length; s++) {
      var text = samples[s];
      var state = { shape: '' };
      var box = words.appendChild(document.createElement('div'));
      for (var n = 1; n <= text.length; n += 3) {
        var part = text.slice(0, n);
        writeBlocks(box, state, part);
        var want = said(blocks(part));
        if (box.textContent !== want.textContent) {
          say('at ' + n + ' characters the page lost words of the answer ' + s + '\n  page: '
            + JSON.stringify(box.textContent) + '\n  want: ' + JSON.stringify(want.textContent));
          break;
        }
        var pageRows = box.querySelectorAll('table.mdtable tr').length;
        // The head of a table is a row of its own, whether or not the rows
        // under it have come yet; and a table may stand inside a quote.
        var modelRows = 0;
        var countRows = function (made) {
          made.forEach(function (one) {
            if (one.kind === 'table') { modelRows += 1 + one.rows.length; }
            if (one.blocks) { countRows(one.blocks); }
          });
        };
        countRows(blocks(part));
        if (pageRows !== modelRows) {
          say('at ' + n + ' characters the table on the page has ' + pageRows
            + ' rows and the marks say ' + modelRows + ' (sample ' + s + ')');
          break;
        }
      }
      // The whole of it at the last, as the answer ends.
      writeBlocks(box, state, text);
      var whole = said(blocks(text));
      if (box.textContent !== whole.textContent) {
        say('the answer ' + s + ' ended with other words than the marks say');
      }
    }
    return found;
  });
  await browser.close();
  if (faults.length) { console.log('FAULTS\n' + faults.slice(0, 10).join('\n')); process.exit(1); }
  console.log('a table and a quote are cut into blocks of their own, stand on the page with '
    + 'their head, their walls and their rule, and keep every word while they are written');
})();
